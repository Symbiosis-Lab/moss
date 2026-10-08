// Shared wrapper for the render-gate Playwright configs (playwright/*.config.ts,
// one per gate). All 30 gates differ only in a handful of values — spec
// location, engines, a `use` block, whether and how a server is spun up — so
// this holds the repeated wiring (reporter, outputDir naming, project
// construction, webServer defaults) once, and each config states only what
// is particular to its gate. outputDir always lands under target/test-tmp/
// (gitignored, inside the repo rather than Playwright's default of a sibling
// directory), by construction.
//
// `projects` is always built from `engines` (never omitted), so every gate
// declares named projects — no gate here runs "unfiltered" the way a bare
// `use` with a device spread and no `projects` array would (ambient-video
// used to be that shape; see its own file for what that broke).
import { defineConfig, devices, type PlaywrightTestConfig } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { MOSS_BIN } from '../tests/e2e/helpers/scratch-site';

type Engine = 'chromium' | 'webkit' | 'firefox';

const ENGINE_PROJECT: Record<Engine, NonNullable<PlaywrightTestConfig['projects']>[number]> = {
  chromium: { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  webkit: { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  firefox: { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
};

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** A scratch site built by `buildScratchSite`, served over a throwaway `http.server`. */
interface ScratchHttpServer {
  /** The directory `buildScratchSite()` returned. */
  serveDir: string;
  port: number;
}

/** The shared nav-island/edge-clamp/vertical-nav-chrome vite dev harness. */
interface ViteHarnessServer {
  /** e.g. 'playwright/vite.nav-island-harness.config.ts' */
  configFile: string;
  port: number;
}

/**
 * The real preview server (`moss-cli build <srcDir> --serve`), for the one
 * gate (preview-parity-gate) whose subject IS the preview server's own
 * injected CSS/JS. `srcDir` is a scaffolded site's SOURCE directory
 * (`scratchSiteDir`, not `buildScratchSite`'s built output) — the preview
 * server builds and serves live, the same way a real `moss preview` does.
 */
interface MossPreviewServer {
  srcDir: string;
  port: number;
}

type GateServer = ScratchHttpServer | ScratchHttpServer[] | ViteHarnessServer | MossPreviewServer;

function isViteHarness(
  server: ScratchHttpServer | ViteHarnessServer | MossPreviewServer,
): server is ViteHarnessServer {
  return 'configFile' in server;
}

function isMossPreviewServer(
  server: ScratchHttpServer | ViteHarnessServer | MossPreviewServer,
): server is MossPreviewServer {
  return 'srcDir' in server;
}

// reuseExistingServer is always false, for every gate: a local run must fail
// loudly on a busy port rather than silently testing whatever else is already
// listening there (see "test: carry over today's macOS fixes").
function toWebServerConfig(server: ScratchHttpServer | ViteHarnessServer | MossPreviewServer) {
  if (isViteHarness(server)) {
    return {
      command: `npx vite --config ${server.configFile}`,
      cwd: repoRoot,
      port: server.port,
      reuseExistingServer: false as const,
      timeout: 60_000,
    };
  }
  if (isMossPreviewServer(server)) {
    // `--no-plugins` matches buildScratchSite's own build flags (see
    // scratch-site.ts), so a parity gate compares the preview server against
    // a static build made the same way, not one with plugins the preview
    // build lacks. `MOSS_PREVIEW_PORT_BASE` is the one env var the preview
    // server reads for its port (`ops/serve/port.rs::env_port_base`) — never
    // the default 8080, which may be a developer's own running app.
    // `run_headless_build` blocks forever under `--serve` (ctrl-C to stop),
    // exactly the shape Playwright's webServer wants.
    return {
      command: `"${MOSS_BIN}" build "${server.srcDir}" --serve --no-plugins`,
      cwd: repoRoot,
      url: `http://127.0.0.1:${server.port}/__moss_health/`,
      reuseExistingServer: false as const,
      timeout: 60_000,
      env: { ...process.env, MOSS_PREVIEW_PORT_BASE: String(server.port) },
    };
  }
  return {
    command: `/usr/bin/python3 -m http.server ${server.port}`,
    cwd: server.serveDir,
    url: `http://localhost:${server.port}/`,
    reuseExistingServer: false as const,
    timeout: 60_000,
  };
}

interface GateConfigOptions {
  /** The gate's own name: default spec match and default outputDir both come from it. */
  gate: string;
  /** Which tests/render-gates/<dir> the spec lives in. Default 'site'. */
  testDir?: 'site' | 'cascade';
  /** Spec file base name, for the one gate (site-elevation) whose spec is not its own name. */
  specName?: string;
  /** Engines to run. Default both — the reason most of these gates exist. */
  engines?: Engine[];
  /** Default false (serialized, one worker) — every gate here that owns a webServer or fakes shared route state needs that. A few proven independent set it true. */
  fullyParallel?: boolean;
  /** Passed straight through to Playwright's `use`. */
  use?: PlaywrightTestConfig['use'];
  /** One scratch-built site behind a throwaway static server, several (ui-accent-seam's two-site comparison), the shared vite harness, the real preview server (preview-parity-gate), or omitted for a stylesheet-injection gate that starts no server at all. */
  webServer?: GateServer;
  /** Overrides the default `../target/test-tmp/playwright-<gate>` — six ported gates kept a pre-existing, differently-spelled directory. */
  outputDir?: string;
  /** notebook-loads' JupyterLite boot needs more than Playwright's own default per-test timeout. */
  timeout?: number;
}

export function defineGateConfig(options: GateConfigOptions) {
  const testDir = options.testDir ?? 'site';
  const specName = options.specName ?? options.gate;
  const engines = options.engines ?? ['chromium', 'webkit'];
  const fullyParallel = options.fullyParallel ?? false;
  const webServer = options.webServer === undefined
    ? undefined
    : Array.isArray(options.webServer)
      ? options.webServer.map(toWebServerConfig)
      : toWebServerConfig(options.webServer);

  return defineConfig({
    testDir: `../tests/render-gates/${testDir}`,
    testMatch: new RegExp(`${specName}\\.spec\\.ts$`),
    fullyParallel,
    ...(fullyParallel ? {} : { workers: 1 }),
    reporter: 'list',
    outputDir: options.outputDir ?? `../target/test-tmp/playwright-${options.gate}`,
    ...(options.timeout ? { timeout: options.timeout } : {}),
    use: options.use ?? {},
    projects: engines.map((engine) => ENGINE_PROJECT[engine]),
    ...(webServer ? { webServer } : {}),
  });
}
