/**
 * shell-marker.ts — preview iframe URL marker for chrome-clearance pre-paint
 *
 * # What this is
 *
 * The shell adds a `__moss_shell=1` query parameter to every URL it sets
 * on the preview iframe. The preview server (`crates/moss-build/src/ops/serve/iframe_bridge.rs`) detects this marker and
 * rewrites the served `<html>` tag to carry `class="moss-shell-frame"` BEFORE
 * the first paint.
 *
 * Inert: the shell insets the preview iframe below the
 * titlebar, so the served document needs no clearance and `site.css` defines
 * no rule for that class. The marker still flows end to end and is kept as a
 * revert path.
 *
 * Without the marker, the bridge script (`iframe-bridge.ts`) would have
 * to add the class at runtime — which it still does as a defense-in-depth
 * fallback — but only after the document has already painted at top:0,
 * causing a visible 48px jump.
 *
 * # Why a query param and not a header / cookie
 *
 * - HTTP headers can't be set on `<iframe src=...>` from the parent
 *   document; the browser controls subresource request headers.
 * - Cookies are scoped per-origin, not per-frame, so a cookie set by the
 *   shell would leak into nested cover/embed iframes loaded from the
 *   same preview server (the very chrome-leak bug we're protecting
 *   against — see the 刘果 `交互` p5 sketches at
 *   test-sites/刘果/交互/活的空间.md and 林中星空.md).
 * - A query param is the only signal we control per-request.
 *
 * # Marker propagation
 *
 * - Initial load: shell calls `withShellMarker(url)` when assigning
 *   `iframe.src` (this file's `withShellMarker` and `SHELL_MARKER_PARAM`).
 * - In-iframe link clicks: bridge's link-click interceptor in
 *   `iframe-bridge.ts` re-stamps the marker before navigation.
 * - In-iframe `window.location = "..."` (rare, user-page JS): marker is
 *   lost. Bridge's runtime topology check (also in `iframe-bridge.ts`)
 *   adds the class as a fallback. One-frame flash, then correct.
 * - Nested `<iframe src="./sketch.html">` from a previewed page: bridge
 *   never gets a chance to mutate the URL (browser fires the request
 *   directly from parser). Marker is absent → server doesn't add class
 *   → no chrome leak into nested iframe. This is the load-bearing
 *   guarantee.
 *
 * # User-visible URLs
 *
 * The marker is a server-side side-channel and should never appear in
 * URLs the user sees (address-bar display, copy-link buttons, share
 * sheets). The shell strips it via `stripShellMarker()` when reporting
 * the iframe's URL externally. The bridge's `moss-navigation` postMessage
 * carries `location.href` which DOES contain the marker — strip it before
 * displaying.
 */

/**
 * The query parameter key. Underscores prefix it so it sorts to the end
 * of any user-visible query string and reads as "internal-only" to anyone
 * inspecting URLs in DevTools.
 */
export const SHELL_MARKER_PARAM = "__moss_shell";

/**
 * Returns `url` with `__moss_shell=1` appended (or merged into the existing
 * query string). Idempotent: calling on a URL that already has the marker
 * returns it unchanged. Preserves the URL's hash and other query params.
 *
 * Accepts both absolute (`http://localhost:1420/foo`) and relative URLs.
 * For relative URLs, resolves against the current `window.location` to
 * produce a parseable URL, then returns the result with the marker.
 *
 * Use this at every site where the shell assigns `iframe.src` for the
 * preview iframe (NOT for any other iframe — the marker is a chrome-
 * clearance signal specific to the preview surface).
 */
export function withShellMarker(url: string): string {
  // Use a base URL fallback so relative paths parse correctly. This is
  // safe because URL() with a base discards the base when the input is
  // absolute. We only end up using the base for relative URLs which are
  // always going to be served by the same origin as the current page.
  const base = typeof window !== "undefined" ? window.location.href : "http://localhost/";
  const parsed = new URL(url, base);
  parsed.searchParams.set(SHELL_MARKER_PARAM, "1");
  // Use `.toString()` rather than `.href` for compatibility with test
  // environments that mock the URL constructor with a partial impl.
  // Both are byte-identical on the real URL class per WHATWG spec.
  return parsed.toString();
}

/**
 * Removes the shell marker query param from a URL, returning the URL the
 * user should see (e.g. for display in a "Copy link" button or address-bar
 * mirror). Preserves all other query params and the hash.
 *
 * Returns the input unchanged if it can't be parsed as a URL.
 */
export function stripShellMarker(url: string): string {
  try {
    const base = typeof window !== "undefined" ? window.location.href : "http://localhost/";
    const parsed = new URL(url, base);
    if (!parsed.searchParams.has(SHELL_MARKER_PARAM)) {
      return url;
    }
    parsed.searchParams.delete(SHELL_MARKER_PARAM);
    // `.toString()` for test-env compatibility (see withShellMarker note).
    return parsed.toString();
  } catch {
    return url;
  }
}
