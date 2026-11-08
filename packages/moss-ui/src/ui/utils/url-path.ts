/**
 * url-path.ts — pure predicates about a site URL path. No Tauri, no DOM.
 *
 * The editor and the preview both ask these, so they live in neither: a pure
 * predicate with two callers that must agree sits below both.
 */

/**
 * True when a URL path addresses the site root/home, for any slash spelling
 * ('', '/', '///'). The root's canonical source is index.md / a self-named
 * folder note, and root detection from the URL is build-independent and
 * reliable (the lossy-slug problem is subfolder-only, never the root).
 *
 * Single source of truth for the rule, shared by the preview's navigation
 * cache and the Edit-open resolve, so the two can never drift.
 */
export function isRootUrlPath(urlPath: string): boolean {
  return urlPath.replace(/^\/+|\/+$/g, "") === "";
}
