// asset-urls.ts — pure URL helpers shared by the blueprint placeholder
// (`asset-placeholder.ts`, inlined into every page) and the preview's asset swap
// (`../bridge/iframe-bridge.ts`).
//
// Side-effect free ON PURPOSE. Both consumers are IIFE bundles; importing the
// placeholder module from the bridge would run its `install()` a second time and
// register a duplicate error listener. So the shared code lives here instead of
// being reached through either bundle.
//
// These four functions used to exist twice, as `toAbsPath` in iframe-bridge.ts
// and `normPath` in the deleted thumb-swap.ts, each with a comment telling the
// reader to update the other one by hand. Two implementations of one conversion
// is two answers; see the same lesson on `assets::paths::resolve_url`.

/**
 * Resolve a URL attribute to an absolute pathname, comparable with an
 * AssetReady/AssetsSettled event path.
 *
 * `decodeURIComponent` is load-bearing: moss percent-encodes emitted URLs per
 * segment while event paths arrive decoded, and plenty of real sites are
 * entirely non-ASCII (`關於/assets/頭像.webp`). Without it every image on such a
 * site fails to match and never recovers.
 */
export function normPath(url: string): string {
  if (!url) return "";
  try {
    return decodeURIComponent(new URL(url, location.href).pathname);
  } catch {
    return url;
  }
}

/** Strip any cache-bust `?_t=…` a previous swap or restore appended. */
export function stripBust(url: string): string {
  return url.replace(/\?_t=\d+/g, "");
}

/**
 * Split a `srcset` into its candidate URLs, dropping width/density descriptors.
 *
 * moss emits multi-candidate ladders — `photo.w800.webp 800w, photo.w1600.webp
 * 1600w` — so comparing a whole `srcset` attribute against one event path only
 * ever matched single-candidate sources. Every laddered image silently failed to
 * swap, placeholder or no placeholder.
 */
export function srcsetUrls(srcset: string): string[] {
  return srcset
    .split(",")
    .map((candidate) => candidate.trim().split(/\s+/)[0] ?? "")
    .filter((url) => url !== "");
}

/** True if any candidate in `srcset` resolves to `absPath`. */
export function srcsetMatches(srcset: string, absPath: string): boolean {
  return srcsetUrls(stripBust(srcset)).some((url) => normPath(url) === absPath);
}

/**
 * Append `bust` to every candidate URL in a `srcset`, preserving descriptors.
 * Mutating `srcset` is what re-runs the parent `<picture>`'s source-set
 * selection, and the bust is what defeats a negatively cached 404 from the
 * window when the variant genuinely was not there yet.
 */
export function bustSrcset(srcset: string, bust: string): string {
  return stripBust(srcset)
    .split(",")
    .map((candidate) => {
      const trimmed = candidate.trim();
      if (trimmed === "") return "";
      const space = trimmed.search(/\s/);
      return space === -1
        ? trimmed + bust
        : trimmed.slice(0, space) + bust + trimmed.slice(space);
    })
    .filter((candidate) => candidate !== "")
    .join(", ");
}
