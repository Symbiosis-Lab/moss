// @symbiosis-lab/moss-syntax — the `.` (core) export: zero CM6 at runtime,
// `@lezer/markdown` type-only. See README.md.
//
// The module set is collision-free by construction (verified at L1): each
// module exports a disjoint name set, so a flat re-export is unambiguous.

export * from './shortcode.js';
export * from './wikilink-grammar.js';
export * from './math-grammar.js';
export * from './wikilink-syntax.js';
export * from './completion-core.js';
export * from './scan.js';
export * from './contract/shortcodes.generated.js';
export * from './footnote-grammar.js';
