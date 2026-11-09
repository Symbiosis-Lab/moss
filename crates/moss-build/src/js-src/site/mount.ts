/**
 * The path a site is served under, for scripts that build or read its URLs.
 *
 * A site published under a path (`https://example.edu/~alice/`) gets
 * `<meta name="moss:base" content="/~alice/">`, and the build has already put
 * every URL the browser loads from the HTML under it. What a script reads
 * from JSON or builds itself is a site path (`/notes/`), which `siteUrl`
 * mounts and `sitePath` takes back. A site at its origin's root has no meta,
 * and both functions return their input.
 */

function readBase(): string {
  const content = document.querySelector<HTMLMetaElement>('meta[name="moss:base"]')?.content ?? "/";
  return content.endsWith("/") ? content : content + "/";
}

/** The mount, e.g. `/~alice/`; `/` at the root. */
export const BASE: string = readBase();

/** `/notes/` → `/~alice/notes/`. Anything not site-root-relative is returned as is. */
export function siteUrl(path: string): string {
  if (BASE === "/" || path.charAt(0) !== "/" || path.charAt(1) === "/") return path;
  return BASE + path.slice(1);
}

/** `/~alice/notes/` → `/notes/`; a path outside the mount is returned as is. */
export function sitePath(pathname: string): string {
  if (BASE === "/") return pathname;
  if (pathname === BASE.slice(0, -1)) return "/";
  return pathname.startsWith(BASE) ? pathname.slice(BASE.length - 1) : pathname;
}
