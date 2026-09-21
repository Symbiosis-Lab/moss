/**
 * Which `moss-cli` a render gate should drive.
 *
 * `MOSS_BIN` wins if set (CI builds the CLI once per job and points every
 * gate at that one binary). Otherwise
 * prefer the debug binary a developer already has, and fall back to release.
 * Nothing here checks the binary is actually executable — a caller that needs
 * a build to have happened first (`buildScratchSite`, a vite harness) makes
 * that check itself, so its error names the fixture it was building.
 */
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export function resolveMossBin(): string {
  if (process.env.MOSS_BIN) return process.env.MOSS_BIN;
  const debug = join(REPO_ROOT, "target/debug/moss-cli");
  if (existsSync(debug)) return debug;
  return join(REPO_ROOT, "target/release/moss-cli");
}
