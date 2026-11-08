// Per-asset cache-bust registry — the editor-side mirror of the preview
// bridge's `?_t=<ts>` image refresh (crates/moss-build/src/js-src/bridge/iframe-bridge.ts).
//
// WHY: a resolved editor image URL is a content-independent path
// (`/assets/x.jpg`, crates/moss-build/src/editor/resolve/asset_resolver.rs). When an image's
// bytes change on disk, the URL does NOT, so CM6's ImageBlockWidget.eq() reuses
// the stale <img> DOM and the browser serves its cached (broken) response. On
// the typed `AssetReady` event we bump a token for that asset; the token is
// folded into the resolved image URL as `?_t=<token>`, which changes the widget
// identity (forcing a repaint) AND defeats the browser cache (forcing a fetch).
//
// `?_t=<digits>` is the existing moss bust convention (navigation-manager.ts
// and iframe-bridge.ts strip it the same way).

/** request-url path → monotonic token. */
const tokens = new Map<string, number>();
let counter = 0;

/** Strip any query/hash and guarantee a single leading slash. */
function normalize(urlOrPath: string): string {
  // Hash is intentionally dropped — it is not part of the asset identity.
  const noQuery = urlOrPath.split('?')[0].split('#')[0];
  return noQuery.startsWith('/') ? noQuery : '/' + noQuery;
}

/**
 * Record that an asset became ready / changed. `path` is the `AssetReady.path`
 * (site-root-relative, no leading slash) — normalized to the request-url form
 * (`/assets/x.jpg`) so `assetBustQuery` can match it.
 */
export function bumpAssetBust(path: string): void {
  if (!path) return;
  tokens.set(normalize(path), ++counter);
}

/**
 * The `?_t=<token>` suffix to append to `requestUrl`, or '' if the asset has
 * never been bumped. `requestUrl` is the bare `ResolvedAsset.request_url`
 * (`/assets/x.jpg`); any pre-existing query is ignored for the lookup.
 */
export function assetBustQuery(requestUrl: string): string {
  const token = tokens.get(normalize(requestUrl));
  return token === undefined ? '' : `?_t=${token}`;
}

/** Test-only: clear all bust state. */
export function __resetAssetBustForTests(): void {
  tokens.clear();
  counter = 0;
}
