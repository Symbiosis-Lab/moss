// wikilink-syntax.ts — the single place that interprets Obsidian wikilink /
// embed delimiters (`[[…]]`, `![[…]]`) for editor consumers.
//
// Frontmatter reference fields (e.g. `cover: '[[DSCF4053.jpeg]]'`) and the
// `children`/`sidebar` pickers store values in wikilink syntax. The build
// unwraps these before resolving (frontmatter_ref_to_stem + content-graph
// reference resolution); editor consumers must do the same or they resolve a
// literal `[[…]]` filename and report it broken. Consolidates the ad-hoc
// `replace(/^\[\[|\]\]$/g, '')` strip and `startsWith('[[') ? … : '[[…]]'`
// wrap that were scattered across chip-bar.ts.

const WIKILINK_RE = /^!?\[\[([\s\S]*)\]\]$/;

/**
 * Remove the wikilink/embed delimiters (`[[ ]]`, optional leading `!`),
 * preserving the inner text — including any `|alias` / `#anchor`. A bare path
 * passes through unchanged. Use this for the round-tripping picker display
 * where the alias must survive an edit.
 */
export function stripWikilinkBrackets(raw: string): string {
  const s = raw.trim();
  const m = WIKILINK_RE.exec(s);
  return m ? m[1].trim() : s;
}

/**
 * Resolve a wikilink/embed value to the bare reference target: strip the
 * `[[ ]]` delimiters AND drop the `|alias`/`|size` pothole and `#anchor`,
 * mirroring `classify_reference`'s split. Use this before resolving an asset
 * (e.g. the cover FilePicker chip) so the editor resolves the same source file
 * the build does.
 */
export function wikilinkTarget(raw: string): string {
  return stripWikilinkBrackets(raw).split('|')[0].split('#')[0].trim();
}

/** Wrap a bare reference in `[[ ]]` (idempotent; trims first). */
export function wrapWikilink(raw: string): string {
  const s = raw.trim();
  return s.startsWith('[[') ? s : `[[${s}]]`;
}

/**
 * Canonical moss embed insert: produces `![[name]]` from a bare filename or
 * folder path. No URL-encoding — the build fuzzy-resolves bare names;
 * separator/encoded paths 404 on deploy. Folder names ending with `/` are
 * kept as-is (e.g. `![[webapp/]]`).
 */
export function wrapEmbedWikilink(name: string): string {
  return `![[${name}]]`;
}
