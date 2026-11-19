# TODO: Korp on Cloudflare

Each task below is meant to be done in its own Claude Code session. Start a
session with: "Read TODO.md, section *Context*, and do task N". Each task lists
what to read first; don't read all of `claude-log` (55 KB), only the sections
named.

When a task is done, mark it done here, and write down anything the next tasks
need to know (decisions, names, URLs) under the task.

## Context (for every task)

- **Goal:** host Giellatekno's Korp (≈15 languages) on Cloudflare, replacing the
  old on-prem gtweb-02 setup (`README.md` describes the old setup). Same
  concepts as the Apertium deployment: `claude-log` sections 4 (architecture)
  and 5 (Cloudflare gotchas); the code is in `~/gut/giellatekno/apertium`
  (`deploy/cloudflare/src/index.ts`, `wrangler.jsonc`, `manage.py`).
- **Accounts:** test = "UiT / Anders Lorentsen" (`manage/CLOUDFLARE_ACCOUNT_ID`,
  has Workers Paid/Containers), prod = "Giellatekno" (no Workers Paid yet). Work
  on test.
- **Management tool:** the Rust crate in `manage/` (replaces `tasks.py`). Run it
  from `manage/`: `CLOUDFLARE_ACCOUNT_ID=$(cat CLOUDFLARE_ACCOUNT_ID) cargo run --release -- …`
- **Containers:** real Docker only, never podman (if `DOCKER_HOST` points to
  podman, `unset DOCKER_HOST`). Builds need `--load`.
- **Backend image:** `manage/containerfiles/Containerfile.backend`: one image
  for all languages. Upstream korp-backend `dev` (9.0.0.dev0, FastAPI, port
  8000) + CWB from `giellatekno.azurecr.io/corpusworkbench:3.5.1`. Expects CWB
  data in `/corpora/{registry,data}` (the entrypoint rewrites registry
  HOME/INFO paths) and the corpus config in `/corpus_config`. Tested locally
  with fkv (`cwb-data/`, `gtweb2_config/corpus_configs/fkv`).
- **Korp 9 renamed its API** (`/query` → `/concordance`, `corpus=` →
  `corpora=`, …; upstream CHANGELOG). Frontend and backend versions must match.
- **Config:** `gtweb2_config/`: corpus configs (`corpus_configs/<lang>/`),
  frontend configs (`front/config-<lang>.yaml`), translations.
- **Working style:** Anders runs anything that changes the Cloudflare accounts
  (deploys, deletes, pushes, credentials); Claude prepares, verifies read-only,
  dry-runs, and gives him the commands. Most of the repo is untracked: before
  deleting a file, say if it's the only copy of something.

---

## 1. Repo hygiene (small; do first) — DONE

Notes (2026-11-19):
- Root `.gitignore` added (credentials, build output, big data,
  `manage/korp-backend-fkv/`, `.claude/`). `manage/xxx`, `cloned/` and `x.py`
  were already gone.
- Deleted `cloudflare/` and root `containerfiles/`.
- main was fast-forwarded to d102bd1 (the tool as it was, committed by the
  task-2 session).
- Docker build context = repo root (the root `.dockerignore` lets only
  `gtweb2_config/` through; add `!logo_change/` in task 5). Came with task 2
  (b975c95); the stale `manage/gtweb2_config/` was then deleted.
- Left alone on purpose: the deleted submodules `korp-backend`/`korp-frontend`
  and the tracked `.deb` (unstaged deletions in `git status`), and the untracked
  `compile-cwb-mono/` and `claude-log`.

- Add a root `.gitignore`: credentials (`manage/.CLOUDFLARE_API_TOKEN`,
  `manage/s3_compat_credentials`, `manage/xxx`, `**/.dev.vars`, `secrets*.env`),
  build output (`target/`, `node_modules/`, `.wrangler/`, `manage/html/`,
  `**/dist/`), big data (`cwb-data/`, `compile-cwb-mono/compiled_mono/`).
- `manage/xxx` contains an API token in plain text: delete the file (Anders
  decides whether to rotate the token).
- Decide which untracked directories to commit (`manage/`, `compile-cwb-mono/`)
  and which old ones to delete (`cloned/`, `cloudflare/`, root `containerfiles/`,
  `manage/gtweb2_config/` (stale copy of `gtweb2_config/`), `x.py`, the `.deb`).
- Commit (staged rename `repath.py` → `repath_cwb_registry.py` is pending).

**Done when:** `git status` shows only intended files, and no secrets are
committable.

## 2. Management tool: clean up the command structure (small, independent) — DONE

Everything is on Cloudflare now, so the `cf` subcommand level goes away:
`manage build back`, `manage build front <lang>`, `manage push back`, `manage
deploy …`. Remove dead code (`cf_get_worker`, `get` in `assets_upload.rs`,
unused variables), the `sync-settings` command that only prints rclone hints
(task 3 replaces it), and the `Instance` type if the new structure doesn't need
it. Read the account id from `CLOUDFLARE_ACCOUNT_ID` (file) when the env var
isn't set.

Don't build new features here: the deploy commands get rewritten in tasks 4
and 5 once those processes are established.

