import { host } from "../host";

/**
 * The URL of a source file for the editor, from its root-relative
 * `requestUrl` (a ResolvedAsset's `request_url`: leading slash, not
 * percent-encoded) and a query string: '' or one starting with `?`, such as
 * the `?_t=<n>` bust or `?original`.
 */
export function sourceAssetUrl(requestUrl: string, query = ''): string {
  const path = requestUrl.startsWith('/') ? requestUrl : '/' + requestUrl;
  // encodeURI keeps '/' and encodes spaces and non-ASCII, which each host's
  // server decodes once.
  return host.sourceAssetBase + encodeURI(path) + query;
}
