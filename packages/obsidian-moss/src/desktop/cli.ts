// Pure logic for talking to the moss CLI: binary discovery, argument
// building, and output parsing. No Obsidian imports and no I/O — everything
// filesystem/env-shaped is injected so vitest covers it without a vault.
//
// What the parsers read: `moss build <folder> --serve` prints ALL status to **stderr** via
// `cli_eprintln!` (crates/moss-build/src/build/cli_output.rs). The lines that matter:
//
//   Building website from: /path/to/vault
//   📁 'name': Site generated at /path/.moss/build.nosync/current
//   🌐 Preview server ready! Access at http://localhost:8080
//   Build failed: <reason>
//
// The port is dynamic (scan starts at 8080, crates/moss-build/src/ops/serve/port.rs),
// so the "Access at" line is the only machine-discoverable source of the URL.

/** Environment surface `findMossBinary` needs; injected for testability. */
export interface DetectEnv {
  /** Contents of process.env.PATH ("" when unset). */
  pathVar: string;
  /** path.delimiter — ":" on POSIX, ";" on Windows. */
  pathDelimiter: string;
  /** "win32" | "darwin" | "linux" */
  platform: string;
  /** os.homedir() */
  homeDir: string;
  /** Existence + executability check (fs.existsSync is enough for v1). */
  fileExists(p: string): boolean;
}

/** Binary name for the platform. */
export function binaryName(platform: string): string {
  return platform === "win32" ? "moss.exe" : "moss";
}

/**
 * Ordered candidate paths for the moss binary.
 *
 * The explicit settings path wins. Then every PATH entry. Then the usual
 * install locations that a GUI-launched Obsidian does not have on PATH —
 * macOS apps launched from Finder inherit launchd's minimal PATH, so
 * Homebrew/npm-global locations must be probed directly.
 */
export function candidateBinaryPaths(explicitPath: string, env: DetectEnv): string[] {
  const name = binaryName(env.platform);
  const sep = env.platform === "win32" ? "\\" : "/";
  const candidates: string[] = [];
  if (explicitPath.trim() !== "") candidates.push(explicitPath.trim());
  for (const dir of env.pathVar.split(env.pathDelimiter)) {
    if (dir !== "") candidates.push(dir + sep + name);
  }
  for (const dir of installDirs(env)) candidates.push(dir + sep + name);
  // De-duplicate, preserving order.
  return [...new Set(candidates)];
}

/**
 * Where the moss app and the CLI installers put the program, per platform. The
 * app's own binary answers the same commands as the CLI, so a writer who has
 * the app needs nothing else.
 */
function installDirs(env: DetectEnv): string[] {
  const home = env.homeDir;
  // Windows: PATH only until the per-user app is proven to answer over pipes.
  if (env.platform === "win32") return [];
  const app = env.platform === "darwin"
    ? ["/Applications/moss.app/Contents/MacOS", `${home}/Applications/moss.app/Contents/MacOS`]
    : [];
  return [...app, "/opt/homebrew/bin", "/usr/local/bin", `${home}/.local/bin`, `${home}/.npm-global/bin`];
}

/** First existing candidate, or null when moss is not installed. */
export function findMossBinary(explicitPath: string, env: DetectEnv): string | null {
  for (const p of candidateBinaryPaths(explicitPath, env)) {
    if (env.fileExists(p)) return p;
  }
  return null;
}

/**
 * The folder a preview builds: the site `moss root` found, else the clicked
 * folder, else the vault. Resolved with the platform's own path rules, so the
 * vault root's menu ("/") and the palette name the same site, and Windows
 * separators compare equal to what `moss root` prints.
 */
export function siteRoot(
  p: Pick<typeof import("node:path"), "join" | "resolve">,
  vault: string,
  target: string | undefined,
  found: string | null,
): string {
  return p.resolve(found ?? p.join(vault, target ?? ""));
}

export interface ServerAddress {
  url: string;
}

