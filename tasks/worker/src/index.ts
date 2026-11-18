// The Korp Worker of one language: runs korp-backend in a container behind
// /api/*. The same code for all languages; the language and its corpora come
// from `vars` (see wrangler.jsonc, the template the management tool generates
// each language's config from).
//
//   /api/*               → korp-backend (Korp 9 API, prefix stripped), e.g.
//                          /api/info, /api/concordance?corpora=…&cqp=…
//   POST /admin/restart  (Authorization: Bearer $ADMIN_TOKEN)
//                        → stop the container; the next request starts a new
//                          one, which fetches the current corpora from R2
//   everything else      → 404 (later: the frontend, as static assets)
//
// The container has no internet access. Its only way out is the made-up
// hostname data.internal, which the Durable Object intercepts, and DataHandler
// below serves from R2: only this language's corpus config and corpora.

import { DurableObject, WorkerEntrypoint } from 'cloudflare:workers';

const KORP_PORT = 8000;
// One container per language. Name it differently to start a fresh one.
const KORP_INSTANCE = 'korp-0';
// Used when CONTAINER_INSTANCE is empty. CPU is billed by use, memory and disk
// by what's provisioned. The disk must hold the unpacked corpora.
const DEFAULT_RESOURCES = { vcpu: 1, memoryMib: 1024, diskMb: 2000 };
// Stop the container after this long without requests (max 6 h). Every start
// downloads and unpacks the corpora, see STARTUP_TIMEOUT_MS.
const IDLE_TIMEOUT_MS = 6 * 60 * 60 * 1000;
// How long to wait for Korp to answer after starting the container: download
// and unpack of the corpora + gunicorn startup.
const STARTUP_TIMEOUT_MS = 5 * 60_000;

const CORPUS_NAME = /^[a-z0-9_-]+$/;

const corpora = (env: Env): string[] => env.CORPORA.split(/\s+/).filter(Boolean);

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
    try {
      await (this.ready ??= this.startAndWait());
    } catch (error) {
      this.ready = undefined;
      console.error('Korp failed to start:', error);
      return Response.json({ error: 'The corpus search service is starting or unavailable, try again shortly.' }, { status: 503 });
    }
    const url = new URL(request.url);
    return this.container.getTcpPort(KORP_PORT).fetch(new Request(`http://container${url.pathname}${url.search}`, request));
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
      // Intercepts must be registered before start(), and again for each new container
      await container.interceptOutboundHttp('data.internal', this.ctx.exports.DataHandler);
      container.start({
        image: container.images.korp,
        instance: this.env.CONTAINER_INSTANCE ? JSON.parse(this.env.CONTAINER_INSTANCE) : DEFAULT_RESOURCES,
        enableInternet: false,
        env: {
          // Read by the entrypoint (see Containerfile.backend)
          DATA_URL: 'http://data.internal',
          CORPORA: corpora(this.env).join(' '),
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
//   http://data.internal/config.tar.zst
//       → configs/<KORP_LANG>/<version>.tar.zst      the corpus config of this language
//   http://data.internal/corpora/<corpus>.tar.zst
//       → corpora/<corpus>/<version>.tar.zst    one corpus, if it's in CORPORA
//
// where <version> is the content of the `current` object next to the archives
// (configs/<KORP_LANG>/current, corpora/<corpus>/current). Anything else is 404.
export class DataHandler extends WorkerEntrypoint<Env> {
  async fetch(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    let prefix: string | undefined;
    if (path === '/config.tar.zst') {
      prefix = `configs/${this.env.KORP_LANG}`;
    } else {
      const corpus = path.match(/^\/corpora\/([^/]+)\.tar\.zst$/)?.[1];
      if (corpus && CORPUS_NAME.test(corpus) && corpora(this.env).includes(corpus)) prefix = `corpora/${corpus}`;
    }
    if (request.method !== 'GET' || !prefix) return new Response('Not found', { status: 404 });

    const version = (await (await this.env.DATA.get(`${prefix}/current`))?.text())?.trim();
    const archive = version ? await this.env.DATA.get(`${prefix}/${version}.tar.zst`) : null;
    if (!archive) {
      console.error(`${prefix} not found in R2 (current: ${version ?? 'missing'})`);
      return new Response('Not found in R2', { status: 404 });
    }
    console.log(`Serving ${prefix}/${version}.tar.zst (${archive.size} bytes)`);
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

    return new Response(`Korp (${env.KORP_LANG}): the API is at /api/, e.g. /api/info\n`, { status: 404 });
  },
} satisfies ExportedHandler<Env>;
