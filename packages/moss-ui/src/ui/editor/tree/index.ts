// The public surface of the file-tree subunit: code outside `tree/` imports
// this file only.
//
// The whole subunit lives here: the pure
// layer (model, spine projection, selection, keyboard map), the stateful seams
// (admission gate, spine, refresh, edit sessions, vault operations, menus, drag,
// renderer) and the controller that composes them.
//
// This lists only what outsiders use, which is little: `editor-main.ts` and
// `os-drag-in.ts` need the controller, and the render-gate harnesses measure the
// spine allocator. Everything inside `tree/` — the controller and the tests
// included — imports its sibling module directly, so a name needed only in here,
// or only by a test, does not belong in this file.
export { FileTree } from './controller';
export { createOneFile } from './refresh';

export { allocateSpine, navReserve, segIconFloor, FT_PR_BASE, FT_NAV_GUTTER } from './spine';

// Versions mode's title-bar content (versions-surface.ts builds every
// screen's back link + subject + actions from these.
export { mkBackCrumb, mkTitleBarRow } from './title-bar';
