// Pure vocabulary + line formatting for `[preview-refresh]` (PreviewManager's
// refresh-decision diagnostic log). Split out from preview-manager.ts so the
// closed outcome/trigger vocabulary — and the two things derived from it that
// touch no manager state — have a single owner other tests and call sites can
// import, instead of living as private members of an already-large manager.

import { extractPathFromUrl } from './path-matcher';

/** What began a refresh decision — `PreviewManager.refresh()`'s own `source`
 *  values, plus `newPageWait-recheck` for the `handleFileChangeEvent` branch
 *  that reloads the awaited page directly instead of running smart-diff. */
export type RefreshTrigger = 'file-changed' | 'other' | 'newPageWait-recheck';

/** How a refresh decision resolved — logged once per decision so a release
 *  log, which otherwise says nothing about morph or reload, can name the
 *  broken link behind a stale pane. */
export type RefreshOutcome =
  | 'morph-applied'
  | 'morph-failed→reload'
  | 'reload'
  | 'iframe-nav-fallback'
  | 'deferred(first navigation)'
  | 'skipped(no page)'
  | 'skipped(navigation pending)'
  | 'none(page unaffected)'
  | 'redirect-home'
  | 'url-moved'
  | 'page-recheck';

/** What a decision compared against. `action` is `getFileChangeAction`'s
 *  verdict, `'reload-linked'` when the event named a file the page loads, or a
 *  stand-in (`'recheck'`) for the newPageWait branch, which bypasses
 *  smart-diff; `'refresh'` for a direct call. `path` is the currentPath compared against.
 *  `changedCount` is `changed_output_files.length`, 0 for a call with no
 *  event of its own. */
export interface RefreshDecisionContext {
  action: string;
  path: string | null;
  changedCount: number;
}

/** Render one `[preview-refresh]` line — the only place its shape is
 *  written, so the manager and this module's own tests stay in sync. */
export function formatRefreshDecisionLine(
  trigger: RefreshTrigger,
  ctx: RefreshDecisionContext,
  outcome: RefreshOutcome,
): string {
  return `[preview-refresh] trigger=${trigger} action=${ctx.action} path=${ctx.path ?? 'null'} changed=${ctx.changedCount} outcome=${outcome}`;
}

/** Decision context for a `refresh()` call with no smart-diff verdict of its
 *  own (the deferred first-navigation refresh, the redirect/rename `'other'`
 *  fallbacks). */
export function implicitRefreshContext(currentUrl: string | null): RefreshDecisionContext {
  return { action: 'refresh', path: currentUrl ? extractPathFromUrl(currentUrl) : null, changedCount: 0 };
}
