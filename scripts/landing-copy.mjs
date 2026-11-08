// Shared by the locale generator and the served-site check.
export const escapeHtml = (value) => value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');

// Catalog strings a landing page must carry in its markup with no script run.
// Narrower than the catalog: the rest are set at runtime (form status, link
// titles) or live in attributes a text scan can't judge.
export const STATIC_COPY = ['title', 'description', 'intro', 'h1', 'b1', 'h2', 'b2', 'h3', 'b3', 'h4', 'b4', 'start', 'editor', 'theme', 'media', 'requestType', 'plugin', 'registry', 'closeH', 'closeB', 'download', 'macNote', 'soon', 'install', 'copy', 'betaH', 'betaB', 'email', 'request', 'privacy'];

/** The STATIC_COPY keys whose string is in neither raw nor escaped form in `html`. */
export function missingStaticCopy(html, catalog) {
  return STATIC_COPY.filter((key) => {
    // A key the catalog lacks would test `includes(undefined)`, which matches any page containing "undefined".
    if (typeof catalog[key] !== 'string') throw new Error(`STATIC_COPY names "${key}", which the catalog does not have`);
    return !html.includes(catalog[key]) && !html.includes(escapeHtml(catalog[key]));
  });
}
