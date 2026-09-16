// The shortcode parser moss and this plugin share — one implementation, no copy.
//
// This file used to be a stub pinning the surface the decoration layer needs
// (`scanShortcodeBlocks`, `KNOWN_SHORTCODES`, `ShortcodeBlock`,
// `ShortcodeSpec`). That surface now ships in `@symbiosis-lab/moss-syntax`,
// extracted from moss's editor (#1020, design:
// docs/archive/2026-08-11-cm6-extraction-design.md), so the stub becomes a
// re-export and the plugin has no parser of its own to drift.
//
// Why the text-driven scanner and not the Lezer grammar the package also
// exports: Obsidian owns its markdown parse and plugins cannot inject a
// `MarkdownConfig` into it, so the grammar is unreachable here. Importing
// `scanShortcodeBlocks` keeps `@lezer/markdown` out of the plugin bundle
// entirely — the package's core layer is type-only against it.
//
// Two behaviours of the real scanner the decoration layer must respect
// (see cm-shortcode.ts): an unclosed block comes back with `closeFrom: null`
// rather than being dropped, and nested blocks hang off `children` rather
// than appearing at top level.

export {
  scanShortcodeBlocks,
  KNOWN_SHORTCODES,
  type ShortcodeBlock,
  type ShortcodeSpec,
} from "@symbiosis-lab/moss-syntax";
