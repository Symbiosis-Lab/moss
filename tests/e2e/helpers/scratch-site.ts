/**
 * One scaffold-and-build routine for every render-gate playwright config.
 *
 * Each cascade/theme render gate needs the same three things: a throwaway moss
 * site under `target/test-tmp/`, a `moss build --no-plugins` of it, and the
 * built output directory to serve — which is a fresh `generations/<id>/` every
 * build, so it has to be resolved after the build, never guessed.
 *
 * ## Why this is called from the config and not from `globalSetup`
 *
 * Playwright starts `webServer` BEFORE `globalSetup`. Seven configs were wired
 * the other way round: `globalSetup` built the site and wrote its path to a
 * sentinel file, and the config read that sentinel — at parse time, one whole
 * phase earlier — to set `webServer.cwd`. On any machine that had run the gate
 * before, the stale sentinel made it look like it worked. From a clean
 * checkout all seven died with `spawn /bin/sh ENOENT`, which is Node reporting
 * a `cwd` that does not exist. `ui-accent-seam.config.ts` had already hit this
 * and worked around it by pasting a third copy of the build inline.
 *
 * Building here, synchronously, at config-parse time, is what that workaround
 * was reaching for. It also deletes the sentinel files and the `*_SERVE_DIR`
 * env vars outright: the built path is now just a value in the same process
 * that needs it. Everything below is deliberately sync (`execSync`, `fs`) so a
 * config can call it without top-level await.
 *
 * Before 2026-08-04 this routine was copy-pasted into seven `*-global-setup.ts`
 * files — ~1,100 lines of which the fixtures were maybe 200. All seven carried
 * their own `resolveBuiltDir`, and all seven still probed `.moss/build/site/`,
 * a path that has been empty since the generations migration. A bug in that
 * lookup had seven places to be fixed and no place to be tested.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { openCrateDir } from "../../support/crate-paths";
import { resolveMossBin } from "../../support/moss-bin";

// The embedded-JS output dirs `scripts/build-backend-scripts.mjs` derives
// EMBEDDED_DIRS from (its BUNDLES table) — the dirs a build embeds its site
// scripts from, so the stamp below needs to know them too.
const EMBEDDED_DIRS = [
  path.join(openCrateDir("moss-build"), "src/assets/js"),
  path.join(openCrateDir("moss-build"), "src/ops/serve/js"),
];

export const WORKTREE = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
/**
 * Which moss-cli renders the fixtures.
 *
 * Defaults to the debug binary a developer already has, falling back to
 * release. CI overrides it to the release artifact `build-cli` published, so
 * the gates cost one artifact download rather than a second compile of the
 * whole tree.
 */
export const MOSS_BIN = resolveMossBin();

export interface ScratchSiteSpec {
  /** Directory under `target/test-tmp/`. */
  name: string;
  /**
   * Site-relative path → contents. `null` deletes the file, which is how a
   * gate opts OUT of a `.moss/theme/style.css` a previous run left behind.
   * A `Buffer` writes raw bytes (e.g. a generated video fixture) — every
   * other gate's fixtures are text, so `writeOrRemove` only reaches for the
   * binary path when a spec actually hands it one.
   */
  files: Record<string, string | Buffer | null>;
  /**
   * Optional extra page written into the built output — used by gates whose
   * assertion needs markup moss would never emit (a `.moss-grid` in an article
   * footer, two custom-property probes). Receives the built `index.html` so it
   * can point at the real fingerprinted stylesheet names.
   */
  probe?: (indexHtml: string) => { fileName: string; html: string };
  /**
   * Build the site as if it were deployed at this URL (`MOSS_SITE_URL`).
   *
   * A site with no real URL emits no QR codes — encoding `http://localhost`
   * would be worse than encoding nothing — so a gate whose subject is the QR
   * has to say where the site lives. Nothing else in the output depends on it
   * being reachable; the codes just need a host to point at.
   */
  siteUrl?: string;
}

