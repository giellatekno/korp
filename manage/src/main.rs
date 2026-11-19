mod cf_models;
pub mod childprocess;
mod cli;
mod corpora;
mod deploy;
mod docker;
mod lang;

use std::path::{Path, PathBuf};

use crate::childprocess::{SubprocessError, child_exit_ok};
use crate::cli::{Args, Task};
use crate::docker::{DOCKER, ImageName};
use crate::lang::Lang;

/// File (in the current directory) with the Cloudflare account id, used when
/// neither --cf-account-id nor CLOUDFLARE_ACCOUNT_ID is given.
const ACCOUNT_ID_FILE: &str = "CLOUDFLARE_ACCOUNT_ID";

/// Directory (in manage/) where wrangler is installed (node_modules), to run it
/// from: the Worker template of all languages (see src/deploy.rs).
const WRANGLER_DIR: &str = "worker";

/// Version part of the backend image tag, followed by the short korp-backend commit.
const KORP_BACKEND_VERSION: &str = "9.0.0-dev";

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

/// Run `docker build -f <root>/manage/containerfiles/<containerfile> [--build-arg <arg>...] <root>`,
/// with the repo root as the context. With `Output::Image`, the image is loaded
/// into docker with the given tags; with `Output::Dir`, the files of the final
/// stage are written into that directory instead (no image).
fn docker_build(config: &Config, containerfile: &str, output: Output, build_args: &[String]) -> Result<(), SubprocessError> {
    let context = get_build_context_dir(config.cwd)?;
    let mut cmd = std::process::Command::new(DOCKER);
    // -f is relative to the process's cwd, not to the context
    let containerfile = context.join("manage/containerfiles").join(containerfile);
    // --network host: the build's downloads (apt, npm, git) use the host's
    // network directly. Docker's own network assumes MTU 1500, so behind a VPN
    // with a smaller MTU (e.g. 1191) larger downloads hang and get reset.
    // The host's network for the RUN steps (downloads): docker's bridge network
    // has MTU 1500, which breaks downloads over a VPN with a smaller MTU (e.g.
    // tun0 with 1191: connections hang or are reset).
    cmd.arg("build").arg("--network").arg("host").arg("-f").arg(containerfile);
    match output {
        Output::Image(tags) => {
            cmd.arg("--load");
            for tag in tags {
                cmd.arg("-t").arg(tag);
            }
        }
        Output::Dir(dir) => {
            cmd.arg("--output").arg(format!("type=local,dest={}", dir.display()));
        }
    }
    for build_arg in build_args {
        cmd.arg("--build-arg").arg(build_arg);
    }
    cmd.arg(context);
    child_exit_ok(&cmd.status()?)?;
    Ok(())
}

/// The upstream commit a Containerfile pins: the default of its
/// `ARG <arg>=<sha>` line (the Containerfiles are the one place the pins are).
fn pinned_commit(config: &Config, containerfile: &str, arg: &str) -> anyhow::Result<String> {
    let path = get_build_context_dir(config.cwd)?.join("manage/containerfiles").join(containerfile);
    let text = std::fs::read_to_string(&path)?;
    let prefix = format!("ARG {arg}=");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|sha| sha.trim().to_string())
        .filter(|sha| sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("no `{prefix}<40-hex commit>` line in {}", path.display()))
}

enum Output<'a> {
    Image(&'a [&'a str]),
    Dir(&'a Path),
}

/// Where the built frontend of `lang` goes: worker/.build/<lang>/dist, next to
/// the language's generated wrangler.json, which serves it as static assets.
fn frontend_dir(config: &Config, lang: Lang) -> PathBuf {
    config.cwd.join(WRANGLER_DIR).join(".build").join(lang.as_str()).join("dist")
}

