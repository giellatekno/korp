# Getting CWB data from R2 into the backend container: notes

Written 2026-11-18, before we build this. Takeaways from the old approach
(`containerfiles/copy_data.py`, now deleted), and what to keep in mind for the
new one.

## What the old approach did

`Containerfile.backend-combined` (Korp 8.3) had an entrypoint that ran
`python /copy_data.py`, then gunicorn. The script:

1. Mounted the R2 bucket `korp-data` (test account) at `/mnt/korp-data` with
   **s3fs**, at `https://<account id>.r2.cloudflarestorage.com`, using R2 S3
   keys from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`. The
   `korp-backend-fkv` Worker passed those keys into the container as env vars.
2. Copied `korp_backend_config/config.py` to the Korp config location.
3. Copied `corpus_configs/$LANG/` to the corpus config dir (`LANG=fkv` was
   baked into the image).
4. Took the corpus names from the corpus config's file names, and copied
   **only those** corpora's registry files and data directories
   (`cwb-data/registry/<corpus>`, `cwb-data/data/<corpus>/`) to local disk.
5. Had a dry-run mode (env `dry-run=1`) that only printed what it would do.

Bucket layout it used (still in `korp-data` on the test account):

```
corpus_configs/<lang>/            korp corpus config per language
korp_backend_config/config.py     Korp 8 config.py (same for all languages)
cwb-data/registry/<corpus>        CWB registry files
cwb-data/data/<corpus>/           CWB data files
```

## Keep

- **Copy to local disk, don't serve from a mount.** CWB reads its files a lot;
  a FUSE-mounted R2 bucket is far too slow for that. Fetch everything at
  startup, then run.
- **Fetch only the corpora the instance needs**, decided by its corpus config
  (step 4). Same image for all languages; the language is runtime input.
- **Registry paths:** no longer a problem. `Containerfile.backend`'s
  entrypoint already rewrites HOME/INFO to `/corpora/data/<dir>` from a
  read-only `/corpora/registry`, so data can be fetched into `/corpora`
  unchanged, wherever it was built.

## Change

- **No s3fs, no R2 keys in the container, no internet for the container.**
  Do it like Apertium (`~/gut/giellatekno/apertium`, `deploy/cloudflare/`):
  the Worker intercepts a made-up hostname (e.g. `http://data.internal/...`)
  and serves objects from an R2 binding; the entrypoint fetches with curl.
  The Worker can also refuse anything the instance doesn't need.
- **Korp 9 has no `config.py`.** Settings are env vars (or `.env`); the
  Worker can pass them to the container at start. Only the corpus config
  and the CWB files need fetching.
- **One archive per corpus** (e.g. `<corpus>.tar.zst` with registry file +
  data dir) instead of thousands of small objects: one request per corpus,
  compressed, and versionable like Apertium's language pairs
  (`<corpus>/<version>.tar.zst` + a `current` pointer, rollback possible).
- The old Worker gives the container R2 S3 keys. When the new setup
  works, delete those keys (R2 API tokens in the dashboard).

## To find out when we get there

- **Sizes:** fkv is ~15 MB of CWB files; sme is ~719 MB (8 corpora, from
  `compile-cwb-mono`). Container disk (`diskMb`) must fit the largest
  instance, and the startup time (download + unpack) is part of every cold
  start: measure it, and choose the idle timeout with that in mind.
- `wrangler r2 object put` has a size limit (~315 MB); bigger archives need
  multipart upload (S3 API, or the REST API) from the management tool.
- Does Korp 9 read our existing `corpus_configs/<lang>/` as is? (Not
  verified yet; Korp 9 renamed a lot.)
- Korp 9's default `LC_COLLATE` is `sv_SE.UTF-8`, which the image doesn't
  have: set it, or install the locale.

## Update 2026-11-19

Built in task 3: layout, sizes and the upload command are in TODO.md (task 3)
and `tasks data --help`. Uploads use multipart through the S3 API.
