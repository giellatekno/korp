mod assets_upload;
mod base64;
mod cf_models;
mod cli;
pub mod childprocess;
mod const_str_trim;
mod instance_specifier;
mod manifest;
mod docker;
mod worker_script;
pub mod wranglerjsonc;

use std::path::Path;

use crate::assets_upload::deploy_to_cloudflare;
use crate::childprocess::{SubprocessError, child_exit_ok};
use crate::instance_specifier::*;
use crate::docker::{DOCKER, ImageName};

fn path_to_string<P: AsRef<Path>>(path: P) -> Option<String> {
    Some(path.as_ref().to_str()?.to_string())
}

/// Start from directory given by `cwd`. Look for a directory named `containerfiles`
/// inside `cwd`, and if found, return `cwd`. If not found, go to `cwd`'s parent,
/// and look again. If arriving at the root and still no such directory found,
/// return an error.
fn get_build_context_dir(cwd: &Path) -> std::io::Result<String> {
    assert!(cwd.is_dir());
    let _span = tracing::debug_span!("get_build_context_dir()");
    let mut dir = cwd;

    loop {
        for item in std::fs::read_dir(dir)? {
            let item = item?;
            if !item.file_type()?.is_dir() {
                continue;
            }
            if item.file_name() == "containerfiles" {
                tracing::debug!(?dir, "found context dir");
                return Ok(path_to_string(dir).expect("non utf-8"));
            }
        }
        let Some(parent) = dir.parent() else {
            return Err(std::io::Error::other("no containerfiles directory found"));
        };
        dir = parent;
    }
}

/// Build the image korp-frontend-base (korp-frontend with its dependencies installed)
async fn build_front_base<'a>(config: &Config<'a>) -> anyhow::Result<()> {
    tracing::info!("build container image 'korp-frontend-base'");
    let build_context_directory = get_build_context_dir(config.cwd)?;
    let mut child = std::process::Command::new(DOCKER)
        .arg("build")
        .arg("-t")
        .arg("korp-frontend-base")
        .arg("-f")
        .arg("containerfiles/Containerfile.frontend-base")
        .arg("--load")
        .arg(build_context_directory)
        .spawn()?;
    child_exit_ok(&child.wait()?)?;
    Ok(())
}

/// Build the image korp-frontend-LANG (the built frontend of one instance, served by nginx)
async fn build_front<'a>(config: &Config<'a>, instance: &Instance) -> Result<(), SubprocessError> {
    tracing::info!("build container image '{}'", instance.image_name());
    let build_context_directory = get_build_context_dir(config.cwd)?;
    let mut child = std::process::Command::new(DOCKER)
        .arg("build")
        .arg("-t")
        .arg(instance.image_name())
        .arg("--file")
        .arg(instance.containerfile_path_str())
        .arg("--build-arg")
        .arg(format!("instance={}", instance.lang.as_str()))
        .arg("--load")
        .arg(build_context_directory)
        .spawn()?;
    child_exit_ok(&child.wait()?)?;
    Ok(())
}

async fn cf_deploy_front<'a>(
    config: &Config<'a>,
    instance: Instance,
) -> anyhow::Result<()> {
    let script_name = instance.image_name();
    let static_assets_dir = config.cwd.join("html");

    build_front_base(config).await?;
    build_front(config, &instance).await?;

    // delete the static assets before copying the new one from the image, to avoid
    // potential stale files
    if static_assets_dir.exists() {
        if static_assets_dir.is_dir() {
            std::fs::remove_dir_all(&static_assets_dir)?;
        } else {
            anyhow::bail!(
                "{} exists, but is not a directory, don't know what to do, can't continue",
                static_assets_dir.display()
            );
        }
    }

    tracing::info!("copy html dir from image");
    let container_name = docker::run_detached(instance.image_name(), None).await?;
    docker::copy(&format!("{container_name}:/usr/share/nginx/html"), ".").await?;
    docker::stop(&container_name).await?;

    deploy_to_cloudflare(config.cf_account_id, config.cf_api_token, script_name, &static_assets_dir).await?;
    Ok(())
}

