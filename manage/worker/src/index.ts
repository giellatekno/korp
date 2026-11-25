// The Korp Worker of one language: the frontend (korp-vue, static assets that
// Cloudflare serves without running this code, see "assets" in wrangler.jsonc),
// and korp-backend in a container behind /api/*. The same code for all languages; the language comes from the var
// KORP_LANG (see wrangler.jsonc, the template the management tool generates
// each language's config from).
//
//   /api/*               → korp-backend (Korp 9 API, prefix stripped), e.g.
//                          /api/info, /api/concordance?corpora=…&cqp=…
//                          (/api/info, /api/corpora/config, /api/corpora/info
//                          are answered from the Durable Object's storage once
//                          seen, so a page load doesn't start the container)
//   POST /admin/restart  (Authorization: Bearer $ADMIN_TOKEN)
//                        → stop the container; the next request starts a new
//                          one, with the language's current release from R2
//   everything else      → the frontend (static assets; index.html for unknown paths)
//
// What the container gets is the language's current *release* in R2 (written
// by `manage data upload`, see `manage data --help`):
//
//   langs/<lang>/current          the release id in use, e.g. 20261119T101500Z
//   langs/<lang>/<release>.json   {"config": {"key"}, "corpora": {"<corpus>": {"key"}, …}, …}
//
// The Durable Object reads it when it starts the container, and passes the
// release id and corpus names. The container has no internet access. Its only
// way out is the made-up hostname data.internal, which the Durable Object
// intercepts, and DataHandler below serves from R2: only archives named in a
// release of this language.

import { DurableObject, WorkerEntrypoint } from 'cloudflare:workers';

const KORP_PORT = 8000;
// One container per language. Name it differently to start a fresh one.
const KORP_INSTANCE = 'korp-0';
// Used when CONTAINER_INSTANCE is empty. CPU is billed by use, memory and disk
// by what's provisioned (memory is most of the bill: ~$6.6/GiB/month awake).
// The disk must hold the unpacked corpora. Korp with 2 gunicorn workers uses
// ~140 MiB idle, ~290 MiB under load (fkv, locally).
const DEFAULT_RESOURCES = { vcpu: 1, memoryMib: 512, diskMb: 2000 };
// Stop the container after this long without requests (max 6 h). Billing runs
// while it's awake, so keep it short. Every start downloads and unpacks the
// corpora, see STARTUP_TIMEOUT_MS.
const IDLE_TIMEOUT_MS = 15 * 60 * 1000;
// How long to wait for Korp to answer after starting the container: download
// and unpack of the corpora + gunicorn startup.
const STARTUP_TIMEOUT_MS = 5 * 60_000;

const RELEASE_ID = /^[0-9]{8}T[0-9]{6}Z$/;

// GET requests that every page load of the frontend makes (corpora/config and
// corpora/info; info for good measure). Their answers only change with the
// release, so the Durable Object keeps them in its storage, per release, and
// answers them without starting the container. A page view alone (a crawler,
// a link preview, someone who looks but doesn't search) then costs nothing.
const CACHED_PATHS = new Set(['/info', '/corpora/config', '/corpora/info']);
// Storage values can be at most 2 MiB; the answers are ~15 KB
const MAX_CACHED_BYTES = 1_000_000;

interface CachedResponse {
  body: string;
  contentType: string;
}

// langs/<lang>/<release>.json, as written by `manage data upload` (src/corpora.rs)
interface Release {
  lang: string;
  release: string;
  config: { key: string; size: number };
  corpora: Record<string, { key: string; size: number }>;
}

const currentReleaseId = async (env: Env): Promise<string | undefined> =>
  (await (await env.DATA.get(`langs/${env.KORP_LANG}/current`))?.text())?.trim() || undefined;

const currentRelease = async (env: Env): Promise<Release> => {
  const id = await currentReleaseId(env);
  if (!id) throw new Error(`no langs/${env.KORP_LANG}/current in R2: upload with \`manage data upload ${env.KORP_LANG}\``);
  const release = await loadRelease(env, id);
  if (!release) throw new Error(`langs/${env.KORP_LANG}/current names ${id}, which isn't in R2`);
  return release;
};

