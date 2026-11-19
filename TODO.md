# TODO: Korp on Cloudflare

Each task below is meant to be done in its own Claude Code session. Start a
session with: "Read TODO.md, section *Context*, and do task N". Each task lists
what to read first; don't read all of `claude-log` (55 KB), only the sections
named.

When a task is done, mark it done here, and write down anything the next tasks
need to know (decisions, names, URLs) under the task.

## Context (for every task)

- **Goal:** host Giellatekno's Korp (18 languages + 3 combined instances) on
  Cloudflare, replacing the old on-prem gtweb-02 setup (`README.md` still
  describes the old setup). Same concepts as the Apertium deployment:
  `claude-log` sections 4 (architecture) and 5 (Cloudflare gotchas); the code
  is in `~/gut/giellatekno/apertium` (`deploy/cloudflare/src/index.ts`,
  `wrangler.jsonc`, `manage.py`).
- **Status (2026-11-19):** fkv runs end to end on the test account:
  https://korp-fkv.uit-anders.workers.dev (frontend at `/`, API at `/api`,
  corpora from R2). The other languages wait for their CWB data (task 7).
- **Accounts:** test = "UiT / Anders Lorentsen" (`manage/CLOUDFLARE_ACCOUNT_ID`,
  has Workers Paid/Containers), prod = "Giellatekno" (no Workers Paid yet). Work
  on test.
- **Management tool:** the Rust crate in `manage/` (binary `manage`). Run it
  from `manage/`: `cargo run --release -- …` (the account id is read from the
  file `CLOUDFLARE_ACCOUNT_ID`, the API token from `.CLOUDFLARE_API_TOKEN`).
  Commands:
  - `data upload|list|rollback|create-bucket` — corpora + corpus configs in R2
  - `deploy <lang> [--dry-run]` — build frontend + backend image, push, deploy, restart
  - `restart <lang>` — restart a language's container (next request loads the current release)
  - `build back|front`, `push back` — the steps of `deploy` on their own
- **Containers:** real Docker only, never podman (if `DOCKER_HOST` points to
  podman, `unset DOCKER_HOST`; shells here often have it set). Builds need
  `--load`; the tool builds with `--network host` because Anders's VPN (tun0,
  MTU 1191) makes downloads on docker's bridge network hang.
- **Versions:** frontend **korp-vue** (branch `api-v9`) + **korp-backend**
  9.0.0.dev0, the combination Språkbanken runs. Both pinned by commit: `ARG
  KORP_VUE_COMMIT` in `Containerfile.frontend`, `ARG KORP_BACKEND_COMMIT` in
  `Containerfile.backend` (the only place the pins are). Update them together.
  Korp 9 renamed its API (`/query` → `/concordance`, `corpus=` → `corpora=`,
  `start`/`end` → `offset`/`limit`; upstream CHANGELOG).
- **Config:** `gtweb2_config/`: corpus configs (`corpus_configs/<lang>/`),
  frontend configs (`front/config-<lang>.yaml`), translations. Our frontend
  customisation (logos, footer, UI translations) is in `korp-vue-instance/`.
- **Working style:** Anders runs anything that changes the Cloudflare accounts
  (deploys, deletes, pushes, credentials) unless he asks Claude to; Claude
  prepares, verifies read-only, dry-runs, and gives him the commands. Before
  deleting an untracked file, say if it's the only copy of something. Don't
  commit Anders's staged submodule deletions (`.gitmodules`, `korp-backend`,
  `korp-frontend`) with other work: use `git commit -- <paths>` or a worktree.

---

## Done

### 1. Repo hygiene — DONE

Root `.gitignore` (credentials, build output, big data, `.claude/`); old
directories deleted. Docker build context = repo root, limited by the root
`.dockerignore`. Left alone on purpose: the staged submodule deletions (see
*Working style*), the untracked `compile-cwb-mono/` and `claude-log`.

### 2. Management tool cleanup — DONE

The `cf` subcommand level is gone; `Lang` (the list of known instances) is in
`manage/src/lang.rs`. `tasks/` was later renamed to `manage/`.

### 3. Corpus data in R2 — DONE

Bucket `korp-corpora` (test account), filled by `manage data upload <lang>`
(`manage/src/corpora.rs`; layout also in `manage data --help`):

```
corpora/<corpus>/<hash>.tar.zst   registry/<corpus> + data/<dir>/  → unpack into /corpora
configs/<lang>/<hash>.tar.zst     corpus_configs/<lang>/ contents → unpack into /corpus_config
langs/<lang>/<release>.json       {"lang", "release", "created",
                                   "config": {"key", "size"},
                                   "corpora": {"<corpus>": {"key", "size"}, …}}
langs/<lang>/current              text: the release id in use, e.g. 20261119T082811Z
```

- **Versioned per language**: a *release* pins the config archive and every
  corpus archive; `current` points to one; `data rollback <lang> [release]`
  moves the pointer. If an upload equals a release newer than `current`
  (rolled back from), the rollback stays, with a warning.
- **Content-addressed archives** (`<hash>` = first 16 hex of sha256 of a
  deterministic tar): unchanged data isn't uploaded again, and a corpus used by
  several instances (e.g. sme and smi) is stored once.
