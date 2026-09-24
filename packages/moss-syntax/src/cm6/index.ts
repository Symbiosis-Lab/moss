// @symbiosis-lab/moss-syntax/cm6 — the CM6-importing, host-free layer: state
// predicates, pure tree extractors, and seam-injected extensions. Everything
// here imports only `@codemirror/*` / `@lezer/*` (peer deps — never bundled;
// a second `@codemirror/state` breaks facet identity) plus the package's own
// core layer. Host specifics arrive through options: strings as thunks
// (cm-shortcode-block), the reference cache + resolution envelope declared
// structurally (cm-link-resolver).
//
// The module set is collision-free by construction (verified at L2): each
// module exports a disjoint name set, so a flat re-export is unambiguous.

export * from './cm-active-lines.js';
export * from './cm-source-mode.js';
export * from './cm-editor-focus.js';
export * from './cm-link-extract.js';
export * from './cm-image-extract.js';
export * from './cm-criticmarkup.js';
export * from './cm-highlight.js';
export * from './cm-shortcode-block.js';
export * from './cm-link-resolver.js';
export * from './cm-footnote.js';