const loadRelease = async (env: Env, id: string): Promise<Release | null> => {
  if (!RELEASE_ID.test(id)) return null;
  const object = await env.DATA.get(`langs/${env.KORP_LANG}/${id}.json`);
  return object ? ((await object.json()) as Release) : null;
};

// Durable Object storage: RELEASE_KEY holds the release the container was
// last started with; cacheKey(…) the answers to CACHED_PATHS.
const RELEASE_KEY = 'release';
const cacheKey = (release: string, url: URL): string => `cache:${release}:${url.pathname}${url.search}`;

export class Korp extends DurableObject<Env> {
  // Shared by concurrent requests while the container starts
  private ready?: Promise<void>;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    const container = ctx.container;
    if (container?.running) {
      // The Durable Object restarted while the container kept running
      void ctx.blockConcurrencyWhile(() => container.setInactivityTimeout(IDLE_TIMEOUT_MS));
      this.watchExit(container);
    }
  }

  private get container(): Container {
    if (!this.ctx.container) throw new Error('No container configured for this Durable Object');
    return this.ctx.container;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const cacheable = request.method === 'GET' && CACHED_PATHS.has(url.pathname);
    if (cacheable) {
      const release = await this.servingRelease();
      const hit = release && this.ctx.storage.kv.get<CachedResponse>(cacheKey(release, url));
      if (hit) return new Response(hit.body, { headers: { 'content-type': hit.contentType, 'x-korp-cache': 'hit' } });
    }

    try {
      await (this.ready ??= this.startAndWait());
    } catch (error) {
      this.ready = undefined;
      console.error('Korp failed to start:', error);
      return Response.json({ error: 'The corpus search service is starting or unavailable, try again shortly.' }, { status: 503 });
    }
    const response = await this.container
      .getTcpPort(KORP_PORT)
      .fetch(new Request(`http://container${url.pathname}${url.search}`, request));

    // Stored under the release the container actually runs, which can differ
    // from the one looked up above if a new one was uploaded meanwhile
    const release = this.ctx.storage.kv.get<string>(RELEASE_KEY);
    if (!cacheable || !response.ok || !release) return response;
    const body = await response.text();
    const contentType = response.headers.get('content-type') ?? 'application/json';
    if (body.length <= MAX_CACHED_BYTES) {
      this.ctx.storage.kv.put<CachedResponse>(cacheKey(release, url), { body, contentType });
    }
    return new Response(body, { status: response.status, headers: { 'content-type': contentType, 'x-korp-cache': 'miss' } });
  }

  // The release whose answers are valid now: the running container's, or if
  // none runs, the one the next start will load.
  private async servingRelease(): Promise<string | undefined> {
    return this.ctx.container?.running ? this.ctx.storage.kv.get<string>(RELEASE_KEY) : await currentReleaseId(this.env);
  }

  // Remembers the release the container runs, and drops cached answers of
  // other releases.
  private setRelease(release: string): void {
    const kv = this.ctx.storage.kv;
    kv.put(RELEASE_KEY, release);
    for (const [key] of kv.list({ prefix: 'cache:' })) {
      if (!key.startsWith(`cache:${release}:`)) kv.delete(key);
    }
  }

  // Stops the container; the next request starts a new one, which fetches the
  // current corpus config and corpora.
  async restart(): Promise<void> {
    this.ready = undefined;
    if (this.container.running) {
      this.destroying = true;
      await this.container.destroy();
    }
  }

  // Set by restart(), so the container's exit isn't logged as an error
  private destroying = false;

  private async startAndWait(): Promise<void> {
    const container = this.container;
    const started = Date.now();
    if (!container.running) {
      const release = await currentRelease(this.env);
      const corpora = Object.keys(release.corpora).sort();
      console.log(`Starting Korp with release ${release.release}: ${corpora.length} corpora`);
      this.setRelease(release.release);
      // Intercepts must be registered before start(), and again for each new container
      await container.interceptOutboundHttp('data.internal', this.ctx.exports.DataHandler);
      container.start({
        image: container.images.korp,
        instance: this.env.CONTAINER_INSTANCE ? JSON.parse(this.env.CONTAINER_INSTANCE) : DEFAULT_RESOURCES,
        enableInternet: false,
        env: {
          // Read by the entrypoint (see Containerfile.backend)
          // The release is in the URL, so every download is from the same release
          DATA_URL: `http://data.internal/${release.release}`,
          CORPORA: corpora.join(' '),
          // Korp 9 settings (korp/config.py in korp-backend)
          CORS_ALLOW_ORIGINS: this.env.CORS_ALLOW_ORIGINS || '[]',
        },
      });
      this.watchExit(container);
    }
    await container.setInactivityTimeout(IDLE_TIMEOUT_MS);

    // `running` only means startup was requested; wait until Korp answers
    const deadline = started + STARTUP_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (!container.running) throw new Error('Korp container stopped during startup');
      try {
        const response = await container
          .getTcpPort(KORP_PORT)
          .fetch('http://container/info', { signal: AbortSignal.timeout(5000) });
        if (response.ok) {
          console.log(`Korp answers, ${((Date.now() - started) / 1000).toFixed(1)} s after the start request`);
          return;
        }
      } catch {
        // Not listening yet
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    throw new Error(`Korp did not answer on port ${KORP_PORT} within ${STARTUP_TIMEOUT_MS / 1000} s`);
  }

  private watchExit(container: Container): void {
    this.ctx.waitUntil(
      container
        .monitor()
        .then(() => console.log('Korp container stopped'))
        .catch((error) =>
          this.destroying ? console.log('Korp container stopped by restart') : console.error('Korp container exited:', error),
        )
        .finally(() => {
          this.ready = undefined;
          this.destroying = false;
        }),
    );
  }
}

