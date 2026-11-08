import { host } from "../host";

/**
 * The URL of a source file for the editor, from its root-relative
 * `requestUrl` (a ResolvedAsset's `request_url`: leading slash, not
 * percent-encoded) and a `?_t=<n>` bust or ''.
 */
export function sourceAssetUrl(requestUrl: string, bustQuery = ''): string {
  const path = requestUrl.startsWith('/') ? requestUrl : '/' + requestUrl;
  // encodeURI keeps '/' and encodes spaces and non-ASCII, which each host's
  // server decodes once.
  return host.sourceAssetBase + encodeURI(path) + bustQuery;
}
