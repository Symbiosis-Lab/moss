/**
 * This file has a twin in the desktop repo (pnpm's git dependency exposes
 * only `packages/*`, so that repo cannot import this one) — change both.
 *
 * Exempt localhost from any HTTP proxy, for every Playwright config that
 * starts a `webServer`.
 *
 * Playwright honours `HTTP_PROXY`/`HTTPS_PROXY` for its own requests, including
 * the `webServer.url` readiness probe. On a machine with a local proxy exported
 * shell-wide (a VPN//mitm client on `127.0.0.1:<port>` — common on macOS), that
 * probe is sent to the proxy, which cannot reach the throwaway static server
 * Playwright just spawned. The run then dies before a single assertion with a
 * bare `Timed out waiting 30000ms from config.webServer` — no clue that a proxy
 * was involved, and the server itself is fine (curl it directly and it answers).
 *
 * A loopback address is never something a proxy should carry, so this is
 * correct everywhere, not a local workaround: it only ADDS to whatever
 * `NO_PROXY` already says, and is a no-op when no proxy is set.
 *
 * Import for side effect, before `defineConfig`:
 *
 *     import './localhost-no-proxy';
 *
 * Both spellings are set — Playwright's fetch reads the lowercase `no_proxy`,
 * while Node tooling and most CLIs read `NO_PROXY`.
 */
const LOOPBACK = ['localhost', '127.0.0.1', '::1'];

for (const key of ['NO_PROXY', 'no_proxy']) {
  const present = (process.env[key] ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  const missing = LOOPBACK.filter((h) => !present.includes(h));
  if (missing.length) process.env[key] = [...present, ...missing].join(',');
}
