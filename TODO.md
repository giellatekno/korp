# TODO: Korp on Cloudflare

Each task below is meant to be done in its own Claude Code session. Start a
session with: "Read TODO.md, section *Context*, and do task N". Each task lists
what to read first; don't read all of `claude-log` (55 KB), only the sections
named.

When a task is done, mark it done here, and write down anything the next tasks
need to know (decisions, names, URLs) under the task.

## Context (for every task)

- **Goal:** host Giellatekno's Korp (18 languages, one instance each) on
  Cloudflare, replacing the old on-prem gtweb-02 setup (`README.md` still
  describes the old setup). Same concepts as the Apertium deployment:
  `claude-log` sections 4 (architecture) and 5 (Cloudflare gotchas); the code
  is in `~/gut/giellatekno/apertium` (`deploy/cloudflare/src/index.ts`,
  `wrangler.jsonc`, `manage.py`).
- **Status (2026-11-20):** all 18 languages run end to end on the test
  account: `https://korp-<lang>.uit-anders.workers.dev` (frontend at `/`, API
  at `/api`, corpora from R2), publicly behind `https://gtweb.uit.no/korp/<lang>/`
  (*Later*, prod account), including sme's historical-orthography modes
  (task 7) and the parallel corpora (task 8). Open: task 6, the loose ends
  under tasks 7 and 8, and *Later*.
- **CWB data:** `~/misc/cwb/corpora/gt_cwb/{registry,data}` on Anders's machine
  has every corpus the configs reference (plus many old compilations; ignore
  its `config/` and `gt_cwb/`), except sme's historical-orthography corpora,
  which are in `gen-altorth-korps/generated/cwb/`. Pass them with `--cwb-dir`
  (repeatable, searched in order); a corpus not found locally is taken from
  the language's current release in R2.
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
- In R2 now: all 18 languages, first uploaded 2026-11-19; sme, sma, smj, fkv
  and smn have newer releases from 2026-11-20 (historical orthography,
  parallel corpora). `manage data list <lang>` shows a language's releases.
- New or changed data or corpus configs only need `data upload <lang>` +
  `restart <lang>`; `deploy` is only for frontend (`korp-vue-instance/`,
  `front/`) or Worker/image changes.

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

Known so far (2026-11-19, from the deployed instances, see task 7): every
corpus answers `/info` and a concordance search, and sme's default mode lists
its 8 corpora. Not yet checked: attributes in the frontend (sidebar,
statistics, extended search) per language. sme's config uses the
`*_20211118` corpora (from `gt_cwb`); `compile-cwb-mono` made newer
`*_20261026` ones, not used yet.

**Done when:** a list of needed config changes per language (or none).

## 7. All languages — DONE (deployed 2026-11-19)

All 18 languages are uploaded and deployed: fao fit fkv koi kpv mdf mhr mrj myv
olo sma sme smj smn sms udm vep vro, each at
`https://korp-<lang>.uit-anders.workers.dev`. Per language:

```
cargo run --release -- data upload <lang> --cwb-dir ~/misc/cwb/corpora/gt_cwb [--dry-run]
cargo run --release -- deploy <lang> [--dry-run]
```

Checked live (2026-11-19): every page loads; every `/api/info` answers (cold
start 16–34 s, mhr the slowest); a concordance search on every corpus works
(87 of 88, see below). Uploads were 2.9 GB before compression in total, mhr
the largest (1.18 GB); it fits the default container (1 vCPU, 1 GB, 2 GB
disk), since the entrypoint streams archives into `tar`.

