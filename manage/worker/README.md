# Korp API Worker (one per language)

`korp-<lang>`: korp-backend (Korp 9) in a container, behind `/api/*`. One
template for all languages (`wrangler.jsonc`, `src/index.ts`); the management
tool generates `.build/<lang>/wrangler.json` per language and deploys it
(`src/deploy_back.rs` in the tool).

```
browser ──▶ Worker korp-<lang>.<subdomain>.workers.dev
            ├─ /api/*              Durable Object "korp-0" ──▶ korp-backend container (port 8000), prefix stripped
            ├─ POST /admin/restart stop the container (Authorization: Bearer $ADMIN_TOKEN)
            └─ everything else     404 (task 5: the frontend, as static assets)

container: enableInternet: false. Its only way out is http://data.internal (DataHandler):
  /config.tar.zst              ──▶ R2 korp-corpora: configs/<lang>/<current>.tar.zst
  /corpora/<corpus>.tar.zst    ──▶ R2 korp-corpora: corpora/<corpus>/<current>.tar.zst  (only corpora in CORPORA)
```

The entrypoint of the image (`containerfiles/Containerfile.backend`) downloads
and unpacks those into `/corpus_config` and `/corpora` when `DATA_URL` is set,
then starts gunicorn. Without `DATA_URL` it uses mounted directories, as before.

## Why one Worker per language

Separate URLs, logs, stats and container sizes per language (sme's corpora are
~50× fkv's), and each language's frontend (task 5) can live in the same Worker,
so the API URL is relative and no CORS is needed. The cost is one deploy per
language, which the tool does anyway.

## R2 layout (bucket `korp-corpora`, all languages of an account)

```
configs/<lang>/<version>.tar.zst     contents of gtweb2_config/corpus_configs/<lang>/
configs/<lang>/current               the version to use (text)
corpora/<corpus>/<version>.tar.zst   registry/<corpus> + data/<dir>/ (dir = last part of HOME)
corpora/<corpus>/current             the version to use (text)
```

`<corpus>` is the lowercase CWB name, same as the file names in
`corpus_configs/<lang>/corpora/*.yaml`, which also decide which corpora a
language gets (var `CORPORA`). Changing `current` needs no deploy, only a
restart. Until task 3 gives the tool an upload command, `stub-data.sh` makes
and uploads archives in this layout.

## Deploy

From `tasks/`:

```sh
export CLOUDFLARE_ACCOUNT_ID=$(cat CLOUDFLARE_ACCOUNT_ID)
cargo run --release -- cf deploy back-fkv --dry-run   # no build/push; checks R2, wrangler deploy --dry-run
cargo run --release -- cf deploy back-fkv             # build + push image, deploy, restart the container
```

The first deploy creates `worker/secrets.<account id>.env` with a random
`ADMIN_TOKEN` (passed with `--secrets-file` on every deploy). Restart by hand:

```sh
curl -X POST -H "Authorization: Bearer $(sed -n 's/^ADMIN_TOKEN=//p' worker/secrets.$CLOUDFLARE_ACCOUNT_ID.env)" \
     https://korp-fkv.<subdomain>.workers.dev/admin/restart
```

## Local test (real Docker, not podman)

```sh
cd worker && npm ci
KORP_DIR=… ./stub-data.sh fkv test1 --local   # from tasks/: fills wrangler dev's local R2
printf 'ADMIN_TOKEN="x"\nCORPORA="fkv_admin_20210319 …"\n' > .dev.vars
npx wrangler dev                               # builds the image; then curl localhost:8787/api/info
```

## Measured (fkv, 2026-11-19, locally)

- Archives: 6.6 MB zstd -19 (15 MB unpacked), 5 corpora + config 3 KB.
- Cold start through the Worker (`wrangler dev`): 1.9 s from request to
  answer (download + unpack < 1 s, gunicorn ~1 s). After a restart: 1.4 s.
- Memory: ~265 MiB with 4 gunicorn workers, idle. Default size
  `{vcpu: 1, memoryMib: 1024, diskMb: 2000}`; override per language with the
  var `CONTAINER_INSTANCE` (JSON). Measure again on Cloudflare, and for sme.
