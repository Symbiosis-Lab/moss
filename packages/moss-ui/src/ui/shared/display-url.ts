/**
 * The one rule for showing a URL as text.
 *
 * moss displays serving URLs bare — `example.com/blog`, not
 * `https://example.com/blog/` — everywhere the URL is a name for the site
 * rather than something to be parsed. Four copies of this had drifted apart
 * (two stripped the trailing slash, two did not), which is why it lives in
 * one place now.
 *
 * Display only: never feed the result back to `openUrl` or anything that
 * needs a real URL. The href is always the untouched original.
 */
export function displayUrl(raw: string): string {
  return raw.replace(/^https?:\/\//, '').replace(/\/+$/, '');
}