sme's historical-orthography corpora (`friis_bible`, `friis_ficti`,
`friis_news`, `itkonen_news`, `leem_bible`, `leem_ficti`; modes `friis`,
`leem`, `itkonen`) were missing at first. Since 2026-11-20 they're compiled
by `gen-altorth-korps/` (its hfst-tokenize memory fix is described in
`debugging-crash.md`, untracked, in the main checkout's root) and live,
uploaded with:

```
cargo run --release -- data upload sme --cwb-dir ../gen-altorth-korps/generated/cwb --cwb-dir ~/misc/cwb/corpora/gt_cwb
cargo run --release -- restart sme
```

Loose ends:
- **mhr_news_20201209:** the match-everything query `[]` gives a 500
  ("unexpected backend error"); normal word searches work (~0.2 s). Probably
  memory (1 GB) when counting all hits of a large corpus; check the container
  log, and consider a bigger instance for mhr (`CONTAINER_INSTANCE`).
- **`deploy` of a brand-new Worker** ends with "restart failed": workers.dev
  answers 404 (1042) / 500 (1101) for longer than the 10 s the restart
  retries (happened for kpv and fao). The deploy itself is fine, and a new
  Worker has no container to restart; `manage restart <lang>` afterwards
  worked. Fix: retry longer (~60 s), or skip the restart when the Worker is new.
- **sme_wikipedia:** not in sme's config (only in the old `smi` one). Anders
  is checking whether it should be in Korp at all (quality).
- Cloudflare's bot check answers 403 (error 1010) to Python's default
  `urllib` User-Agent; set a User-Agent in scripts that call the API.

## 8. Parallel corpora — DONE (deployed 2026-11-20)

The sentence-aligned corpora (`xxx2yyy_<cat>_<date>-<lang>`: two CWB corpora
per pair, linked by CWB alignment, `ALIGNED` in the registry) are a `parallel`
mode in the instance of their minority language. Nothing new was needed in
the backend, the frontend or `manage`: Korp 9 searches `BASE|ALIGNED` pairs,
and korp-vue switches to its parallel search when the mode has
`parallel: true`.

| instance | pairs (from `gt_cwb`) |
|---|---|
| sme | `nob2sme_{admin,bible,facta,laws,science,mixed}_20191210` |
| sma | `nob2sma_{admin,bible,facta,science}_20190926` |
| smj | `nob2smj_{admin,facta}_20210929` |
| fkv | `nob2fkv_{admin,bible,facta,ficti,news}_20210319` |
| smn | `fin2smn_20170828` (no category; titled "Administrative texts") |

Left out on purpose: the undated `nob2sme_20131127` and `nob2sma_20170828`
(older compilations; the old gtweb1 config didn't use them either).

How it's set up (`corpus_configs/<lang>/`, ported from the old gtweb1
`~/projects/korp/frontend-setup/old_clientside_modes_on_gtweb1/parallel_mode.js`
and `parallel_fin_mode.js`):
- `modes/parallel.yaml`: `parallel: true`, `start_lang: <lang>`.
- One `corpora/*.yaml` per side, with `lang` and `linked_to` (the other
  side). The minority-language side is listed; the nob/fin side has
  `hide: true` and is picked along with it. Either language can be the one
  searched first (the language select in the search panel).
- Context and within are `link` (the alignment unit), not `sentence`.
- Both sides use the language's `pos`/`msd`/`lemma` attribute definitions;
  the nob tags are the same GT style (nob has `IM`, which isn't in the `pos`
  dropdown; typed CQP works).
- UI strings: `lang.<code>` names the languages in the parallel language
  select (korp-vue only has Språkbanken's). Added for sme, nob, sma, smj, fkv,
  smn in every instance locale, plus nob translations of the
  `search.parallel.*` strings; kept in `EXTRA` in
  `korp-vue-instance/port-translations.py` so a rerun keeps them.

Checked live: each instance serves the mode, and every pair answers an aligned
concordance query in both directions (`corpora=A|B`,
`cqp=[…] :LINKED_CORPUS:B […]`). Not yet clicked through in a browser.

Loose ends:
- **nob2fkv data is broken:** both sides are full of bogus `:` and `:\n`
  tokens (47,707 `:` in `nob2fkv_admin_20210319-nob`, the most frequent
  "word"), shown in results as "Ildsjeler : i : : fart :". A compile-time
  artefact; needs recompiling. One facta hit also paired unrelated sentences
  (one example only; extent unknown).
- Native-speaker check of the wording made up for this: "cealkkabárra"
  (sentence pair), the mode labels and descriptions in sme/fin, "Seaguhuvvon
  teavsttat" (`mixed`), and the language names in sme/rus.
- `search.parallel.add_query`/`remove_query`/`instructions` are only
  translated in nob; sme/fin/rus show them in English.

## Later

- Cold start: 16–34 s per language (fkv 26 s on its first deploy). Break it
  down (provisioning vs. R2 download vs. unpack); choose the instance size
  from measured peak memory, per language (mhr's data is ~180× fkv's).
- **Combined instances `smi` / `nsu` / `other` (parked; maybe dropped).**
  `gtweb2_config/corpus_configs/` (and `front/`) has three directories that
  aren't language codes, from the old setup where one Korp held several
  languages as *modes*. Anders wants to move away from them; their corpora are
  all in the single-language instances. If they're wanted after all:

  | instance | contents | default mode | other modes |
  |---|---|---|---|
  | `smi` | all Sámi languages | sme | sma, smj, smn, sms |
  | `nsu` | non-Sámi Uralic | kpv | koi, mdf, mhr, mrj, myv, udm |
  | `other` | the rest | fkv | fao, fit, olo, vep, vro |

  - No data of their own: archives are content-addressed, so `data upload`
    reuses the per-language archives already in R2. `nsu` (22 corpora) and
    `other` (22) list exactly their languages' corpora; `smi` (39) has
    `sme_wikipedia_20211118` (see task 7) and lacks sme's
    historical-orthography corpora (fine, per Anders).
  - Add `smi`, `nsu`, `other` to `manage/src/lang.rs` (only the 18 language
    codes are accepted now), and check that nothing treats the instance name
    as a language code (a locale, `LC_COLLATE`, the frontend's default language).
  - Workers `korp-smi`, `korp-nsu`, `korp-other`; check their modes with Korp 9.
- More management commands: `status`, `logs`, `usage`, `images list|prune`
  (ideas from Apertium's `manage.py`).
- Prod account (needs Workers Paid). The public URLs already work:
  `https://gtweb.uit.no/korp/<lang>/` is an nginx reverse proxy (like
  `/jorgal/`, see `claude-log` section 5) to the test account's
  `korp-<lang>.uit-anders.workers.dev` (all 18 checked 2026-11-20), and the
  selector page (`selector-page/index.html`) links there. Moving to prod means
  repointing the proxy.
- Update `README.md` (it describes the old gtweb-02 setup).
