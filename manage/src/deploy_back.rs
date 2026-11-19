//! Deploy the Korp Worker of one language (`korp-<lang>`): korp-backend in a
//! container behind `/api/*`, see `worker/src/index.ts`.
//!
//! One Worker per language, all from the template `worker/wrangler.jsonc` and
//! the same code (`worker/src/index.ts`). For a language, this:
//!
//! 1. checks that R2 has a current release of the language (`manage data
//!    upload <lang>` makes one; the Worker reads it when the container starts),
//! 2. type-checks the Worker (`npm run check` in `worker/`),
//! 3. writes `worker/.build/<lang>/wrangler.json`: the template with the
//!    Worker name, `KORP_LANG` and the image pinned by digest,
//! 4. runs `wrangler deploy` with it, with the secrets in
//!    `worker/secrets.<account id>.env` (created on the first deploy: a random
//!    `ADMIN_TOKEN`),
//! 5. restarts the container (`POST /admin/restart`): Durable-Object containers
//!    don't restart on deploy, and a new container fetches the current release.
//!
//! Building and pushing the image is the caller's job; this gets the pushed
//! image as `registry.cloudflare.com/<account>/korp-backend@sha256:…`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::childprocess::child_exit_ok;

use crate::corpora::BUCKET;

/// A valid but nonexistent image, for `--dry-run` (wrangler accepts it there).
pub const PLACEHOLDER_IMAGE_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

pub struct Options<'a> {
    /// The manage/ directory (has worker/; its parent has gtweb2_config/)
    pub manage_dir: &'a Path,
    pub account_id: &'a str,
    pub lang: &'a str,
    /// registry.cloudflare.com/<account>/korp-backend@sha256:…
    pub image: &'a str,
    /// Only `wrangler deploy --dry-run`; missing R2 objects are warnings
    pub dry_run: bool,
}

pub fn worker_name(lang: &str) -> String {
    format!("korp-{lang}")
}

pub fn deploy(opts: &Options) -> anyhow::Result<()> {
    let worker_dir = opts.manage_dir.join("worker");
    ensure_npm_install(&worker_dir)?;
    check_r2(opts, &worker_dir)?;

    tracing::info!("type-check the Worker (npm run check)");
    run(Command::new("npm").args(["run", "--silent", "check"]).current_dir(&worker_dir).stdout(Stdio::null()))?;

    let config_path = write_wrangler_json(opts, &worker_dir)?;
    let secrets_path = ensure_secrets_file(&worker_dir, opts.account_id)?;

    let config = config_path.to_str().context("non-UTF-8 path")?;
    let secrets = secrets_path.to_str().context("non-UTF-8 path")?;
    let mut args = vec!["wrangler", "deploy", "--config", config, "--secrets-file", secrets];
    if opts.dry_run {
        args.push("--dry-run");
    }
    tracing::info!("npx {}", args.join(" "));
    let output = run_tee(wrangler(&worker_dir, opts.account_id).args(&args))?;

    if opts.dry_run {
        println!("\nDry run: nothing deployed. Config: {}", config_path.display());
        return Ok(());
    }

    // wrangler prints the Worker's URL(s), e.g. https://korp-fkv.<subdomain>.workers.dev
    let url = output
        .split_whitespace()
        .find(|word| word.starts_with("https://") && word.ends_with(".workers.dev"))
        .map(str::to_string);
    match url {
        Some(url) => {
            restart(&url, &secrets_path)?;
            println!("\nDeployed {} to {url}\nTry: curl {url}/api/info", worker_name(opts.lang));
        }
        None => println!(
            "\nDeployed, but found no workers.dev URL in wrangler's output: restart the container yourself \
             (POST <url>/admin/restart with the ADMIN_TOKEN from {})",
            secrets_path.display()
        ),
    }
    Ok(())
}

/// `npx wrangler` in worker/, for the given account.
fn wrangler(worker_dir: &Path, account_id: &str) -> Command {
    let mut cmd = Command::new("npx");
    cmd.current_dir(worker_dir).env("CLOUDFLARE_ACCOUNT_ID", account_id);
    cmd
}

fn run(cmd: &mut Command) -> anyhow::Result<()> {
    let status = cmd.status().with_context(|| format!("can't run {cmd:?}"))?;
    child_exit_ok(&status).with_context(|| format!("{cmd:?} failed"))?;
    Ok(())
}

/// Runs the command, printing its stdout as it goes, and returns it.
fn run_tee(cmd: &mut Command) -> anyhow::Result<String> {
    let mut child = cmd.stdout(Stdio::piped()).spawn().with_context(|| format!("can't run {cmd:?}"))?;
    let mut output = String::new();
    for line in BufReader::new(child.stdout.take().expect("piped")).lines() {
        let line = line?;
        println!("{line}");
        output.push_str(&line);
        output.push('\n');
    }
    child_exit_ok(&child.wait()?).with_context(|| format!("{cmd:?} failed"))?;
    Ok(output)
}

fn ensure_npm_install(worker_dir: &Path) -> anyhow::Result<()> {
    if !worker_dir.join("node_modules").is_dir() {
        tracing::info!("npm ci in {}", worker_dir.display());
        run(Command::new("npm").arg("ci").current_dir(worker_dir))?;
    }
    Ok(())
}

