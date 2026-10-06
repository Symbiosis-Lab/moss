/**
 * Where the render-gate fixtures find `moss-core`'s and `moss-build`'s own
 * source — tokens.json, site.css, the JS/TS a gate serves through vite.
 *
 * In this repo every crate is a plain workspace member at a fixed path, so
 * this is a lookup table, not a resolver: no `cargo metadata` shell-out, no
 * environment probing. (The desktop repo's equivalent has to ask cargo where
 * a crate lives, because there it is a pinned git dependency checked out at
 * an unpredictable path — that problem does not exist here.)
 */
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

const CRATE_DIRS: Record<string, string> = {
  "moss-core": join(REPO_ROOT, "crates/moss-core"),
  "moss-build": join(REPO_ROOT, "crates/moss-build"),
  "moss-cli": join(REPO_ROOT, "crates/moss-cli"),
};

/** The directory containing `name`'s `Cargo.toml`. */
export function openCrateDir(name: string): string {
  const dir = CRATE_DIRS[name];
  if (!dir) throw new Error(`no known crate directory for ${JSON.stringify(name)}`);
  return dir;
}

/** `moss-build`'s `src/assets` directory — the CSS/JS/templates most render gates read. */
export function mossBuildAssets(): string {
  return join(openCrateDir("moss-build"), "src", "assets");
}