**Done when:** the tool compiles without warnings and `--help` shows the new
structure; `build back` still builds the image.

**Done (2026-11-18).** For the next tasks:
- Commands: `build back`, `build front <lang>`, `push back` (build + `wrangler
  containers push`), `deploy back <lang>` (`wrangler deploy` in
  `korp-backend-<lang>/`), `deploy front <lang>`. `Lang` is in `src/lang.rs`;
  `Instance`/`Side` are gone.
- Account id: `--cf-account-id` / `CLOUDFLARE_ACCOUNT_ID`, else the file
  `CLOUDFLARE_ACCOUNT_ID` in the current directory. The API token file is
  only read by `deploy front`.
- Docker build context is the repo root (the first ancestor with
  `gtweb2_config/`), limited by the root `.dockerignore`; Containerfiles are
  passed as `<root>/manage/containerfiles/<name>`. Task 5 must add
  `!logo_change/` to `.dockerignore`.
- Fixed: `deploy front` passed the API token and script name swapped to
  `create_version`/`create_deployment` (untested against the API since).
- `wrangler` is still run from `korp-backend-fkv/` (`WRANGLER_DIR` in
  `main.rs`), which task 4 replaces.

## 3. Corpus data in R2 (built and tested locally; the first real upload is Anders's)

**State (2026-11-19):** the `data` command in `manage/src/corpora.rs` is built.
It's tested against a local S3 server (VersityGW in Docker), not against R2:
fkv upload, re-upload (no change), rollback, multipart (sme_news), and
round-trip into the Korp 9 backend image (`/info`, `/concordance` OK). Still
to do, by Anders: create the bucket and upload fkv (commands below).

**Layout** (bucket `korp-corpora`, also in `manage data --help`):

```
corpora/<corpus>/<hash>.tar.zst   registry/<corpus> + data/<dir>/  → unpack into /corpora
configs/<lang>/<hash>.tar.zst     corpus_configs/<lang>/ contents → unpack into /corpus_config
langs/<lang>/<release>.json       {"lang", "release", "created",
                                   "config": {"key", "size"},
                                   "corpora": {"<corpus>": {"key", "size"}, …}}
langs/<lang>/current              text: the release id in use, e.g. 20261119T101500Z
```

Decisions:
- **Versioned per language, not per corpus.** A *release* pins the config
  archive and every corpus archive; `current` points to one. Rollback moves the
  pointer (`data rollback <lang> [release]`). Apertium's rule is kept: if an
  upload equals a release newer than `current` (rolled back from), the rollback
  stays, with a warning.
- **Archives are content-addressed**: `<hash>` = first 16 hex of sha256 of a
  deterministic tar (sorted, no times or owners). Unchanged data isn't uploaded
  again; unchanged everything makes no new release. Archives never change.
- **The language's corpora come from its corpus config** (`corpora/*.yaml`),
  not from name prefixes: sme's config also has `friis_*`, `leem_*`,
  `itkonen_*`. A corpus the config lists but that isn't found locally is
  taken from the current release; if it isn't there either: error, or left
  out with `--skip-missing`.
- **Corpus config in R2** (per language, versioned with the corpora), so a
  config change needs only a restart, not a deploy.
- **Upload via R2's S3 API** (multipart above 100 MB, 64 MB parts), with S3
  credentials derived from the API token (key id = token id, secret =
  sha256(token)). No R2 keys to manage, but the token needs R2 write access
  (it can list buckets; writing is untested). Test hook: `KORP_S3_ENDPOINT` +
  `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` use another S3 server.
- **Sizes:** CWB data is already compressed: fkv 6.7 MB in total; sme_news
  343 MB raw → 282 MB (zstd -19; -3 would be 308 MB), unpacks in <1 s. So the
  download dominates cold start (task 4: measure from R2).

Task 4 uses this: see its state below. Note: sme's config names the
`*_20211118` corpora, while `compile-cwb-mono` made `*_20261026` (task 6).

Commands for Anders (from `manage/`, with `CLOUDFLARE_ACCOUNT_ID` set):

```
cargo run --release -- data create-bucket
cargo run --release -- data upload fkv --dry-run
cargo run --release -- data upload fkv        # --cwb-dir defaults to ../cwb-data
cargo run --release -- data list fkv
```

The old bucket `korp-data` can go once task 4 no longer uses it.

### Original task description

How CWB data and corpus configs get into R2, in a form the backend container
can fetch at startup. Read first: `manage/r2-data-notes.md` (takeaways from the
old s3fs approach), Apertium's pair versioning (`claude-log` section 4, "Pair
versioning rule").

- Format: probably one archive per corpus (`<corpus>.tar.zst`: registry file +
  data dir), versioned with a `current` pointer like Apertium's pairs.
- Where the corpus config goes (in R2 per language, or baked into something).
- Upload: a management-tool command. `wrangler r2 object put` stops at ~315 MB;
  sme's corpora are ~719 MB in total, so check per-archive sizes, and use
  multipart upload if needed.
- Sources: `cwb-data/` (fkv, old), `compile-cwb-mono/compiled_mono/` (sme,
  new). The old bucket `korp-data` has the Korp-8 layout.

