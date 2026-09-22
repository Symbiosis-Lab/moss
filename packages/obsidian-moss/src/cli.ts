// Pure logic for talking to the moss CLI: binary discovery, argument
// building, and output parsing. No Obsidian imports and no I/O — everything
// filesystem/env-shaped is injected so vitest covers it without a vault.
//
// Ground truth for the parsers (verified against moss 0.11.1, 2026-08-11):
// `moss build <folder> --serve` prints ALL status to **stderr** via
// `cli_eprintln!` (src-tauri/src/diagnostics.rs). The lines that matter:
//
//   Building website from: /path/to/vault
//   📁 'name': Site generated at /path/.moss/build.nosync/current
//   🌐 Preview server ready! Access at http://localhost:8080
//   moss: 2 problems reported above — the site was still generated.
//   Build failed: <reason>
//
// The port is dynamic (scan starts at 8080, src-tauri/src/preview/server/port.rs),
// so the "Access at" line is the only machine-discoverable source of the URL.
// Every moss preview server also serves GET /__moss_health/ whose JSON body
// contains "moss-preview-server" — used to validate a fallback port.

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
 * Homebrew/npm-global locations must be probed directly (the git/pandoc
 * model the design doc names).
 */
export function candidateBinaryPaths(explicitPath: string, env: DetectEnv): string[] {
  const name = binaryName(env.platform);
  const sep = env.platform === "win32" ? "\\" : "/";
  const candidates: string[] = [];
  if (explicitPath.trim() !== "") candidates.push(explicitPath.trim());
  for (const dir of env.pathVar.split(env.pathDelimiter)) {
    if (dir !== "") candidates.push(dir + sep + name);
  }
  const extraDirs =
    env.platform === "win32"
      ? []
      : [
          "/opt/homebrew/bin",
          "/usr/local/bin",
          `${env.homeDir}/.local/bin`,
          `${env.homeDir}/.npm-global/bin`,
        ];
  for (const dir of extraDirs) candidates.push(`${dir}/${name}`);
  // De-duplicate, preserving order.
  return [...new Set(candidates)];
}

/** First existing candidate, or null when moss is not installed. */
export function findMossBinary(explicitPath: string, env: DetectEnv): string | null {
  for (const p of candidateBinaryPaths(explicitPath, env)) {
    if (env.fileExists(p)) return p;
  }
  return null;
}

export interface BuildArgsOptions {
  serve?: boolean;
  watch?: boolean;
  /** Extra user-configured flags, already split (e.g. ["--no-plugins"]). */
  extraFlags?: string[];
}

/** argv (after the binary) for a moss build of `vaultPath`. */
export function buildArgs(vaultPath: string, opts: BuildArgsOptions = {}): string[] {
  const args = ["build", vaultPath];
  if (opts.serve) args.push("--serve");
  if (opts.watch) args.push("--watch");
  for (const f of opts.extraFlags ?? []) {
    if (f !== "") args.push(f);
  }
  return args;
}

/** Split a settings-box flag string into argv entries. */
export function splitFlags(raw: string): string[] {
  return raw.split(/\s+/).filter((f) => f !== "");
}

export interface ServerAddress {
  url: string;
  port: number;
}

const SERVER_URL_RE = /Access at (https?:\/\/(?:localhost|127\.0\.0\.1):(\d{1,5}))/;

/**
 * Find the preview-server URL in a chunk of CLI stderr, or null.
 * Tolerates the chunk containing many lines and partial-line buffering —
 * callers feed it the accumulated stream, not single lines.
 */
export function parseServerUrl(text: string): ServerAddress | null {
  const m = SERVER_URL_RE.exec(text);
  if (!m) return null;
  return { url: m[1], port: Number(m[2]) };
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
 * The `moss: N problem(s) reported above — …` summary count, or 0.
 * The `moss: ` prefix is load-bearing upstream (diagnostics.rs documents that
 * agents grep for it), so this anchors on it too.
 */
export function parseProblemCount(text: string): number {
  const m = /^moss: (\d+) problems? reported above/m.exec(text);
  return m ? Number(m[1]) : 0;
}

/** Substring that identifies a real moss preview server's health body. */
export const MOSS_HEALTH_MARKER = '"moss-preview-server"';

/** Path every moss preview server reserves for health checks. */
export const MOSS_HEALTH_PATH = "/__moss_health/";
