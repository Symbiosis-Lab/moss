// packages/moss-ui/src/ui/editor/preview-prospect.ts
//
// One classification of a project-relative path, read by two consumers:
// PreviewFollower's retry decision, and the note that tells the author why the
// preview did not move. Splitting them was the bug this file closes — the
// follower asked "can this ever be a page" while the note asked "is this under
// .moss/", so a text file that can never be a page got a silent retry.

import { isMossInternalPath } from './path-domain';
import { detectFileCategory } from './file-types';

/**
 * What the preview can do with a path.
 *
 * - `page` — the build may emit a page for it. Missing now means NOT YET
 *   (unbuilt, just renamed), so the follower retries on the next build.
 * - `site-wide` — it paints on every page rather than having one: a `.moss/`
 *   input, or a layout slot like `footer.md`. The preview deliberately stays
 *   on the page it is showing.
 * - `asset` — an image, video, font or other binary. It navigates by its own
 *   built asset URL, so a missing one means unbuilt, not unshowable; the
 *   action panel's native viewer is already showing it either way.
 * - `not-a-page` — a text file the build never turns into a page.
 *
 * Folders land in `not-a-page`. They reach the follower through the registry's
 * active-path signal, and folder navigation is a separate path
 * (`navigatePreviewTo('folder', …)`) that does not consult this.
 */
export type PreviewProspect = 'page' | 'site-wide' | 'asset' | 'not-a-page';

/** The subset of prospects that earn an on-screen explanation. */
export type PreviewNoteReason = 'site-wide' | 'not-a-page';

/**
 * The part of the backend's `SourceRole` this classifier reads.
 *
 * Declared structurally rather than importing the generated type, and nothing
 * can drift: PreviewFollower passes the generated `SourceRole` verbatim, so
 * dropping either field in Rust fails to typecheck at that call site.
 */
export interface PageVerdict {
  slot: string | null;
  can_be_page: boolean;
}

/**
 * Classify `relPath`, using the backend's answer when there is one.
 *
 * `role` is what `describe_source_role` returned, or `null` when it has not
 * been asked or the probe failed. `null` resolves to `page`, which is the
 * pre-existing fail-open behaviour: an unavailable answer must never suppress
 * navigation, and waiting is the harmless verdict.
 *
 * Whether the backend needs asking at all is `previewProspect(path, null) ===
 * 'page'` — the other three answers are path facts that no round trip can
 * change. The page question is NOT one of them: the extension set the build
 * accepts lives in `build::scan::classify::is_page_source`, and a second copy of
 * it here is the Rust↔TS parity trap the slot check already avoids.
 */
export function previewProspect(relPath: string, role: PageVerdict | null): PreviewProspect {
  const norm = relPath.replace(/\\/g, '/').replace(/^\.\//, '');
  if (isMossInternalPath(norm)) return 'site-wide';
  if (detectFileCategory(norm) !== 'text') return 'asset';
  if (!role) return 'page';
  if (role.slot) return 'site-wide';
  return role.can_be_page ? 'page' : 'not-a-page';
}

/**
 * True when no amount of rebuilding will produce a page URL, so the follower
 * must not arm its FileChanged retry loop.
 */
export function isStructurallyPermanentMiss(prospect: PreviewProspect): boolean {
  return prospect !== 'page';
}