/// Directory (in tasks/) where wrangler is installed (node_modules), to run it from.
/// For now that's the korp-backend-fkv Worker, the only one there is.
/// TODO: wrangler setup shared by all instances, when the Workers are redone.
const WRANGLER_DIR: &str = "korp-backend-fkv";

/// Push the backend image to the Cloudflare container registry of the account,
/// with `wrangler containers push`.
async fn push_back<'a>(config: &Config<'a>, image_name: ImageName) -> anyhow::Result<()> {
    let image_name = image_name.to_name_and_tag_string();
    tracing::info!(working_directory = WRANGLER_DIR, image_name, "run: 'pnpm wrangler containers push {image_name}'");

    let status = std::process::Command::new("pnpm")
        .args(["wrangler", "containers", "push", &image_name])
        .current_dir(config.cwd.join(WRANGLER_DIR))
        // also when the account id was given with --cf-account-id, not the env var
        .env("CLOUDFLARE_ACCOUNT_ID", config.cf_account_id)
        .status()?;
    child_exit_ok(&status)?;
    Ok(())
}

async fn wrangler_deploy(wd: &Path, lang: &str) -> anyhow::Result<()> {
    let dir = format!("korp-backend-{lang}");
    tracing::info!(working_directory = dir, "run: 'pnpm wrangler deploy'");

    std::process::Command::new("pnpm")
        .args(["wrangler", "deploy"])
        .current_dir(wd)
        .status()?;

    Ok(())
}

async fn cf_deploy_back<'a>(
    config: &Config<'a>,
    instance: Instance,
) -> anyhow::Result<()> {
    //let cwb_version = build_cwb_containerimage(config.cwd).await?;
    //let lang = instance.lang.as_str();
    //let wd = config.cwd.join(format!("korp-backend-{lang}"));
    //let image_name = build_korp_backend(config, &cwb_version).await?;
    //tracing::debug!("image_name: {image_name}, tag={}", image_name.tag);
    //let wrangler_jsonc_path = wd.join("wrangler.jsonc");
    //let mut doc = crate::wranglerjsonc::Document::from_path(&wrangler_jsonc_path)?;
    //let image = doc.get_image()?;
    //let new_image = image.with_tag(&image_name.tag)?;

    //if image == new_image {
    //    tracing::info!("no image change");
    //} else {
    //    tracing::info!(?new_image, ?image, "image changed");
    //}

    //doc.set_image(new_image.clone())?;
    //doc.save(&wrangler_jsonc_path)?;
    //push_korp_backend_to_cf_containerregistry(config.cwd, lang, new_image).await?;
    let lang = instance.lang.as_str();
    let wd = config.cwd.join(format!("korp-backend-{lang}"));
    wrangler_deploy(&wd, lang).await?;
    Ok(())
}

/// Tag of the korp-backend image. The Containerfile clones korp-backend's dev
/// branch (version 9.0.0.dev0), so this doesn't pin anything.
/// TODO: derive it from the built image (korp-backend version + commit, CWB version).
const KORP_BACKEND_TAG: &str = "9.0.0-dev";

async fn build_back<'a>(
    config: &Config<'a>,
) -> Result<ImageName, SubprocessError> {
    tracing::info!("build container image 'korp-backend' from containerfiles/Containerfile.backend");
    let build_context_directory = get_build_context_dir(config.cwd)?;
    let image_name = ImageName::builder()
        .registry(format!("registry.cloudflare.com/{}", config.cf_account_id))
        .name("korp-backend".to_string())
        .tag(KORP_BACKEND_TAG.to_string())
        .build()
        .unwrap();
    let mut child = std::process::Command::new(DOCKER)
        .arg("build")
        .arg("-t")
        .arg(format!("korp-backend:{KORP_BACKEND_TAG}"))
        .arg("-t")
        .arg(&image_name.to_string())
        .arg("-f")
        .arg("containerfiles/Containerfile.backend")
        .arg("--load")
        .arg(build_context_directory)
        .spawn()?;
    let exit_status = child.wait()?;
    child_exit_ok(&exit_status)?;

    // pn wrangler containers push registry.cloudflare.com/<account id>/korp-backend:<tag>
    //
    // update wrangler.jsonc
    // run pn wrangler deploy

    Ok(image_name)
}
async fn cf_deploy<'a>(
    config: &Config<'a>,
    instance: Instance,
) -> anyhow::Result<()> {
    if instance.is_backend() {
        cf_deploy_back(config, instance).await
    } else {
        cf_deploy_front(config, instance).await
    }
}

