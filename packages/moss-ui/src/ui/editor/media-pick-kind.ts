// A leaf type with no dependencies, split out of cm-insert-bar.ts so that
// os-drag-in.ts's type-only import of it does not close a cycle through
// cm-insert-bar -> cm-reference-card -> asset-picker/browse-button ->
// os-drag-in.

/** Chips that open the file picker instead of inserting a bare template. */
export type MediaPickKind = 'media' | 'gallery' | 'hero';
