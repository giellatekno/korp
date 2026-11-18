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
- **Accounts:** test = "UiT / Anders Lorentsen" (`tasks/CLOUDFLARE_ACCOUNT_ID`,
  has Workers Paid/Containers), prod = "Giellatekno" (no Workers Paid yet). Work
  on test.
- **Management tool:** the Rust crate in `tasks/` (replaces `tasks.py`). Run it
  from `tasks/`: `CLOUDFLARE_ACCOUNT_ID=$(cat CLOUDFLARE_ACCOUNT_ID) cargo run --release -- …`
- **Containers:** real Docker only, never podman (if `DOCKER_HOST` points to
  podman, `unset DOCKER_HOST`). Builds need `--load`.
- **Backend image:** `tasks/containerfiles/Containerfile.backend`: one image
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

## 1. Repo hygiene (small; do first) — DONE, one step left

Notes (2026-11-19):
- Root `.gitignore` added (credentials, build output, big data,
  `tasks/korp-backend-fkv/`, `.claude/`). `tasks/xxx`, `cloned/` and `x.py`
  were already gone.
- Deleted `cloudflare/` and root `containerfiles/`.
- main was fast-forwarded to d102bd1 (the tool as it was, committed by the
  task-2 session).
- Docker build context = repo root (the root `.dockerignore` lets only
  `gtweb2_config/` through; add `!logo_change/` in task 5). This is in task 2's
  branch (commit 6c319da). **Left to do:** once that branch is merged,
  `rm -rf tasks/gtweb2_config` (stale; nothing uses it after the merge).
- Left alone on purpose: the deleted submodules `korp-backend`/`korp-frontend`
  and the tracked `.deb` (unstaged deletions in `git status`), and the untracked
  `compile-cwb-mono/` and `claude-log`.

- Add a root `.gitignore`: credentials (`tasks/.CLOUDFLARE_API_TOKEN`,
  `tasks/s3_compat_credentials`, `tasks/xxx`, `**/.dev.vars`, `secrets*.env`),
  build output (`target/`, `node_modules/`, `.wrangler/`, `tasks/html/`,
  `**/dist/`), big data (`cwb-data/`, `compile-cwb-mono/compiled_mono/`).
- `tasks/xxx` contains an API token in plain text: delete the file (Anders
  decides whether to rotate the token).
- Decide which untracked directories to commit (`tasks/`, `compile-cwb-mono/`)
  and which old ones to delete (`cloned/`, `cloudflare/`, root `containerfiles/`,
  `tasks/gtweb2_config/` (stale copy of `gtweb2_config/`), `x.py`, the `.deb`).
- Commit (staged rename `repath.py` → `repath_cwb_registry.py` is pending).

**Done when:** `git status` shows only intended files, and no secrets are
committable.

## 2. Management tool: clean up the command structure (small, independent)

Everything is on Cloudflare now, so the `cf` subcommand level goes away:
`tasks build back`, `tasks build front <lang>`, `tasks push back`, `tasks
deploy …`. Remove dead code (`cf_get_worker`, `get` in `assets_upload.rs`,
unused variables), the `sync-settings` command that only prints rclone hints
(task 3 replaces it), and the `Instance` type if the new structure doesn't need
it. Read the account id from `CLOUDFLARE_ACCOUNT_ID` (file) when the env var
isn't set.

Don't build new features here: the deploy commands get rewritten in tasks 4
and 5 once those processes are established.

**Done when:** the tool compiles without warnings and `--help` shows the new
structure; `build back` still builds the image.

## 3. Corpus data in R2

How CWB data and corpus configs get into R2, in a form the backend container
can fetch at startup. Read first: `tasks/r2-data-notes.md` (takeaways from the
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

## 4. API Worker (backend on Cloudflare)

Replace `tasks/korp-backend-fkv/` (made from the containers template: deprecated
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
REST API, no wrangler) in `tasks/src/`.

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

- Management commands once tasks 4 and 5 work: `deploy`, `status`, `logs`,
  `restart`, `usage`, `images list|prune` (copy the ideas from Apertium's
  `manage.py`).
- All ~15 languages: corpora compiled (`compile-cwb-mono`), uploaded, deployed.
- Prod account (needs Workers Paid), URLs (gtweb.uit.no reverse proxy like
  `/jorgal/`, see `claude-log` section 5), the selector page
  (`selector-page/index.html`).
- Historical-orthography corpora (`gen-altorth-korps/`).
- Update `README.md` (it describes the old gtweb-02 setup).