/// `wrangler r2 object get <bucket>/<key> --remote --pipe`: the object's
/// contents, or None if it can't be read.
fn r2_get(opts: &Options, worker_dir: &Path, key: &str) -> anyhow::Result<Option<String>> {
    let output = wrangler(worker_dir, opts.account_id)
        .args(["wrangler", "r2", "object", "get", &format!("{BUCKET}/{key}"), "--remote", "--pipe"])
        .stderr(Stdio::null())
        .output()?;
    Ok(output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// Checks that R2 has a current release of the language (the Worker reads it
/// when it starts the container). Missing is an error, or a warning with --dry-run.
fn check_r2(opts: &Options, worker_dir: &Path) -> anyhow::Result<()> {
    let lang = opts.lang;
    let problem = match r2_get(opts, worker_dir, &format!("langs/{lang}/current"))? {
        Some(id) if !id.trim().is_empty() => {
            let id = id.trim();
            match r2_get(opts, worker_dir, &format!("langs/{lang}/{id}.json"))? {
                Some(json) => {
                    let release: Value = serde_json::from_str(&json)
                        .with_context(|| format!("langs/{lang}/{id}.json isn't JSON"))?;
                    let corpora: Vec<&str> = release["corpora"]
                        .as_object()
                        .map(|c| c.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    tracing::info!("R2: {lang} is at release {id}, {} corpora: {}", corpora.len(), corpora.join(" "));
                    return Ok(());
                }
                None => format!("langs/{lang}/current names {id}, but langs/{lang}/{id}.json isn't in R2"),
            }
        }
        _ => format!("no release of {lang} in R2 bucket {BUCKET} (run `manage data upload {lang}`)"),
    };
    if opts.dry_run {
        eprintln!("WARNING: {problem}");
        Ok(())
    } else {
        bail!("{problem}")
    }
}

/// Writes worker/.build/<lang>/wrangler.json from the template, and returns its path.
fn write_wrangler_json(opts: &Options, worker_dir: &Path) -> anyhow::Result<PathBuf> {
    let template_path = worker_dir.join("wrangler.jsonc");
    let text = std::fs::read_to_string(&template_path)?;
    let mut config: Value = jsonc_parser::parse_to_serde_value(&text, &Default::default())
        .with_context(|| format!("can't parse {}", template_path.display()))?;

    let obj = config.as_object_mut().context("wrangler.jsonc: not an object")?;
    // Relative paths are relative to the config file, two levels down
    obj.remove("$schema");
    obj.insert("main".into(), json!("../../src/index.ts"));
    obj.insert("name".into(), json!(worker_name(opts.lang)));
    let images = config
        .pointer_mut("/containers/0/images")
        .context("wrangler.jsonc: no containers[0].images")?;
    *images = json!({ "korp": { "image": opts.image } });
    let vars = config.pointer_mut("/vars").and_then(Value::as_object_mut).context("wrangler.jsonc: no vars")?;
    vars.insert("KORP_LANG".into(), json!(opts.lang));

    let dir = worker_dir.join(".build").join(opts.lang);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("wrangler.json");
    std::fs::write(&path, serde_json::to_string_pretty(&config)? + "\n")?;
    tracing::info!("wrote {}", path.display());
    Ok(path)
}

/// worker/secrets.<account id>.env, created with a random ADMIN_TOKEN if missing.
fn ensure_secrets_file(worker_dir: &Path, account_id: &str) -> anyhow::Result<PathBuf> {
    let path = worker_dir.join(format!("secrets.{account_id}.env"));
    if !path.exists() {
        let mut bytes = [0u8; 32];
        std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut bytes)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        writeln!(file, "# Secrets of the korp-<lang> Workers in account {account_id}, passed on every deploy")?;
        writeln!(file, "ADMIN_TOKEN={}", hex::encode(bytes))?;
        tracing::info!("created {} with a new ADMIN_TOKEN", path.display());
    }
    Ok(path)
}

fn admin_token(secrets_path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(secrets_path)?
        .lines()
        .find_map(|line| line.strip_prefix("ADMIN_TOKEN=").map(|t| t.trim().trim_matches('"').to_string()))
        .with_context(|| format!("no ADMIN_TOKEN in {}", secrets_path.display()))
}

/// POST <url>/admin/restart: stops the container; the next request starts a
/// new one with the deployed image and the current corpora.
pub fn restart(url: &str, secrets_path: &Path) -> anyhow::Result<()> {
    let token = admin_token(secrets_path)?;
    tracing::info!("POST {url}/admin/restart");
    let output = Command::new("curl")
        .args(["--fail-with-body", "--silent", "--show-error", "-X", "POST", "-H"])
        .arg(format!("Authorization: Bearer {token}"))
        .arg(format!("{url}/admin/restart"))
        .output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        bail!("restart failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

/// The registry digest reference of a pushed image:
/// registry.cloudflare.com/<account>/korp-backend@sha256:…, from `docker
/// image inspect` (docker records it on push).
pub fn pushed_image_digest(local_image: &str, repository: &str) -> anyhow::Result<String> {
    let output = Command::new(crate::docker::DOCKER)
        .args(["image", "inspect", "--format", "{{json .RepoDigests}}", local_image])
        .output()?;
    child_exit_ok(&output.status).context("docker image inspect failed")?;
    let digests: Vec<String> = serde_json::from_slice(&output.stdout)?;
    digests
        .into_iter()
        .find(|d| d.starts_with(&format!("{repository}@sha256:")))
        .with_context(|| format!("{local_image} has no digest for {repository}: was it pushed?"))
}
