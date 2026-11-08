/**
 * Join a site's bare host with one of its pretty page paths into a full
 * https URL.
 *
 * `page.path` never carries a leading slash — the article map's own
 * convention (the Rust side joins a base URL with a `PageChangeRecord` path
 * the same way). Naive
 * `${host}${path}` concatenation produces URLs like
 * `https://example.mosspub.comcommunity/blog/slug/`, missing the slash
 * after the host. Trimming both sides before joining tolerates a leading
 * slash anyway, and a doubled one, at no cost.
 */
export function pageUrl(host: string, path: string): string {
  return `https://${host.replace(/\/+$/, '')}/${path.replace(/^\/+/, '')}`;
}