/// Build the frontend of `lang` (korp-vue + korp-vue-instance/ +
/// gtweb2_config/front/config-<lang>.yaml) into frontend_dir().
fn build_front(config: &Config, lang: Lang) -> anyhow::Result<PathBuf> {
    let dir = frontend_dir(config, lang);
    let commit = pinned_commit(config, "Containerfile.frontend", "KORP_VUE_COMMIT")?;
    tracing::info!("build the frontend of {lang} (korp-vue {}) into {}", &commit[..7], dir.display());
    // Start empty: no files left over from an earlier build
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    docker_build(
        config,
        "Containerfile.frontend",
        Output::Dir(&dir),
        &[format!("lang={lang}")],
    )?;
    if !dir.join("index.html").is_file() {
        anyhow::bail!("the frontend build made no index.html in {}", dir.display());
    }
    Ok(dir)
}

/// Build the backend image (one image for all languages). Tagged both as
/// korp-backend:<tag> and as <account registry>/korp-backend:<tag>.
fn build_back(config: &Config) -> anyhow::Result<ImageName> {
    tracing::info!("build container image 'korp-backend' from manage/containerfiles/Containerfile.backend");
    let image_name = ImageName::builder()
        .registry(format!("registry.cloudflare.com/{}", config.cf_account_id()?))
        .name("korp-backend".to_string())
        .tag(format!("{KORP_BACKEND_VERSION}-{}", &pinned_commit(config, "Containerfile.backend", "KORP_BACKEND_COMMIT")?[..7]))
        .build()
        .expect("name is set");
    docker_build(
        config,
        "Containerfile.backend",
        Output::Image(&[&image_name.to_name_and_tag_string(), &image_name.to_string()]),
        &[],
    )?;
    Ok(image_name)
}

/// Push the backend image to the Cloudflare container registry of the account,
/// with `wrangler containers push` (npx, in worker/).
fn push_back(config: &Config, image_name: &ImageName) -> anyhow::Result<()> {
    let image_name = image_name.to_name_and_tag_string();
    let dir = config.cwd.join(WRANGLER_DIR);
    tracing::info!(working_directory = %dir.display(), "run: 'npx wrangler containers push {image_name}'");
    let status = std::process::Command::new("npx")
        .args(["wrangler", "containers", "push", &image_name])
        .current_dir(dir)
        // also when the account id came from --cf-account-id or the file
        .env("CLOUDFLARE_ACCOUNT_ID", config.cf_account_id()?)
        .status()?;
    child_exit_ok(&status)?;
    Ok(())
}

/// Deploy the Worker korp-<lang>: build the frontend, build and push the
/// backend image (unless dry run), then the rest in deploy.rs.
fn deploy(config: &Config, lang: Lang, dry_run: bool) -> anyhow::Result<()> {
    let account_id = config.cf_account_id()?;
    build_front(config, lang)?;
    let repository = format!("registry.cloudflare.com/{account_id}/korp-backend");
    let image = if dry_run {
        format!("{repository}@{}", deploy::PLACEHOLDER_IMAGE_DIGEST)
    } else {
        let image_name = build_back(config)?;
        push_back(config, &image_name)?;
        deploy::pushed_image_digest(&image_name.to_string(), &repository)?
    };
    tracing::info!(image, "backend image");
    deploy::deploy(&deploy::Options {
        manage_dir: config.cwd,
        account_id: &account_id,
        lang: lang.as_str(),
        image: &image,
        dry_run,
    })
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
        Task::BuildFront(lang) => {
            let dir = build_front(&config, lang)?;
            println!("Built the frontend of {lang} into {}", dir.display());
        }
        Task::PushBack => {
            let image_name = build_back(&config)?;
            push_back(&config, &image_name)?;
        }
        Task::Deploy { lang, dry_run } => deploy(&config, lang, dry_run)?,
        Task::Restart(lang) => {
            deploy::restart_lang(&cwd, &config.cf_account_id()?, &config.cf_api_token()?, lang.as_str()).await?
        }
        Task::Data(ref matches) => corpora::run(&config.cf_account_id()?, &config.cf_api_token()?, matches).await?,
    }

    Ok(())
}