// Serves the container's downloads, from R2 (bucket binding DATA):
//
//   http://data.internal/<release>/config.tar.zst
//       → the config archive named in langs/<KORP_LANG>/<release>.json
//   http://data.internal/<release>/corpora/<corpus>.tar.zst
//       → the archive of <corpus> named in that release
//
// Anything else is 404, so the container can only get this language's data.
export class DataHandler extends WorkerEntrypoint<Env> {
  async fetch(request: Request): Promise<Response> {
    const match = new URL(request.url).pathname.match(/^\/([^/]+)\/(config|corpora\/([^/]+))\.tar\.zst$/);
    if (request.method !== 'GET' || !match) return new Response('Not found', { status: 404 });
    const [, id, what, corpus] = match;

    const release = await loadRelease(this.env, id);
    const key = !release
      ? undefined
      : what === 'config'
        ? release.config.key
        : Object.hasOwn(release.corpora, corpus)
          ? release.corpora[corpus].key
          : undefined;
    const archive = key ? await this.env.DATA.get(key) : null;
    if (!archive) {
      console.error(`Not found: ${what} of release ${id} of ${this.env.KORP_LANG} (key: ${key ?? 'none'})`);
      return new Response('Not found in R2', { status: 404 });
    }
    console.log(`Serving ${key} (${archive.size} bytes)`);
    return new Response(archive.body, { headers: { 'content-length': String(archive.size) } });
  }
}

const timingSafeEqual = (a: string, b: string): boolean => {
  const encoder = new TextEncoder();
  const [x, y] = [encoder.encode(a), encoder.encode(b)];
  return x.byteLength === y.byteLength && crypto.subtle.timingSafeEqual(x, y);
};

export default {
  async fetch(request, env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === '/api' || url.pathname.startsWith('/api/')) {
      url.pathname = url.pathname.slice('/api'.length) || '/';
      return env.KORP.getByName(KORP_INSTANCE).fetch(new Request(url, request));
    }

    if (url.pathname === '/admin/restart') {
      const token = request.headers.get('authorization')?.replace(/^Bearer /, '') ?? '';
      if (request.method !== 'POST' || !env.ADMIN_TOKEN || !timingSafeEqual(token, env.ADMIN_TOKEN)) {
        return new Response('Forbidden', { status: 403 });
      }
      await env.KORP.getByName(KORP_INSTANCE).restart();
      return new Response('Korp restarted; the next request starts it with the current corpora.\n');
    }

    // Only /admin/* paths other than /admin/restart get here (see run_worker_first)
    return new Response('Not found\n', { status: 404 });
  },
} satisfies ExportedHandler<Env>;
