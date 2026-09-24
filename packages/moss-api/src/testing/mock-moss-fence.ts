/**
 * Mock-side mirror of the `.moss/` plugin sandbox fence.
 *
 * Mirrors the real guard: `PluginPath::sandboxed` in
 * open/crates/moss-build/src/vault/fs.rs, and its one documented exception,
 * `PluginPath::shared_social_data`,
 * which is bound to the calling plugin's own `<plugin_id>.json` file — not
 * the whole shared directory. Before this mirror existed, mock-tauri.ts's
 * `write_project_file` / `read_project_file` handlers never enforced the
 * `.moss/` fence at all — so a plugin's own test suite kept passing for two
 * months after the real command started refusing the Matters plugin's
 * `.moss/data/social/matters.json` write, because "green" here proved
 * nothing about the real guard. Kept in its own module rather than inline in
 * mock-tauri.ts so that file's size doesn't grow for a self-contained policy
 * mirror.
 *
 * The example cases this rule must get right — a plugin's own file, a
 * sibling's, first-party `review.json`, the legacy directory, traversal —
 * are not hand-duplicated here: both this module's own test and the Rust
 * `PluginPath::shared_social_data` test read the same fixture table,
 * `open/fixtures/social-data-fence-cases.json`, so the two policies cannot
 * silently drift the way the mock and the real guard once did.
 */

/** Whether the first meaningful segment of a project-relative path is `.moss`. */
function isMossInternalPath(relativePath: string): boolean {
  const first = relativePath
    .replace(/\\/g, "/")
    .split("/")
    .find((seg) => seg !== "" && seg !== ".");
  return first !== undefined && first.toLowerCase() === ".moss";
}

/**
 * File stems under `.moss/data/social/` that name a first-party writer, not
 * a plugin — mirrors `RESERVED_SOCIAL_DATA_IDS` in vault/fs.rs. A plugin
 * manifest simply naming itself "review" would otherwise pass the
 * cross-plugin check trivially (the id and the file it claims genuinely
 * agree), so this is checked before the filename comparison.
 */
const RESERVED_SOCIAL_DATA_IDS = ["review"];

/**
 * The one documented exception to the `.moss/` fence: the calling plugin's
 * own file in the shared social-data standard's canonical directory
 * (`.moss/data/social/<pluginId>.json`) or its legacy home
 * (`.moss/social/<pluginId>.json`, plus that directory's `.migrated-bak`
 * archive copy) — never another plugin's file, and never first-party data
 * such as `review.json`.
 */
function isOwnSharedSocialDataPath(pluginId: string | null, relativePath: string): boolean {
  if (!pluginId) return false;
  if (RESERVED_SOCIAL_DATA_IDS.some((reserved) => pluginId.toLowerCase() === reserved)) {
    return false;
  }
  const segments = relativePath
    .replace(/\\/g, "/")
    .split("/")
    .filter((seg) => seg !== "" && seg !== ".");
  const ownCanonicalFile = `${pluginId}.json`;
  const ownLegacyArchiveFile = `${pluginId}.json.migrated-bak`;
  if (segments.length === 4 && segments[1] === "data" && segments[2] === "social") {
    return segments[0].toLowerCase() === ".moss" && segments[3] === ownCanonicalFile;
  }
  if (segments.length === 3 && segments[1] === "social") {
    return (
      segments[0].toLowerCase() === ".moss" &&
      (segments[2] === ownCanonicalFile || segments[2] === ownLegacyArchiveFile)
    );
  }
  return false;
}

const MOSS_FENCE_MESSAGE =
  "Access to .moss/ is not allowed. Use plugin storage (readPluginFile / writePluginFile) or readSiteFile instead.";

/** Throws the same message the real Rust guard returns, unless the path is
 * the calling plugin's own shared-social-data file. */
export function enforceMossFence(pluginId: string | null | undefined, relativePath: string): void {
  if (isMossInternalPath(relativePath) && !isOwnSharedSocialDataPath(pluginId ?? null, relativePath)) {
    throw new Error(MOSS_FENCE_MESSAGE);
  }
}