- **A language's corpora come from its corpus config** (`corpora/*.yaml`), not
  from name prefixes (sme's config also has `friis_*`, `leem_*`, `itkonen_*`).
  A listed corpus that isn't found locally is taken from the current release;
  if it isn't there either: error, or left out with `--skip-missing`.
- Upload goes through R2's S3 API (multipart above 100 MB) with S3 credentials
  derived from the API token. `--cwb-dir` defaults to `../cwb-data`.
- Sizes: CWB data is already compressed (sme_news 343 MB → 282 MB with zstd
  -19); the download dominates cold start.
- In R2 now: fkv only (release `20261119T082811Z`, 6.65 MB, 5 corpora + config).

### 4. API Worker — DONE

One Worker per instance, `korp-<lang>`, all from the template in
`manage/worker/` (`README.md` there); per-language config generated into
`worker/.build/<lang>/wrangler.json`. The Durable Object's container reads
`langs/<lang>/current` at start and gets `DATA_URL=http://data.internal/<release>`
+ `CORPORA`; the Worker serves only archives named in a release of this
language. Image pinned by digest. `/admin/restart` is authenticated
(`ADMIN_TOKEN`).

Measured on the test account: cold start 26 s from the first request (locally
1.9 s), then ~0.2 s per request. Not broken down yet (container provisioning
vs. download from R2); peak memory not measured.

Cleanup done: the old `korp-backend-fkv` Worker, its container app and bucket
`korp-data`. Left: revoke the R2 S3 keys the old Worker used (dashboard); delete
the Korp 8 images `korp-backend:8.3.0-*` from the registry.

### 5. Frontend — DONE (deployed 2026-11-19)

korp-vue, served as static assets by the same `korp-<lang>` Worker
(`run_worker_first` only for `/api` and `/admin/*`), so no CORS and no separate
frontend Worker. The API URL is computed in the browser (`api` next to the
page). `korp-vue-instance/` holds settings (from `front/config-<lang>.yaml`),
UiT + Giellatekno logos, our footer, and UI translations ported from the old
ones by English text (~145 of ~330 strings; the rest show in English).

Deployed with `manage deploy fkv`: the page loads with all 5 fkv corpora, and a
concordance search through the API works. The old Korp 8 frontend Workers
`korp-frontend-fkv`/`-sme` and `logo_change/` are deleted.

Left (small): translations of attribute values
(`gtweb2_config/translations/corpora-<lang>.json`) aren't ported to korp-vue.

---

## 6. Check corpus configs against Korp 9 (independent)

The corpus configs in `gtweb2_config/corpus_configs/` were written for Korp 8.
Run the backend image locally (see the header of `Containerfile.backend`) for
each language that has local CWB data, and check that Korp 9 accepts the
configs: modes, attributes, `/corpora/info`, a concordance search per corpus.
Fix or list what needs changing.

Known so far: sme's config names the `*_20211118` corpora, while
`compile-cwb-mono` made `*_20261026` (8 sme corpora, 719 MB, in
`compile-cwb-mono/compiled_mono/cwb/`). Either rename in the config or
recompile; decide with Anders.

**Done when:** fkv works fully (it does, as far as tested); a list of needed
changes for the others.

## 7. All languages, and the combined instances smi / nsu / other

**Waiting for:** Anders is locating the CWB data for the other languages.
Locally there is only fkv (`cwb-data/`) and sme (`compile-cwb-mono/compiled_mono/cwb/`).

Per language, once the data is there:

```
cargo run --release -- data upload <lang> --cwb-dir <dir> --dry-run
cargo run --release -- data upload <lang> --cwb-dir <dir>
cargo run --release -- deploy <lang> --dry-run
cargo run --release -- deploy <lang>
```

The 18 languages: fao fit fkv koi kpv mdf mhr mrj myv olo sma sme smj smn sms
udm vep vro.

**Combined instances.** `gtweb2_config/corpus_configs/` (and `front/`) also
has three directories that are not language codes. They come from the old
setup, where one Korp held several languages as *modes*:

| instance | contents | default mode | other modes |
|---|---|---|---|
| `smi` | all Sámi languages | sme ("North sami texts") | sma, smj, smn, sms |
| `nsu` | non-Sámi Uralic | kpv ("Komi-Zyrian texts") | koi, mdf, mhr, mrj, myv, udm |
| `other` | the rest | fkv ("Kven text") | fao, fit, olo, vep, vro |

They use the same corpora as the single-language instances, so once those are
uploaded they need no data of their own (archives are content-addressed and
shared). Checked against the per-language configs (2026-11-19):
- `nsu` (22 corpora) and `other` (22) list exactly the corpora of their languages.
- `smi` (39) has `sme_wikipedia_20211118`, which `sme`'s config doesn't, and
  lacks sme's historical-orthography corpora (`friis_bible`, `friis_ficti`,
  `friis_news`, `itkonen_news`, `leem_bible`, `leem_ficti`). Check with Anders
  whether that's intended.

To do for them:
- Add `smi`, `nsu`, `other` to `manage/src/lang.rs` (only the 18 language
  codes are accepted now). Check that nothing treats the instance name as a
  language code (e.g. a locale, `LC_COLLATE`, the frontend's default language).
- Worker names would be `korp-smi`, `korp-nsu`, `korp-other`.
- The configs are Korp 8 and untested with Korp 9 (task 6); modes are the
  part most likely to need changes.
- Decide with Anders whether all three are still wanted next to the
  single-language instances.

**Done when:** every instance whose data exists answers on the test account,
and the selector page (`selector-page/index.html`) lists them.

## Later

- Cold start: break down the 26 s (provisioning vs. R2 download vs. unpack);
  choose the instance size from measured peak memory, per language (sme is
  ~100× fkv).
- More management commands: `status`, `logs`, `usage`, `images list|prune`
  (ideas from Apertium's `manage.py`).
- Prod account (needs Workers Paid), URLs (gtweb.uit.no reverse proxy like
  `/jorgal/`, see `claude-log` section 5). The selector page
  (`selector-page/index.html`, redesigned 2026-11-19) links to
  `https://gtweb-02.uit.no/korp/<lang>`; change the links when the URLs are
  decided, and add rows for new instances.
- Historical-orthography corpora (`gen-altorth-korps/`).
- Update `README.md` (it describes the old gtweb-02 setup).