/** Scaffold, build, and return the directory to serve. */
export function buildScratchSite(spec: ScratchSiteSpec): string {
  const tag = `[${spec.name}]`;
  if (!fs.existsSync(MOSS_BIN)) {
    throw new Error(
      `${tag} moss-cli binary not found at ${MOSS_BIN}. ` +
        "Run `cargo build -p moss-cli` (or `cargo build --release -p moss-cli`) first, or set MOSS_BIN.",
    );
  }

  const siteDir = path.join(WORKTREE, "target/test-tmp", spec.name);
  for (const [rel, content] of Object.entries(spec.files)) {
    writeOrRemove(path.join(siteDir, rel), content);
  }

  // Reuse the previous build when nothing that feeds it has moved.
  //
  // This is not merely a speed nicety. Playwright parses the config once per
  // process — the runner, plus one per worker — so building unconditionally
  // here ran three times per invocation: the comments gate spent 1m51s
  // producing three identical sites to serve 1.5s of assertions. Whichever
  // process gets the lock builds; the others wait for its stamp.
  //
  // The stamp covers the complete input set — the fixtures written just above,
  // the binary that renders them, and the site scripts that binary reads off
  // disk in a dev build (see `siteScriptsHash`) — so a hit cannot serve stale
  // output.
  const stampPath = path.join(siteDir, ".moss/build-stamp.json");
  const stamp = createHash("sha256")
    .update(JSON.stringify(spec.files))
    .update(spec.siteUrl ?? "")
    .update(String(fs.statSync(MOSS_BIN).mtimeMs))
    .update(siteScriptsHash())
    .digest("hex");

  const fresh = readStamp(stampPath, stamp);
  if (fresh) return fresh;

  const lock = `${stampPath}.lock`;
  try {
    fs.mkdirSync(path.dirname(lock), { recursive: true });
    fs.mkdirSync(lock); // atomic: throws if another process is already building
  } catch {
    return awaitStamp(stampPath, stamp, tag);
  }
  try {
    console.log(`${tag} building scratch site …`);
    execSync(`"${MOSS_BIN}" build "${siteDir}" --no-plugins`, {
      cwd: siteDir,
      stdio: "pipe",
      timeout: 120_000,
      env: buildEnv(spec, siteDir),
    });
    const builtDir = findBuiltDir(siteDir);
    if (!builtDir) {
      throw new Error(`${tag} no build output under ${siteDir}/.moss/build/ after a build`);
    }
    if (spec.probe) {
      const indexHtml = fs.readFileSync(path.join(builtDir, "index.html"), "utf8");
      const { fileName, html } = spec.probe(indexHtml);
      fs.writeFileSync(path.join(builtDir, fileName), html, "utf8");
    }
    // Written last: the stamp is what tells a waiting process the build is
    // complete, probe page included.
    fs.writeFileSync(stampPath, JSON.stringify({ stamp, builtDir }), "utf8");
    return builtDir;
  } finally {
    fs.rmSync(lock, { recursive: true, force: true });
  }
}

/**
 * The site scripts a build would emit, as one hash — part of the stamp.
 *
 * A gate whose subject is a site script rather than a stylesheet can otherwise
 * assert against a bundle nobody is looking at. A dev build reads its
 * bundles from disk (`dev_path` in `build/emit/scripts.rs`, via
 * `load_js_asset`) rather than using its embedded copy, so editing
 * `crates/moss-build/src/js-src/site/**` and running
 * `node scripts/build-backend-scripts.mjs` changes what a build WOULD emit
 * without touching either the fixtures or the binary's mtime. Every input the
 * old stamp knew about was unchanged, so the stamp hit and the gate served
 * the previous bundle — a silent pass on the code you just edited, which is
 * the exact failure this whole gate exists to prevent.
 *
 * Hashing the directories is right for a release binary too: there the embedded
 * copy is what ships, and `build-backend-scripts.mjs` writes these same files,
 * so a stamp keyed on them is never weaker than one keyed on the binary alone.
 */
function siteScriptsHash(): string {
  const h = createHash("sha256");
  for (const dir of EMBEDDED_DIRS) {
    if (!fs.existsSync(dir)) continue;
    for (const name of fs.readdirSync(dir).sort()) {
      // The dir is in the digest, so a bundle moving between crates moves the
      // stamp — that move is exactly what silently emptied this hash once.
      h.update(dir).update(name).update(fs.readFileSync(path.join(dir, name)));
    }
  }
  return h.digest("hex");
}

/** The environment the build runs under — this process's, plus `MOSS_SITE_URL` for a gate that names one. */
function buildEnv(spec: ScratchSiteSpec, _siteDir: string): NodeJS.ProcessEnv {
  return spec.siteUrl
    ? { ...process.env, MOSS_SITE_URL: spec.siteUrl }
    : process.env;
}

/** The built dir a previous run recorded for this exact input set, if usable. */
function readStamp(stampPath: string, stamp: string): string | null {
  try {
    const saved = JSON.parse(fs.readFileSync(stampPath, "utf8"));
    if (saved.stamp !== stamp) return null;
    return fs.existsSync(path.join(saved.builtDir, "index.html")) ? saved.builtDir : null;
  } catch {
    return null; // absent or malformed — rebuild
  }
}

/** Block until the process holding the lock publishes a matching stamp. */
function awaitStamp(stampPath: string, stamp: string, tag: string): string {
  const deadline = Date.now() + 150_000;
  while (Date.now() < deadline) {
    const built = readStamp(stampPath, stamp);
    if (built) return built;
    // Sync sleep: playwright parses configs synchronously, so there is no
    // event loop to yield to here.
    execSync("sleep 0.5");
  }
  throw new Error(`${tag} timed out waiting for another process to build the scratch site`);
}

