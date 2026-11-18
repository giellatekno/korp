mod assets_upload;
mod base64;
mod cf_models;
pub mod childprocess;
mod cli;
mod docker;
mod lang;
mod manifest;

use std::path::Path;

use crate::assets_upload::deploy_to_cloudflare;
use crate::childprocess::{SubprocessError, child_exit_ok};
use crate::cli::{Args, Task};
use crate::docker::{DOCKER, ImageName};
use crate::lang::Lang;

/// File (in the current directory) with the Cloudflare account id, used when
/// neither --cf-account-id nor CLOUDFLARE_ACCOUNT_ID is given.
const ACCOUNT_ID_FILE: &str = "CLOUDFLARE_ACCOUNT_ID";

/// Directory (in tasks/) where wrangler is installed (node_modules), to run it from.
/// For now that's the korp-backend-fkv Worker, the only one there is.
/// TODO: wrangler setup shared by all languages, when the Workers are redone (task 4).
const WRANGLER_DIR: &str = "korp-backend-fkv";

/// Tag of the korp-backend image. The Containerfile clones korp-backend's dev
/// branch (version 9.0.0.dev0), so this doesn't pin anything.
/// TODO: derive it from the built image (korp-backend version + commit, CWB version).
const KORP_BACKEND_TAG: &str = "9.0.0-dev";

struct Config<'a> {
    cwd: &'a Path,
    args: &'a Args,
}

impl Config<'_> {
    /// The account id: --cf-account-id or CLOUDFLARE_ACCOUNT_ID if given, else the
    /// contents of the file CLOUDFLARE_ACCOUNT_ID in the current directory.
    fn cf_account_id(&self) -> anyhow::Result<String> {
        if let Some(id) = &self.args.cf_account_id {
            return Ok(id.trim().to_string());
        }
        let path = self.cwd.join(ACCOUNT_ID_FILE);
        match std::fs::read_to_string(&path) {
            Ok(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
            Ok(_) => anyhow::bail!("{} is empty", path.display()),
            Err(e) => anyhow::bail!(
                "no Cloudflare account id: give --cf-account-id, set CLOUDFLARE_ACCOUNT_ID, \
                 or put it in the file {} ({e})",
                path.display()
            ),
        }
    }

    fn cf_api_token(&self) -> anyhow::Result<String> {
        let path = &self.args.cf_api_token_file;
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(s.trim().to_string()),
            Err(e) => anyhow::bail!("can't read the Cloudflare API token file '{path}': {e}"),
        }
    }
}

/// The Docker build context: the korp repo root, i.e. the first of `cwd` and its
/// ancestors that contains `gtweb2_config/` (the Containerfiles COPY from it).
/// What gets sent is limited by `.dockerignore` in the repo root.
fn get_build_context_dir(cwd: &Path) -> std::io::Result<&Path> {
    let _span = tracing::debug_span!("get_build_context_dir()");
    let dir = cwd
        .ancestors()
        .find(|dir| dir.join("gtweb2_config").is_dir())
        .ok_or_else(|| std::io::Error::other("no gtweb2_config directory found in cwd or its parents"))?;
    tracing::debug!(?dir, "found context dir");
    Ok(dir)
}

fn frontend_image_name(lang: Lang) -> String {
    format!("korp-frontend-{lang}")
}

/// Run `docker build --load -f <containerfile> -t <tag>... [--build-arg <arg>...] <context>`,
/// with the repo root as the context, and `<root>/tasks/containerfiles/<containerfile>`.
fn docker_build(
    config: &Config,
    containerfile: &str,
    tags: &[&str],
    build_args: &[String],
) -> Result<(), SubprocessError> {
    let context = get_build_context_dir(config.cwd)?;
    let mut cmd = std::process::Command::new(DOCKER);
    // -f is relative to the process's cwd, not to the context
    let containerfile = context.join("tasks/containerfiles").join(containerfile);
    cmd.arg("build").arg("--load").arg("-f").arg(containerfile);
    for tag in tags {
        cmd.arg("-t").arg(tag);
    }
    for build_arg in build_args {
        cmd.arg("--build-arg").arg(build_arg);
    }
    cmd.arg(context);
    child_exit_ok(&cmd.status()?)?;
    Ok(())
}

/// Build the image korp-frontend-base (korp-frontend with its dependencies installed)
fn build_front_base(config: &Config) -> Result<(), SubprocessError> {
    tracing::info!("build container image 'korp-frontend-base'");
    docker_build(
        config,
        "Containerfile.frontend-base",
        &["korp-frontend-base"],
        &[],
    )
}

