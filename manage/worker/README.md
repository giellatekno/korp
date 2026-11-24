# Korp Worker (one per language)

`korp-<lang>`: the frontend (korp-vue, static assets) and korp-backend (Korp 9)
in a container behind `/api/*`. One template for all languages
(`wrangler.jsonc`, `src/index.ts`); the management tool builds the language's
frontend into `.build/<lang>/dist/`, generates `.build/<lang>/wrangler.json`
next to it, and deploys that (`src/deploy.rs` in the tool). The frontend's
instance code is `korp-vue-instance/` in the repo root.

```
browser ──▶ Worker korp-<lang>.<subdomain>.workers.dev
            ├─ /api/*              Durable Object "korp-0" ──▶ korp-backend container (port 8000), prefix stripped
            ├─ POST /admin/restart stop the container (Authorization: Bearer $ADMIN_TOKEN)
            └─ everything else     the frontend: static assets, served without running the Worker
                                   (index.html for unknown paths); it calls the API at api/ next to the page

container start: the Durable Object reads R2 langs/<lang>/current → langs/<lang>/<release>.json,
and starts the container with DATA_URL=http://data.internal/<release> and CORPORA=<its corpora>.

container: enableInternet: false. Its only way out is http://data.internal (DataHandler):
  /<release>/config.tar.zst            ──▶ the config archive named in that release
  /<release>/corpora/<corpus>.tar.zst  ──▶ the corpus archive named in that release
  (only releases of this language; anything else 404)
```

The entrypoint of the image (`containerfiles/Containerfile.backend`) downloads
and unpacks those into `/corpus_config` and `/corpora` when `DATA_URL` is set,
then starts gunicorn. Without `DATA_URL` it uses mounted directories, as before.

## Why one Worker per language

Separate URLs, logs, stats and container sizes per language (sme's corpora are
~50× fkv's), and each language's frontend lives in the same Worker,
so the API URL is relative and no CORS is needed. The cost is one deploy per
language, which the tool does anyway.

## Data

The R2 bucket `korp-corpora` and its layout belong to `manage data` (see
`manage data --help`, `src/corpora.rs`). The Worker only reads it: the
language's current release decides the corpus config and corpora. So a data
change (`manage data upload <lang>`, `manage data rollback <lang>`) needs no
deploy, only a restart.

## Deploy

From `manage/` (the account id comes from the file `CLOUDFLARE_ACCOUNT_ID`):

```sh
cargo run --release -- data upload fkv           # once, and when data changes
cargo run --release -- deploy fkv --dry-run      # builds the frontend; no image build/push; checks R2, wrangler deploy --dry-run
cargo run --release -- deploy fkv                # frontend + image build, push, deploy, restart the container
```

The first deploy creates `worker/secrets.<account id>.env` with a random
`ADMIN_TOKEN` (passed with `--secrets-file` on every deploy). After a data
change (`data upload`, `data rollback`), restart the container so it fetches
the new current release:

```sh
cargo run --release -- restart fkv      # POST https://korp-fkv.<subdomain>.workers.dev/admin/restart
```

## Local test (real Docker, not podman: `unset DOCKER_HOST`)

Fill wrangler dev's local R2 with a release: upload to a local S3 server with
`KORP_S3_ENDPOINT=http://127.0.0.1:7070 AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=…
manage data upload fkv` (e.g. `versity/versitygw … posix /data` in Docker), then
copy each object with `npx wrangler r2 object put korp-corpora/<key> --local --file …`.

```sh
cd worker && npm ci
printf 'ADMIN_TOKEN="test"\n' > .dev.vars
(cd .. && cargo run --release -- build front fkv) && rm -rf dist && cp -r .build/fkv/dist dist
npx wrangler dev        # builds the image; then open http://localhost:8787/
```

**Over a VPN:** `wrangler dev` builds the image with docker's default network
(MTU 1500), and downloads in the build hang or are reset when the VPN's MTU is
smaller (e.g. tun0: 1191). The manage tool builds with `--network host` and
isn't affected. Work around it for `wrangler dev`: build the image with `manage
build back`, and point a copy of `wrangler.jsonc` at a Dockerfile containing
only `FROM korp-backend:<tag>` (or set `"mtu"` in docker's daemon.json).

## Measured (fkv, 2026-11-19, locally)

- Archives: 6.6 MB zstd -19 (15 MB unpacked), 5 corpora + config 3 KB.
- Cold start through the Worker (`wrangler dev`): 1.9 s from request to
  answer (download + unpack < 1 s, gunicorn ~1 s). After a restart: 1.4 s.
- Memory: ~265 MiB with 4 gunicorn workers, idle; with 2 workers (the
  default since 2026-11-24) ~140 MiB idle, ~290 MiB under load. Default size
  `{vcpu: 1, memoryMib: 512, diskMb: 2000}`; override per language with the
  var `CONTAINER_INSTANCE` (JSON). Measure again on Cloudflare, and for sme.
