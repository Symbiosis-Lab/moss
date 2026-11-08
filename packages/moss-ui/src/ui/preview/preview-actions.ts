// Preview action functions - pure functions for determining preview actions
// No side effects, no DOM dependencies

import { pathMatchesCurrentPage } from './path-matcher';
import type { FileChangeEvent } from '@symbiosis-lab/moss-ui';

export type { FileChangeEvent };

/**
 * Actions that can be taken in response to file changes.
 */
export type FileChangeAction =
  | { type: 'none' }              // No action needed
  | { type: 'refresh' }           // Refresh current page
  | { type: 'redirect-home' }     // Redirect to homepage (current page deleted)
  | { type: 'update-url'; newPath: string }; // Update URL (current page's output moved)

/**
 * Determine what action to take for a file change event.
 *
 * This implements "smart diff refresh" - we only refresh if the current page
 * was actually affected by the changes. This prevents preview flickering
 * when plugins write many files at once (e.g., downloading images).
 *
 * Priority order:
 * 1. Current page deleted -> redirect to homepage
 * 2. Current page's output moved -> update URL
 * 3. Current page content changed -> refresh
 * 4. Other changes -> no action (no flicker!)
 *
 * @param event - File change event from backend (null = simple refresh)
 * @param currentPath - Current page path (null = no page to check)
 * @returns Action to take
 */
export function getFileChangeAction(
  event: FileChangeEvent | null,
  currentPath: string | null
): FileChangeAction {
  // Null event = simple refresh (legacy behavior / fallback)
  if (!event) {
    return { type: 'refresh' };
  }

  // No current path = can't do smart diff, fall back to refresh
  // This happens when navigation history isn't initialized yet (first load)
  if (!currentPath) {
    // If there are changes, refresh to be safe
    if (hasAnyChanges(event)) {
      return { type: 'refresh' };
    }
    return { type: 'none' };
  }

  // Check if current page was deleted
  if (event.deleted_paths && event.deleted_paths.length > 0) {
    const currentPageDeleted = event.deleted_paths.some(
      deletedPath => pathMatchesCurrentPage(deletedPath, currentPath)
    );
    if (currentPageDeleted) {
      return { type: 'redirect-home' };
    }
  }

  // Check if current page was renamed
  if (event.moved_output_paths && event.moved_output_paths.length > 0) {
    for (const [oldPath, newPath] of event.moved_output_paths) {
      if (pathMatchesCurrentPage(oldPath, currentPath)) {
        return { type: 'update-url', newPath };
      }
    }
  }

  // Check if current page content changed (by hash)
  // changed_output_files contains paths relative to .moss/build/site/
  if (event.changed_output_files && event.changed_output_files.length > 0) {
    if (event.changed_output_files.includes(currentPath)) {
      return { type: 'refresh' };
    }
  }

  // Current page not affected - no action needed
  return { type: 'none' };
}

/**
 * The elements whose URLs a page loads. A FileChanged that names none of the
 * shown page's own outputs may still name one of these: a stylesheet, script
 * or image the build copied in after it showed the page.
 */
export const LINKED_SELECTOR =
  'link[href],script[src],img[src],img[srcset],source[src],source[srcset],video[src],video[poster],audio[src],iframe[src],embed[src],object[data]';

const URL_ATTRIBUTES = ['href', 'src', 'srcset', 'poster', 'data'];

/**
 * The site-root-relative paths (no leading slash, the form
 * `changed_output_files` uses) of every URL in `elements`' attributes,
 * resolved against `pageUrl`. Off-site URLs are left out, and so is what a
 * stylesheet loads itself (`url()`, `@import`, fonts): a change to one of
 * those refreshes the page only through its stylesheet's own change.
 */