/// Build the image korp-frontend-LANG (the built frontend of one language, served by nginx)
fn build_front(config: &Config, lang: Lang) -> Result<(), SubprocessError> {
    build_front_base(config)?;
    let image_name = frontend_image_name(lang);
    tracing::info!("build container image '{image_name}'");
    docker_build(
        config,
        "Containerfile.frontend",
        &[&image_name],
        &[format!("instance={lang}")],
    )
}

/// Build the frontend of `lang`, copy the files out of the image into html/, and
/// upload them as the static-assets Worker korp-frontend-<lang>.
async fn deploy_front(config: &Config<'_>, lang: Lang) -> anyhow::Result<()> {
    // read these first, to fail before the (long) build
    let account_id = config.cf_account_id()?;
    let api_token = config.cf_api_token()?;

    let image_name = frontend_image_name(lang);
    let static_assets_dir = config.cwd.join("html");

    build_front(config, lang)?;

    // delete the static assets before copying the new one from the image, to avoid
    // potential stale files
    if static_assets_dir.is_dir() {
        std::fs::remove_dir_all(&static_assets_dir)?;
    } else if static_assets_dir.exists() {
        anyhow::bail!(
            "{} exists, but is not a directory, don't know what to do, can't continue",
            static_assets_dir.display()
        );
    }

    tracing::info!("copy html dir from image");
    let container_name = docker::run_detached(&image_name, None).await?;
    let dest = config.cwd.to_str().expect("utf-8 path");
    docker::copy(&format!("{container_name}:/usr/share/nginx/html"), dest).await?;
    docker::stop(&container_name).await?;

    deploy_to_cloudflare(&account_id, &api_token, &image_name, &static_assets_dir).await?;
    Ok(())
}

/// Build the backend image (one image for all languages). Tagged both as
/// korp-backend:<tag> and as <account registry>/korp-backend:<tag>.
fn build_back(config: &Config) -> anyhow::Result<ImageName> {
    tracing::info!("build container image 'korp-backend' from tasks/containerfiles/Containerfile.backend");
    let image_name = ImageName::builder()
        .registry(format!("registry.cloudflare.com/{}", config.cf_account_id()?))
        .name("korp-backend".to_string())
        .tag(KORP_BACKEND_TAG.to_string())
        .build()
        .expect("name is set");
    docker_build(
        config,
        "Containerfile.backend",
        &[&image_name.to_name_and_tag_string(), &image_name.to_string()],
        &[],
    )?;
    Ok(image_name)
}

/// Run `pnpm wrangler <args>` in `dir`, with the account id in the environment.
fn wrangler(config: &Config, dir: &Path, args: &[&str]) -> anyhow::Result<()> {
    tracing::info!(working_directory = %dir.display(), "run: 'pnpm wrangler {}'", args.join(" "));
    let status = std::process::Command::new("pnpm")
        .arg("wrangler")
        .args(args)
        .current_dir(dir)
        // also when the account id came from --cf-account-id or the file
        .env("CLOUDFLARE_ACCOUNT_ID", config.cf_account_id()?)
        .status()?;
    child_exit_ok(&status)?;
    Ok(())
}

/// Push the backend image to the Cloudflare container registry of the account,
/// with `wrangler containers push`.
fn push_back(config: &Config, image_name: &ImageName) -> anyhow::Result<()> {
    let image_name = image_name.to_name_and_tag_string();
    wrangler(
        config,
        &config.cwd.join(WRANGLER_DIR),
        &["containers", "push", &image_name],
    )
}

/// Deploy the API Worker of `lang` with `wrangler deploy` in korp-backend-<lang>/.
/// To be rewritten in task 4 (one Worker template for all languages).
fn deploy_back(config: &Config, lang: Lang) -> anyhow::Result<()> {
    let dir = config.cwd.join(format!("korp-backend-{lang}"));
    if !dir.is_dir() {
        anyhow::bail!("{} doesn't exist (only fkv has an API Worker so far)", dir.display());
    }
    wrangler(config, &dir, &["deploy"])
}

fn setup_tracing() {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_tracing();
    let args = cli::parse_args();
    let cwd = std::env::current_dir()?;
    let config = Config { cwd: &cwd, args: &args };

    match args.task {
        Task::BuildBack => {
            build_back(&config)?;
        }
        Task::BuildFront(lang) => build_front(&config, lang)?,
        Task::PushBack => {
            let image_name = build_back(&config)?;
            push_back(&config, &image_name)?;
        }
        Task::DeployBack(lang) => deploy_back(&config, lang)?,
        Task::DeployFront(lang) => deploy_front(&config, lang).await?,
    }

    Ok(())
}