fn setup_tracing() {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}

struct Config<'a> {
    cwd: &'a Path,
    cf_account_id: &'a str,
    cf_api_token: &'a str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_tracing();
    let cwd = std::env::current_dir()?;
    let matches = cli::parse_args();

    let cf_account_id = get_cf_account_id(&matches)?;
    let cf_api_token = get_cf_api_token(&matches)?;

    let config = Config {
        cwd: &cwd,
        cf_account_id: &cf_account_id,
        cf_api_token: &cf_api_token,
    };

    if let Some(matches) = matches.subcommand_matches("cf") {
        if let Some(matches) = matches.subcommand_matches("deploy") {
            let inst = *matches.get_one("instance").expect("required");
            return cf_deploy(&config, inst).await;
        } else if let Some(matches) = matches.subcommand_matches("build") {
            match matches.subcommand() {
                Some(("back", _)) => {
                    build_back(&config).await?;
                }
                Some(("front", matches)) => {
                    let lang: Lang = *matches.get_one("lang").expect("required");
                    let instance = Instance { side: Side::Front, lang };
                    build_front_base(&config).await?;
                    build_front(&config, &instance).await?;
                }
                _ => unreachable!("clap: build requires a subcommand"),
            }
        } else if let Some(matches) = matches.subcommand_matches("push-image") {
            match matches.subcommand() {
                Some(("back", _)) => {
                    let image_name = build_back(&config).await?;
                    push_back(&config, image_name).await?;
                }
                _ => unreachable!("clap: push-image requires a subcommand"),
            }
        } else if let Some(matches) = matches.subcommand_matches("sync-settings") {
            println!("not automated (for now at least), explanation:\n");
            println!("in R2, the bucket name is 'korp-data', it contains three directories:");
            println!("corpus_configs/<lang>/ - korp backend configuration files for lang <lang>");
            println!("korp_backend_config/config.py - the korp backend config.py (same for all langs)");
            println!("cwb-data/registry/ - cwb registry files (/home/services/korp/cwb_files/registry on gtweb)");
            println!("cwb-data/data/ - the cwb data files (/home/services/korp/cwb_files/data on gtweb)");
            println!("");
            println!("To sync the corpus_configs/ directory, run:\n");
            println!("rclone sync corpus_configs/ cf-r2-gt-priv-korp-data:korp-data/corpus_configs/");
            println!("");
            println!("To sync the config.py file, run:");
            println!("  rclone copyto config.py cf-r2-gt-priv-korp-data:korp-data/korp_backend_config/config.py");
            println!("");
            println!("to sync the cwb-data files:");
            println!("(in the root korp directory)");
            println!("rclone sync cwb-data/ cf-r2-gt-priv-korp-data:korp-data/cwb-data");
        }
    }

    Ok(())
}

fn get_cf_api_token(matches: &clap::ArgMatches) -> anyhow::Result<String> {
    let path: &String = matches
        .get_one("cf-api-token-file")
        .expect("has default value, so is always present");
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(s),
        Err(e) => anyhow::bail!("critical: can't read cloudflare api token file '{path}': {e}"),
    }
}

fn get_cf_account_id(matches: &clap::ArgMatches) -> anyhow::Result<String> {
    match matches.get_one::<String>("cf-account-id") {
        Some(account_id) => Ok(account_id.clone()),
        None => anyhow::bail!(
            "can't find cloudflare account id. either set env var CLOUDFLARE_ACCOUNT_ID, or give --cf-account-id"
        ),
    }
}