**Done when:** fkv's corpora are in R2 in the new layout, uploaded by the tool,
and the layout is documented (here and in the tool's help).

## 4. API Worker (backend on Cloudflare) (built and tested locally; the first real deploy is Anders's)

**State (2026-11-19):** merged into main with task 3. One Worker per language
(`korp-<lang>`), all from the same code and template in `manage/worker/`
(`README.md` there); `manage deploy back <lang> [--dry-run]` builds, pushes,
pins the image by digest, writes `worker/.build/<lang>/wrangler.json`,
deploys and restarts. The Worker uses task 3's layout: at container start the
Durable Object reads `langs/<lang>/current` and that release, passes
`DATA_URL=http://data.internal/<release>` + `CORPORA`, and `DataHandler` serves
only archives named in a release of this language. Tested locally (wrangler
dev + local R2 filled by `data upload` via a local S3): `/api/info`,
`/api/concordance`, restart; cold start 1.9 s for fkv. Not yet on Cloudflare,
not measured there.

Cleanup done (2026-11-19): the old `korp-backend-fkv` Worker, its container
application and the bucket `korp-data` are deleted on the test account (all its
contents had local copies), and so is `manage/korp-backend-fkv/`. `manage
restart <lang>` restarts a language's container. Left: the R2 S3 keys the old
Worker used (revoke in the dashboard) and the Korp 8 images `korp-backend:8.3.0-*`
in the registry.

### Original task description


Replace `manage/korp-backend-fkv/` (made from the containers template: deprecated
`Container` class, image by tag, R2 S3 keys passed into the container) with one
Worker template for all languages. Depends on task 3 for the data format (can
start in parallel with a stub).

- Like Apertium's `src/index.ts`: Durable Object with
  `scheduling_policy: "durable_object"`, `ctx.container.start()`,
  `enableInternet: false`, `interceptOutboundHttp("data.internal", …)` serving
  only this language's corpora from R2; image pushed by us and pinned by
  digest; startup wait loop; restart via authenticated `/admin/restart`.
- Container side: the entrypoint in `Containerfile.backend` fetches the corpus
  archives (+ corpus config) from `http://data.internal/…` into `/corpora`
  before starting gunicorn. Korp 9 settings come as env vars from the Worker
  (`CORS_ALLOW_ORIGINS`, `LC_COLLATE`: the image lacks `sv_SE.UTF-8`, …).
- **Decide:** one Worker per language (template + generated per-language
  wrangler config, like Apertium's `.build/<instance>/wrangler.json`), or one
  Worker for all languages with a Durable Object/container per language
  (`/<lang>/…` → `getByName(lang)`, language passed in `start({env})`). The
  second means one deploy and one container application; the first gives
  separate URLs, logs and stats per language. Either way: no per-language
  directories.
- Measure: cold start (download + unpack + start), peak memory → instance size.
- Afterwards: delete the old `korp-backend-fkv` Worker *and* its container
  application (deleting a Worker leaves the container app), and the R2 S3 keys
  it used.

**Done when:** fkv's API answers `/info` and `/concordance` on the test
account, from data in R2, deployed by the tool.

## 5. Frontend (static Worker)

No containers at runtime: build in a container, copy the files out, upload them
as a Worker with static assets only. Most of it exists: `build_front_base`,
`build_front`, `cf_deploy_front` and `assets_upload.rs` (uploads through the
REST API, no wrangler) in `manage/src/`.

- The frontend must be a Korp-9-compatible korp-frontend.
  `Containerfile.frontend-base` builds upstream `dev` + PR 509 (check what that
  PR is, and whether it's still needed).
- Backend URL: frontend configs (`gtweb2_config/front/config-<lang>.yaml`) set
  `korp_backend_url` at build time. Point it at the task-4 API. Also consider
  serving frontend and API from the same Worker (like Apertium), which would
  make the URL relative and avoid CORS. Decide together with task 4.
- Translations from `gtweb2_config/translations/`, logos from `logo_change/`.

**Done when:** `front-fkv` is deployed as a static Worker on the test account
and searches work against the task-4 API.

## 6. Check corpus configs against Korp 9 (independent)

The corpus configs in `gtweb2_config/corpus_configs/` were written for Korp 8.
Run the backend image locally (see the header of `Containerfile.backend`) for
each language that has local CWB data, and check that Korp 9 accepts the
configs: modes, attributes, `/corpora/info`, a concordance search per corpus.
Fix or list what needs changing.

**Done when:** fkv works fully; a list of needed changes for the others.

## Later

- Management commands once tasks 4 and 5 work: `deploy`, `status`, `logs`, `usage`, `images list|prune` (copy the ideas from Apertium's
  `manage.py`).
- All ~15 languages: corpora compiled (`compile-cwb-mono`), uploaded, deployed.
- Prod account (needs Workers Paid), URLs (gtweb.uit.no reverse proxy like
  `/jorgal/`, see `claude-log` section 5), the selector page
  (`selector-page/index.html`).
- Historical-orthography corpora (`gen-altorth-korps/`).
- Update `README.md` (it describes the old gtweb-02 setup).