/**
 * Pull a stylesheet href out of built HTML so a probe page can link the real
 * fingerprinted file. Throws rather than emitting a page that links nothing —
 * a probe with no stylesheet renders default colours, and the gate then passes
 * or fails for a reason that has nothing to do with the cascade.
 */
export function requireLinkHref(html: string, pattern: RegExp, what: string): string {
  const m = html.match(pattern);
  if (!m) throw new Error(`could not find ${what} (${pattern}) in the built index.html`);
  return m[0];
}

/** As `requireLinkHref`, but for a link the gate tolerates being absent. */
export function findLinkHref(html: string, pattern: RegExp): string | null {
  return html.match(pattern)?.[0] ?? null;
}

/**
 * A tiny synthetic MP4 at an arbitrary `width`x`height`, for gates whose
 * subject only a real decoded video (not a text fixture like the SVGs in
 * `gate-sites.ts`) can exercise. Shells out to `ffmpeg`'s `testsrc` filter —
 * present by default on both GitHub-hosted runner images (`ubuntu-latest`,
 * `macos-latest`), so this needs no extra CI setup, and on a dev machine that
 * already needs it to run moss's own video pipeline.
 *
 * At least 2 seconds long: `FFmpegManager::generate_thumbnail` seeks to a
 * fixed `-ss 00:00:01` to grab the poster frame. A 1-second clip lands that
 * seek at-or-past EOF, so the poster comes out 0 bytes and the built page
 * serves `poster="…thumb.jpg"` as a 404. That silent failure doesn't just
 * cost a poster: WebKit ignores it, but Chromium resolves a `<video>`'s
 * `aspect-ratio: auto` from the poster image's own natural size, not the
 * decoded video frame, whenever a `poster` attribute is present — so a
 * broken poster made Chromium fall back to the CSS's placeholder 16:9 ratio
 * even after the video itself had fully decoded (`readyState` 4,
 * `videoWidth`/`videoHeight` correct). Confirmed by comparing a 1-second and
 * a 2-second clip through a real `moss-cli build`, isolated Playwright
 * launches, both engines: only the 1-second clip's build leaves a dead
 * `clip.thumb.jpg` link and only its Chromium run reports the wrong ratio.
 *
 * Cached under `target/test-tmp/fixtures/` keyed by dimensions, on the same
 * reasoning as `buildScratchSite`'s build-stamp cache: Playwright parses each
 * config once per worker process, and re-encoding on every parse would pay
 * the ffmpeg cost that many times over for bytes that never change.
 */
export function syntheticTestClip(width: number, height: number): Buffer {
  const cachePath = path.join(WORKTREE, `target/test-tmp/fixtures/clip-${width}x${height}.mp4`);
  if (fs.existsSync(cachePath)) return fs.readFileSync(cachePath);
  fs.mkdirSync(path.dirname(cachePath), { recursive: true });
  try {
    execSync(
      `ffmpeg -y -f lavfi -i "testsrc=size=${width}x${height}:rate=15" -t 2 -pix_fmt yuv420p "${cachePath}"`,
      { stdio: "pipe" },
    );
  } catch (e) {
    throw new Error(
      `could not generate the ${width}x${height} test clip — is ffmpeg on PATH? (${(e as Error).message})`,
    );
  }
  return fs.readFileSync(cachePath);
}

function writeOrRemove(filePath: string, content: string | Buffer | null): void {
  if (content === null) {
    fs.rmSync(filePath, { force: true });
    return;
  }
  if (Buffer.isBuffer(content)) {
    if (fs.existsSync(filePath) && fs.readFileSync(filePath).equals(content)) return;
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, content);
    return;
  }
  if (fs.existsSync(filePath) && fs.readFileSync(filePath, "utf8") === content) return;
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content, "utf8");
}

/**
 * `.moss/build.nosync/current` is a symlink re-pointed at a new
 * `generations/<id>/` every build. `staging/` is the fallback for a build
 * that did not ship. Per-machine output moved from `.moss/build` to
 * `.moss/build.nosync` (moss_paths.rs's `build_dir()`) while this helper
 * still pointed at the old name, so every GATES_BUILD gate failed
 * `no build output` on a fresh scratch site — there was never a legacy
 * `.moss/build` to migrate from in the first place. See moss_paths.rs's
 * `current_ptr`/`staging_dir` for the paths this mirrors.
 */
function findBuiltDir(siteDir: string): string | null {
  const currentLink = path.join(siteDir, ".moss/build.nosync/current");
  if (fs.existsSync(currentLink)) {
    const target = fs.realpathSync(currentLink);
    if (fs.statSync(target).isDirectory()) return target;
  }
  const staging = path.join(siteDir, ".moss/build.nosync/staging");
  return fs.existsSync(staging) ? staging : null;
}
