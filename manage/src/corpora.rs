//! Corpus data in R2: the `data` command (see LAYOUT for what goes where).
//!
//! Talks to R2 through its S3 API (multipart uploads need it), with S3
//! credentials derived from the Cloudflare API token: access key id = the
//! token's id, secret = sha256 of the token, as documented for R2. So no
//! separate R2 keys are needed, but the token needs R2 write access.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};
use rusty_s3::actions::{CreateMultipartUpload, ListObjectsV2};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use sha2::{Digest, Sha256};

use crate::cf_models::ApiResponse;

/// The R2 bucket with the corpora, the same name in every account.
pub const BUCKET: &str = "korp-corpora";

pub const LAYOUT: &str = "\
Layout of the R2 bucket korp-corpora:

  corpora/<corpus>/<hash>.tar.zst    one CWB corpus: registry/<corpus> and
                                     data/<dir>/ (<dir> from the registry's HOME,
                                     normally <corpus>). Unpack into /corpora.
  configs/<lang>/<hash>.tar.zst      the Korp corpus config of one language
                                     (gtweb2_config/corpus_configs/<lang>/:
                                     attributes/, corpora/, modes/).
                                     Unpack into /corpus_config.
  langs/<lang>/<release>.json        a release of one language: which config
                                     and which corpus archives it uses, e.g.
                                     {\"lang\": \"fkv\", \"release\": \"20261119T101500Z\",
                                      \"created\": \"2026-11-19T10:15:00Z\",
                                      \"config\": {\"key\": \"configs/fkv/<hash>.tar.zst\", \"size\": 1234},
                                      \"corpora\": {\"fkv_news_20210319\":
                                          {\"key\": \"corpora/fkv_news_20210319/<hash>.tar.zst\", \"size\": 5678}, ...}}
  langs/<lang>/current               the release in use (text, e.g. 20261119T101500Z)

<hash> is the first 16 hex digits of the sha256 of the (uncompressed,
deterministic) tar archive, so archives never change once uploaded, and
uploading unchanged data does nothing. <release> is the UTC time it was made;
releases sort by name. The corpora of a language are the ones its corpus
config lists (corpora/<corpus>.yaml); corpus names don't always start with
the language code.

The backend reads langs/<lang>/current, then that release, then fetches the
archives it names.";

/// How long the presigned S3 urls are valid. They're used right away.
const SIGN_FOR: Duration = Duration::from_secs(3600);
/// Files up to this size are uploaded in one request, bigger ones in parts.
/// (A single PUT is limited to ~5 GB by R2, wrangler stops at ~315 MB.)
const SINGLE_PUT_MAX: u64 = 100 * 1024 * 1024;
/// Size of each part of a multipart upload (R2: all parts but the last must be
/// the same size, at least 5 MiB).
const PART_SIZE: usize = 64 * 1024 * 1024;
/// zstd level of the archives. CWB data is already compressed, so this gains
/// little over the default (sme_news: 343 MB raw, 308 MB at -3, 286 MB at -19),
/// but it's only paid when a corpus changes. Unpacking is fast either way.
const ZSTD_LEVEL: i32 = 19;

// ---------------------------------------------------------------- the command

pub fn command() -> Command {
    Command::new("data")
        .about("Corpus data in R2: upload corpora and corpus configs, list, roll back")
        .after_long_help(LAYOUT)
        .subcommand_required(true)
        .subcommand(
            Command::new("upload")
                .about("Upload a language's corpus config and corpora, and make them its current release")
                .long_about(
                    "Upload a language's corpus config and the corpora it lists, and make \
                     them the language's current release (see `data --help` for the layout).\n\n\
                     Archives already in R2 aren't uploaded again. If nothing changed, no new \
                     release is made. If the result equals a release that is newer than the \
                     current one (the language was rolled back from it), the rollback is kept. \
                     A corpus listed in the config but not found locally is taken from the \
                     current release, if it's there.",
                )
                .arg(Arg::new("lang").required(true).help("e.g. fkv"))
                .arg(
                    Arg::new("cwb-dir")
                        .long("cwb-dir")
                        .action(ArgAction::Append)
                        .default_value("../cwb-data")
                        .help("Directory with CWB registry/ and data/ to take corpora from (repeatable, searched in order)"),
                )
                .arg(
                    Arg::new("config-dir")
                        .long("config-dir")
                        .default_value("../gtweb2_config/corpus_configs")
                        .help("Directory with the corpus configs, one subdirectory per language"),
                )
                .arg(
                    Arg::new("skip-missing")
                        .long("skip-missing")
                        .action(ArgAction::SetTrue)
                        .help("Leave out corpora that the config lists but that are neither found locally nor in R2"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .short('n')
                        .action(ArgAction::SetTrue)
                        .help("Only read from R2, and say what would be uploaded"),
                ),
        )
        .subcommand(
            Command::new("list")
                .about("List the languages in R2, or one language's releases and current contents")
                .arg(Arg::new("lang")),
        )
        .subcommand(
            Command::new("rollback")
                .about("Make an earlier release of a language current (default: the one before the current one)")
                .arg(Arg::new("lang").required(true))
                .arg(Arg::new("release").help("e.g. 20261119T101500Z (`data list <lang>` lists them)")),
        )
        .subcommand(Command::new("create-bucket").about(format!("Create the R2 bucket {BUCKET}")))
}

pub async fn run(account_id: &str, api_token: &str, matches: &ArgMatches) -> anyhow::Result<()> {
    if let Some(("create-bucket", _)) = matches.subcommand() {
        return create_bucket(account_id, api_token).await;
    }
    let r2 = R2::connect(account_id, api_token).await?;
    match matches.subcommand() {
        Some(("upload", m)) => {
            let opts = UploadOptions {
                lang: m.get_one::<String>("lang").expect("required").clone(),
                cwb_dirs: m.get_many::<String>("cwb-dir").expect("default").map(PathBuf::from).collect(),
                config_dir: m.get_one::<String>("config-dir").expect("default").into(),
                skip_missing: m.get_flag("skip-missing"),
                dry_run: m.get_flag("dry-run"),
            };
            upload(&r2, &opts).await
        }
        Some(("list", m)) => match m.get_one::<String>("lang") {
            Some(lang) => list_lang(&r2, lang).await,
            None => list_langs(&r2).await,
        },
        Some(("rollback", m)) => {
            let lang = m.get_one::<String>("lang").expect("required");
            rollback(&r2, lang, m.get_one::<String>("release").map(String::as_str)).await
        }
        _ => unreachable!("clap: data requires a subcommand"),
    }
}

// ------------------------------------------------------------------- releases

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Object {
    key: String,
    size: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Release {
    lang: String,
    release: String,
    created: String,
    config: Object,
    corpora: BTreeMap<String, Object>,
}

impl Release {
    fn same_content(&self, other: &Release) -> bool {
        self.config.key == other.config.key
            && self.corpora.len() == other.corpora.len()
            && self.corpora.iter().zip(&other.corpora).all(|((n1, o1), (n2, o2))| n1 == n2 && o1.key == o2.key)
    }

    fn total_size(&self) -> u64 {
        self.config.size + self.corpora.values().map(|o| o.size).sum::<u64>()
    }
}

fn release_key(lang: &str, release: &str) -> String {
    format!("langs/{lang}/{release}.json")
}

fn current_key(lang: &str) -> String {
    format!("langs/{lang}/current")
}

/// The release ids of a language, oldest first.
async fn releases(r2: &R2, lang: &str) -> anyhow::Result<Vec<String>> {
    let prefix = format!("langs/{lang}/");
    let mut ids: Vec<String> = r2
        .list(&prefix)
        .await?
        .into_iter()
        .filter_map(|o| o.key.strip_prefix(&prefix)?.strip_suffix(".json").map(str::to_string))
        .collect();
    ids.sort();
    Ok(ids)
}

async fn current_release_id(r2: &R2, lang: &str) -> anyhow::Result<Option<String>> {
    Ok(r2
        .get(&current_key(lang))
        .await?
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string()))
}

async fn load_release(r2: &R2, lang: &str, id: &str) -> anyhow::Result<Release> {
    let key = release_key(lang, id);
    let bytes = r2.get(&key).await?.with_context(|| format!("{key} not found in R2"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("{key}: not a valid release"))
}

async fn set_current(r2: &R2, lang: &str, id: &str) -> anyhow::Result<()> {
    r2.put(&current_key(lang), id.as_bytes().to_vec(), "text/plain").await
}

// --------------------------------------------------------------------- upload

struct UploadOptions {
    lang: String,
    cwb_dirs: Vec<PathBuf>,
    config_dir: PathBuf,
    skip_missing: bool,
    dry_run: bool,
}

async fn upload(r2: &R2, opts: &UploadOptions) -> anyhow::Result<()> {
    let lang = opts.lang.as_str();
    let dry_run = opts.dry_run;
    if !r2.bucket_exists().await? {
        if dry_run {
            tracing::warn!("bucket {BUCKET} doesn't exist (yet): treating it as empty");
        } else {
            bail!("bucket {BUCKET} doesn't exist: create it with `data create-bucket`");
        }
    }

    let config_dir = opts.config_dir.join(lang);
    if !config_dir.is_dir() {
        bail!("no corpus config for {lang}: {} is not a directory", config_dir.display());
    }
    let corpus_names = corpora_in_config(&config_dir)?;
    tracing::info!(lang, corpora = corpus_names.len(), config = %config_dir.display(), "corpus config");

    let current_id = current_release_id(r2, lang).await?;
    let current = match &current_id {
        Some(id) => Some(load_release(r2, lang, id).await?),
        None => None,
    };

    // Find the corpora, and check that none is missing before uploading anything
    let mut local = Vec::new();
    let mut corpora = BTreeMap::new();
    let mut missing = Vec::new();
    for name in &corpus_names {
        match find_corpus(&opts.cwb_dirs, name)? {
            Some(entries) => local.push((name, entries)),
            None => match current.as_ref().and_then(|c| c.corpora.get(name)) {
                Some(object) => {
                    tracing::info!(corpus = name, key = object.key, "not found locally, keeping the one in the current release");
                    corpora.insert(name.clone(), object.clone());
                }
                None if opts.skip_missing => {
                    tracing::warn!(corpus = name, "not found locally nor in R2: leaving it out (--skip-missing)");
                }
                None => missing.push(name.as_str()),
            },
        }
    }
    if !missing.is_empty() {
        bail!(
            "the corpus config of {lang} lists corpora that are neither in {} nor in R2: {}\n\
             Give the directory they're in with --cwb-dir, fix the config, or leave them out with --skip-missing.",
            opts.cwb_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", "),
            missing.join(", ")
        );
    }

    // Upload the config and the local corpora
    let mut entries = Vec::new();
    add_tree(&mut entries, "", &config_dir)?;
    // In a dry run, what would be uploaded: count and size before compression
    // (the compressed size is only known after compressing, which a dry run skips)
    let mut pending = (0usize, 0u64);
    let (config, tar_size) =
        ensure_uploaded(r2, &format!("configs/{lang}"), &format!("config of {lang}"), &entries, dry_run).await?;
    if let Some(tar_size) = tar_size {
        pending = (pending.0 + 1, pending.1 + tar_size);
    }
    for (name, entries) in local {
        let (object, tar_size) = ensure_uploaded(r2, &format!("corpora/{name}"), name, &entries, dry_run).await?;
        if let Some(tar_size) = tar_size {
            pending = (pending.0 + 1, pending.1 + tar_size);
        }
        corpora.insert(name.clone(), object);
    }

    let now = chrono::Utc::now();
    let release = Release {
        lang: lang.to_string(),
        release: now.format("%Y%m%dT%H%M%SZ").to_string(),
        created: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        config,
        corpora,
    };

    // Same as an existing release?
    if let (Some(current), Some(current_id)) = (&current, &current_id) {
        if current.same_content(&release) {
            println!("{lang}: up to date (release {current_id})");
            return Ok(());
        }
        for id in releases(r2, lang).await? {
            if &id == current_id {
                continue;
            }
            if load_release(r2, lang, &id).await?.same_content(&release) {
                if &id > current_id {
                    println!(
                        "{lang}: this is release {id}, which was rolled back to {current_id}: keeping the rollback. \
                         To go forward again: `data rollback {lang} {id}`"
                    );
                } else if dry_run {
                    println!("{lang}: this is the earlier release {id}: would make it current again (dry run)");
                } else {
                    set_current(r2, lang, &id).await?;
                    println!("{lang}: this is the earlier release {id}: made it current again");
                }
                return Ok(());
            }
        }
    }

    if dry_run {
        let (count, tar_size) = pending;
        println!(
            "{lang}: would make a new release with the config and {} corpora (dry run). \
             {count} archive(s) to upload, {} before compression; {} already in R2.",
            release.corpora.len(),
            human(tar_size),
            release.corpora.len() + 1 - count,
        );
        return Ok(());
    }
    let json = serde_json::to_string_pretty(&release)?;
    let key = release_key(lang, &release.release);
    if r2.size(&key).await?.is_some() {
        bail!("{key} exists already (two uploads in the same second?): run it again");
    }
    r2.put(&key, json.into_bytes(), "application/json").await?;
    set_current(r2, lang, &release.release).await?;
    println!(
        "{lang}: release {} is current ({} corpora, {})",
        release.release,
        release.corpora.len(),
        human(release.total_size())
    );
    Ok(())
}

/// The corpora a corpus config lists: the names of its corpora/*.yaml files.
fn corpora_in_config(config_dir: &Path) -> anyhow::Result<Vec<String>> {
    let dir = config_dir.join("corpora");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let stem = path.file_stem().and_then(|s| s.to_str()).context("non-utf8 file name")?;
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Find a corpus in the CWB directories (each with registry/ and data/), and
/// return its archive entries: registry/<corpus> and data/<dir>/..., where
/// <dir> is the last component of the registry's HOME (normally <corpus>; the
/// rest of HOME doesn't matter, the backend's entrypoint rewrites it).
fn find_corpus(cwb_dirs: &[PathBuf], name: &str) -> anyhow::Result<Option<Vec<(String, PathBuf)>>> {
    for cwb_dir in cwb_dirs {
        let registry = cwb_dir.join("registry").join(name);
        if !registry.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&registry).with_context(|| format!("reading {}", registry.display()))?;
        let home = text
            .lines()
            .find_map(|line| line.strip_prefix("HOME").filter(|rest| rest.starts_with([' ', '\t'])))
            .map(|rest| rest.trim().trim_matches('"'))
            .with_context(|| format!("{}: no HOME line", registry.display()))?;
        let dir_name = Path::new(home)
            .file_name()
            .and_then(|s| s.to_str())
            .with_context(|| format!("{}: can't make sense of HOME {home}", registry.display()))?;
        let data_dir = cwb_dir.join("data").join(dir_name);
        if !data_dir.is_dir() {
            bail!("{}: its data directory {} doesn't exist", registry.display(), data_dir.display());
        }
        let mut entries = vec![(format!("registry/{name}"), registry)];
        add_tree(&mut entries, &format!("data/{dir_name}"), &data_dir)?;
        return Ok(Some(entries));
    }
    Ok(None)
}

/// Upload the archive of `entries` as `<prefix>/<hash>.tar.zst`, unless it's
/// there already. Returns its key and (compressed) size; in a dry run, for an
/// archive that isn't in R2, the size is unknown (0) and the second value is
/// its size before compression.
async fn ensure_uploaded(
    r2: &R2,
    prefix: &str,
    what: &str,
    entries: &[(String, PathBuf)],
    dry_run: bool,
) -> anyhow::Result<(Object, Option<u64>)> {
    let (hash, tar_size) = tar_hash(entries).with_context(|| format!("archiving {what}"))?;
    let key = format!("{prefix}/{hash}.tar.zst");
    if let Some(size) = r2.size(&key).await? {
        println!("{what}: already in R2: {key} ({})", human(size));
        return Ok((Object { key, size }, None));
    }
    if dry_run {
        println!("{what}: would upload {key} ({} before compression)", human(tar_size));
        return Ok((Object { key, size: 0 }, Some(tar_size)));
    }

    let tmp = std::env::temp_dir().join(format!("korp-{}-{hash}.tar.zst", prefix.replace('/', "-")));
    tracing::info!(file = %tmp.display(), "{what}: compressing {} (zstd -{ZSTD_LEVEL})", human(tar_size));
    let result: anyhow::Result<Object> = async {
        write_archive(entries, &tmp).with_context(|| format!("writing {}", tmp.display()))?;
        let size = std::fs::metadata(&tmp)?.len();
        tracing::info!(key, size = %human(size), "{what}: uploading");
        r2.put_file(&key, &tmp).await?;
        println!("{what}: uploaded {key} ({})", human(size));
        Ok(Object { key, size })
    }
    .await;
    let _ = std::fs::remove_file(&tmp);
    Ok((result?, None))
}

// ------------------------------------------------------------ list, rollback

async fn list_langs(r2: &R2) -> anyhow::Result<()> {
    let mut langs: Vec<String> = r2
        .list("langs/")
        .await?
        .into_iter()
        .filter_map(|o| Some(o.key.strip_prefix("langs/")?.split_once('/')?.0.to_string()))
        .collect();
    langs.dedup();
    if langs.is_empty() {
        println!("no languages in {BUCKET}");
    }
    for lang in langs {
        let ids = releases(r2, &lang).await?;
        match current_release_id(r2, &lang).await? {
            Some(id) => {
                let release = load_release(r2, &lang, &id).await?;
                println!(
                    "{lang:6} current {id}  {:2} corpora  {:>10}  ({} releases)",
                    release.corpora.len(),
                    human(release.total_size()),
                    ids.len()
                );
            }
            None => println!("{lang:6} no current release ({} releases)", ids.len()),
        }
    }
    Ok(())
}

async fn list_lang(r2: &R2, lang: &str) -> anyhow::Result<()> {
    let current_id = current_release_id(r2, lang).await?;
    let ids = releases(r2, lang).await?;
    if ids.is_empty() {
        println!("{lang}: no releases");
        return Ok(());
    }
    println!("releases of {lang}:");
    for id in &ids {
        let marker = if Some(id) == current_id.as_ref() { "*" } else { " " };
        println!(" {marker} {id}");
    }
    let Some(current_id) = current_id else {
        println!("no current release");
        return Ok(());
    };
    let release = load_release(r2, lang, &current_id).await?;
    println!("\ncurrent release {current_id} ({}):", human(release.total_size()));
    println!("  {:28} {:>10}  {}", "(config)", human(release.config.size), release.config.key);
    for (name, object) in &release.corpora {
        println!("  {name:28} {:>10}  {}", human(object.size), object.key);
    }
    Ok(())
}

async fn rollback(r2: &R2, lang: &str, release: Option<&str>) -> anyhow::Result<()> {
    let ids = releases(r2, lang).await?;
    let current_id = current_release_id(r2, lang).await?;
    let target = match release {
        Some(id) => {
            if !ids.iter().any(|i| i == id) {
                bail!("{lang} has no release {id} (`data list {lang}` lists them)");
            }
            id.to_string()
        }
        None => {
            let current_id = current_id.as_ref().with_context(|| format!("{lang} has no current release"))?;
            ids.iter()
                .filter(|id| *id < current_id)
                .next_back()
                .with_context(|| format!("{lang}: there's no release before {current_id}"))?
                .clone()
        }
    };
    if current_id.as_deref() == Some(target.as_str()) {
        println!("{lang}: {target} is current already");
        return Ok(());
    }
    load_release(r2, lang, &target).await?; // check that it's readable
    set_current(r2, lang, &target).await?;
    println!(
        "{lang}: current release is now {target} (was {}). Restart the backend to use it.",
        current_id.as_deref().unwrap_or("none")
    );
    Ok(())
}

async fn create_bucket(account_id: &str, api_token: &str) -> anyhow::Result<()> {
    let response: ApiResponse<serde_json::Value> = reqwest::Client::new()
        .post(format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/r2/buckets"))
        .bearer_auth(api_token.trim())
        .json(&serde_json::json!({ "name": BUCKET }))
        .send()
        .await?
        .json()
        .await?;
    response.try_into_result()?;
    println!("created bucket {BUCKET}");
    Ok(())
}

// ------------------------------------------------------------------- archives

/// Add `dir` and everything below it, sorted, as entries under `prefix` in
/// the archive ("" = at the top, without an entry for `dir` itself).
fn add_tree(entries: &mut Vec<(String, PathBuf)>, prefix: &str, dir: &Path) -> anyhow::Result<()> {
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(dir)?;
        let rel = rel.to_str().with_context(|| format!("non-utf8 path {}", entry.path().display()))?;
        let name = match (prefix, rel) {
            ("", "") => continue,
            ("", rel) => rel.to_string(),
            (prefix, "") => prefix.to_string(),
            (prefix, rel) => format!("{prefix}/{rel}"),
        };
        entries.push((name, entry.into_path()));
    }
    Ok(())
}

/// Write the entries as a tar archive that depends only on their names,
/// contents and permissions (no times, no owners), so equal data gives an
/// equal archive, wherever and whenever it's made.
fn write_tar<W: Write>(entries: &[(String, PathBuf)], writer: W) -> io::Result<W> {
    let mut builder = tar::Builder::new(writer);
    builder.mode(tar::HeaderMode::Deterministic);
    for (name, path) in entries {
        builder.append_path_with_name(path, name)?;
    }
    builder.into_inner()
}

struct HashWriter {
    hasher: Sha256,
    len: u64,
}

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        self.len += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The content hash (16 hex digits) and size of the tar archive of the entries.
fn tar_hash(entries: &[(String, PathBuf)]) -> io::Result<(String, u64)> {
    let w = write_tar(entries, HashWriter { hasher: Sha256::new(), len: 0 })?;
    let hash = hex::encode(w.hasher.finalize());
    Ok((hash[..16].to_string(), w.len))
}

fn write_archive(entries: &[(String, PathBuf)], path: &Path) -> io::Result<()> {
    let file = io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = zstd::stream::write::Encoder::new(file, ZSTD_LEVEL)?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    encoder.multithread(threads as u32)?;
    let encoder = write_tar(entries, encoder)?;
    encoder.finish()?.flush()
}

fn human(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::DECIMAL)
}

// ------------------------------------------------------------- R2 (S3 API)

struct R2 {
    http: reqwest::Client,
    bucket: Bucket,
    credentials: Credentials,
}

struct ListedObject {
    key: String,
}

impl R2 {
    async fn connect(account_id: &str, api_token: &str) -> anyhow::Result<Self> {
        let token = api_token.trim();
        let http = reqwest::Client::new();

        // For testing against a local S3 server (e.g. MinIO) instead of R2:
        // KORP_S3_ENDPOINT=http://127.0.0.1:9000, with AWS_ACCESS_KEY_ID and
        // AWS_SECRET_ACCESS_KEY.
        if let Ok(endpoint) = std::env::var("KORP_S3_ENDPOINT") {
            tracing::warn!(endpoint, "using KORP_S3_ENDPOINT instead of R2");
            let bucket = Bucket::new(endpoint.parse()?, UrlStyle::Path, BUCKET, "us-east-1")?;
            let credentials = Credentials::from_env().context("KORP_S3_ENDPOINT needs AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY")?;
            return Ok(Self { http, bucket, credentials });
        }

        #[derive(serde::Deserialize)]
        struct Verify {
            id: String,
        }
        // Account-owned tokens verify at /accounts/<id>/tokens/verify, user
        // tokens at /user/tokens/verify.
        let mut token_id = None;
        for url in [
            format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/tokens/verify"),
            "https://api.cloudflare.com/client/v4/user/tokens/verify".to_string(),
        ] {
            let response: ApiResponse<Verify> = http.get(&url).bearer_auth(token).send().await?.json().await?;
            if response.success {
                token_id = response.result.map(|v| v.id);
                break;
            }
        }
        let token_id = token_id.context("the Cloudflare API token doesn't verify (neither as an account nor a user token)")?;
        let secret = hex::encode(Sha256::digest(token.as_bytes()));

        let endpoint = format!("https://{account_id}.r2.cloudflarestorage.com").parse()?;
        let bucket = Bucket::new(endpoint, UrlStyle::Path, BUCKET, "auto")?;
        Ok(Self { http, bucket, credentials: Credentials::new(token_id, secret) })
    }

    async fn bucket_exists(&self) -> anyhow::Result<bool> {
        let url = self.bucket.head_bucket(Some(&self.credentials)).sign(SIGN_FOR);
        let response = self.http.head(url).send().await?;
        match response.status().as_u16() {
            404 => Ok(false),
            _ => checked(response, "HEAD bucket").await.map(|_| true),
        }
    }

    /// The object, or None if it doesn't exist.
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let url = self.bucket.get_object(Some(&self.credentials), key).sign(SIGN_FOR);
        let response = self.http.get(url).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = checked(response, &format!("GET {key}")).await?;
        Ok(Some(response.bytes().await?.to_vec()))
    }

    /// The size of the object, or None if it doesn't exist.
    async fn size(&self, key: &str) -> anyhow::Result<Option<u64>> {
        let url = self.bucket.head_object(Some(&self.credentials), key).sign(SIGN_FOR);
        let response = self.http.head(url).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = checked(response, &format!("HEAD {key}")).await?;
        let length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .with_context(|| format!("HEAD {key}: no content-length"))?;
        Ok(Some(length))
    }

    /// All objects whose key starts with `prefix`, sorted by key. An empty
    /// list if the bucket doesn't exist.
    async fn list(&self, prefix: &str) -> anyhow::Result<Vec<ListedObject>> {
        let mut objects = Vec::new();
        let mut continuation: Option<String> = None;
        loop {
            let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
            action.with_prefix(prefix);
            if let Some(token) = &continuation {
                action.with_continuation_token(token.clone());
            }
            let response = self.http.get(action.sign(SIGN_FOR)).send().await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(Vec::new());
            }
            let text = checked(response, &format!("list {prefix}")).await?.text().await?;
            let parsed = ListObjectsV2::parse_response(&text).with_context(|| format!("list {prefix}: bad response"))?;
            objects.extend(parsed.contents.into_iter().map(|c| ListedObject { key: c.key }));
            match parsed.next_continuation_token {
                Some(token) => continuation = Some(token),
                None => break,
            }
        }
        objects.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(objects)
    }

    async fn put(&self, key: &str, body: Vec<u8>, content_type: &str) -> anyhow::Result<()> {
        let url = self.bucket.put_object(Some(&self.credentials), key).sign(SIGN_FOR);
        let response = self
            .http
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await?;
        checked(response, &format!("PUT {key}")).await?;
        Ok(())
    }

    /// Upload a file: in one request if it's small, else as a multipart upload.
    async fn put_file(&self, key: &str, path: &Path) -> anyhow::Result<()> {
        let size = std::fs::metadata(path)?.len();
        if size <= SINGLE_PUT_MAX {
            return self.put(key, std::fs::read(path)?, "application/zstd").await;
        }

        let action = self.bucket.create_multipart_upload(Some(&self.credentials), key);
        let response = self.http.post(action.sign(SIGN_FOR)).send().await?;
        let text = checked(response, &format!("create multipart upload {key}")).await?.text().await?;
        let upload_id = CreateMultipartUpload::parse_response(&text)
            .with_context(|| format!("create multipart upload {key}: bad response"))?
            .upload_id()
            .to_string();

        let result = self.upload_parts(key, path, size, &upload_id).await;
        if result.is_err() {
            let url = self.bucket.abort_multipart_upload(Some(&self.credentials), key, &upload_id).sign(SIGN_FOR);
            if let Err(e) = self.http.delete(url).send().await {
                tracing::warn!(key, upload_id, "couldn't abort the multipart upload: {e}");
            }
        }
        result
    }

    async fn upload_parts(&self, key: &str, path: &Path, size: u64, upload_id: &str) -> anyhow::Result<()> {
        let parts = size.div_ceil(PART_SIZE as u64);
        let mut file = std::fs::File::open(path)?;
        let mut etags = Vec::new();
        for number in 1..=parts {
            let mut buf = Vec::with_capacity(PART_SIZE);
            (&mut file).take(PART_SIZE as u64).read_to_end(&mut buf)?;
            let part_number = u16::try_from(number).context("too many parts")?;
            tracing::info!(key, "uploading part {number}/{parts} ({})", human(buf.len() as u64));
            let url = self
                .bucket
                .upload_part(Some(&self.credentials), key, part_number, upload_id)
                .sign(SIGN_FOR);
            let response = self.http.put(url).body(buf).send().await?;
            let response = checked(response, &format!("PUT {key} part {number}")).await?;
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .with_context(|| format!("PUT {key} part {number}: no etag"))?
                .to_string();
            etags.push(etag);
        }

        let action = self.bucket.complete_multipart_upload(
            Some(&self.credentials),
            key,
            upload_id,
            etags.iter().map(String::as_str),
        );
        let url = action.sign(SIGN_FOR);
        let response = self.http.post(url).body(action.body()).send().await?;
        checked(response, &format!("complete multipart upload {key}")).await?;
        Ok(())
    }
}

/// The response if it's a success, else an error with the status and body.
async fn checked(response: reqwest::Response, what: &str) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let hint = if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
        "\n(the API token needs R2 access to this bucket: \"Workers R2 Storage: Edit\" for writing)"
    } else {
        ""
    };
    bail!("{what}: {status}: {}{hint}", body.chars().take(500).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_hash_is_deterministic_and_ignores_mtime() {
        let dir = std::env::temp_dir().join(format!("korp-tar-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x"), "hello").unwrap();
        std::fs::write(dir.join("a/y"), "world").unwrap();
        let mut entries = Vec::new();
        add_tree(&mut entries, "data/a", &dir.join("a")).unwrap();
        let names: Vec<_> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["data/a", "data/a/b", "data/a/b/x", "data/a/y"]);

        let (h1, _) = tar_hash(&entries).unwrap();
        let f = std::fs::File::options().write(true).open(dir.join("a/y")).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
        let (h2, _) = tar_hash(&entries).unwrap();
        assert_eq!(h1, h2);
        std::fs::write(dir.join("a/y"), "world!").unwrap();
        let (h3, _) = tar_hash(&entries).unwrap();
        assert_ne!(h1, h3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