export function linkedPaths(elements: ReadonlyArray<{ attributes: Record<string, string> }>, pageUrl: string): Set<string> {
  const origin = new URL(pageUrl).origin;
  const paths = new Set<string>();
  for (const { attributes } of elements) {
    for (const name of URL_ATTRIBUTES) {
      const value = attributes[name];
      if (!value) continue;
      const urls = name === 'srcset' ? value.split(',').map((c) => c.trim().split(/\s+/)[0] ?? '') : [value];
      for (const url of urls) {
        try {
          const resolved = new URL(url, pageUrl);
          if (resolved.origin === origin) paths.add(decodeURIComponent(resolved.pathname).replace(/^\//, ''));
        } catch {
          // Not a URL: nothing it loads can have changed.
        }
      }
    }
  }
  return paths;
}

/** Output files `event` names that are not pages: the ones a page can load. */
export function changedNonPages(event: FileChangeEvent): string[] {
  return (event.changed_output_files ?? []).filter((path) => !path.endsWith('.html'));
}

/**
 * Check if a file change event has any changes that might affect the preview.
 *
 * @param event - File change event from backend
 * @returns true if the event contains meaningful changes
 */
export function hasAnyChanges(event: FileChangeEvent | null): boolean {
  if (!event) return false;

  return !!(
    (event.changed_output_files && event.changed_output_files.length > 0) ||
    (event.deleted_paths && event.deleted_paths.length > 0) ||
    (event.moved_output_paths && event.moved_output_paths.length > 0)
  );
}

/** Result of a preview navigation, as reported by the iframe bridge. */
export interface NavigationResult {
  /** Pathname of the page the preview just navigated to (query stripped). */
  pathname: string;
  /** True when that page is the preview server's synthetic 404 page. */
  notFound: boolean;
}

/**
 * In-flight state for the new-page wait.
 *
 * Creating a page (or a folder) navigates the preview to the page's
 * build-INDEPENDENT URL immediately — before any build has produced it — so
 * that the preview follows the editor instead of sitting on the previous page
 * for the length of a build. Until that build lands, the URL serves the
 * preview server's synthetic 404, and the author's first sight of the page
 * they just made is an error screen. The wait covers that window with the
 * shell's one waiting surface.
 *
 * `null` means no wait is armed.
 */
export type NewPageWaitState = {
  /** Pathname of the page we are waiting for (query-agnostic). */
  pathname: string;
  /** Full URL to reload when a build reports it wrote output. */
  targetUrl: string;
  /** How many matching output changes have prompted a recheck. */
  rechecks: number;
  stalled?: boolean;
} | null;

/** What a new-page wait should do next. */
export type NewPageWaitDecision =
  | { type: 'ignore' }                         // no wait armed, or nothing new — leave state untouched
  | { type: 'arrived' };                       // the page serves — drop the screen, keep the page

/**
 * Pure decision for a navigation the bridge echoed while a new-page wait is armed.
 *
 * Rules (in order):
 * 1. No wait armed → `ignore`.
 * 2. The preview is showing a DIFFERENT pathname than the one we are waiting on
 *    (the author clicked away, or a later create superseded this one) →
 *    `arrived`: the wait is over and whatever is on screen owns the window.
 * 3. The tracked page loaded and is not a 404 → `arrived`.
 * 4. The tracked page is still the synthetic 404 → `ignore`: keep waiting.
 *    A 404 here is expected while the selected generation catches up.
 *
 * `superseded` is the pathname of a wait this one REPLACED (two ⌘N in quick
 * succession). Its navigation was already in flight when it was replaced, so
 * its echo can still land — after the new wait is armed, carrying the old
 * pathname. Rule 2 would read that as "the author clicked away" and uncover a
 * page whose build has not run, and the new wait's own echo would then find
 * nothing armed. One remembered pathname is enough: only the immediately
 * preceding create can still have an echo in flight, because arming the next
 * one is what replaces it.
 */
export function decideNewPageWait(
  state: NewPageWaitState,
  result: NavigationResult,
  superseded: string | null = null
): NewPageWaitDecision {
  if (!state) return { type: 'ignore' };
  if (result.pathname !== state.pathname) {
    return result.pathname === superseded ? { type: 'ignore' } : { type: 'arrived' };
  }
  if (!result.notFound) return { type: 'arrived' };
  return { type: 'ignore' };
}