// The port requires a trailing non-digit: a chunk boundary can cut the line
// mid-port ("Access at http://localhost:8"), and without the lookahead that
// reads as a complete URL, then the next chunk completes the same line and
// reads again as a different one. `(?!\d)` doesn't close this: it's vacuously
// true at the buffer's end, exactly where a cut-off chunk ends. The CLI prints
// a second "Access at" line when it takes over serving a folder from another
// moss process.
const SERVER_URL_RE_ALL = /Access at (https?:\/\/(?:localhost|127\.0\.0\.1):\d{1,5})(?=\D)/g;

/**
 * Every "Access at" URL in a chunk of CLI output, in order. Callers replay
 * this list against persisted state to tell a first announcement from a later
 * change.
 */
export function parseServerUrls(text: string): ServerAddress[] {
  return [...text.matchAll(SERVER_URL_RE_ALL)].map((m) => ({ url: m[1] }));
}

/** The fatal-failure line, or null. (`Build failed: <reason>` on stderr.) */
export function parseBuildFailure(text: string): string | null {
  const m = /^Build failed: (.*)$/m.exec(text);
  return m ? m[1].trim() : null;
}

/** `Error: <reason>` lines (bad folder, nested-site guard, plugin gate). */
export function parseCliError(text: string): string | null {
  const m = /^Error: (.*)$/m.exec(text);
  return m ? m[1].trim() : null;
}

/**
 * The moss app's deep link that opens a folder for building, previewing and
 * publishing. The app percent-decodes the whole path, separators included.
 */
export function publishInAppUrl(folder: string): string {
  return `moss://deploy?path=${encodeURIComponent(folder)}`;
}

/**
 * The notice shown as the folder opens in the moss app, where it is published
 * and where the app shows what goes public. `address` is what `moss root
 * --json` reports (see `parseRootJson`).
 */
export function publishNotice(folder: string, address: SiteAddress): string {
  const to = address ? ` to ${address}` : address === null ? " to mosspub.com" : "";
  return `Opening ${folder} in moss to publish${to}.`;
}

/** A string once the site is live, null before its first publish, undefined when the CLI cannot say. */
export type SiteAddress = string | null | undefined;

/** What `moss root` says about a page the last build made (see moss's `cli::root`). */
export interface PageStatus {
  path: string;
  draft: boolean;
  /** Whether the last publish carried it; absent when moss cannot say now. */
  published?: boolean;
  /** Whether it changed since that publish; absent when moss cannot say now. */
  changed?: boolean;
  channels: string[];
  /** A posted copy's address, by channel. */
  copies: Record<string, string>;
}

const known = (v: unknown): boolean | undefined => (typeof v === "boolean" ? v : undefined);

function pageStatus(value: unknown): PageStatus | undefined {
  if (!value || typeof value !== "object") return undefined;
  const p = value as Record<string, unknown>;
  if (typeof p.path !== "string") return undefined;
  const channels = Array.isArray(p.channels) ? p.channels.filter((c): c is string => typeof c === "string") : [];
  const copies: Record<string, string> = {};
  for (const [channel, url] of Object.entries(p.copies && typeof p.copies === "object" ? p.copies : {})) {
    if (typeof url === "string") copies[channel] = url;
  }
  return { path: p.path, draft: p.draft === true, published: known(p.published), changed: known(p.changed), channels, copies };
}

/**
 * `moss root --json`: the site folder that owns a path (null when none does or
 * the run failed), its address, and for a page the last build made, its facts.
 * A CLI too old to print the address leaves it undefined.
 */
export function parseRootJson(stdout: string): { root: string | null; address: SiteAddress; page?: PageStatus } {
  try {
    const o: unknown = JSON.parse(stdout);
    if (o && typeof o === "object") {
      const { root, address, page } = o as Record<string, unknown>;
      return {
        root: typeof root === "string" ? root : null,
        address: address === null || typeof address === "string" ? address : undefined,
        page: pageStatus(page),
      };
    }
  } catch {
    // A failed run says nothing about the site.
  }
  return { root: null, address: undefined };
}
