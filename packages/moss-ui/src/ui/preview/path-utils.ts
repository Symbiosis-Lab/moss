// Path utilities for preview URL display

/**
 * Convert a bare url_path (from PageInfo backend data) to a display path.
 *
 * Unlike extractDisplayPath which takes a full URL, this works on bare paths
 * like "about/index.html" or "feed.xml" — as returned by the Rust backend.
 *
 * - Adds leading slash
 * - Removes .html extension
 * - Converts index pages to directory paths (trailing slash)
 * - Converts root index to "/"
 */
export function urlPathToDisplayPath(urlPath: string): string {
  let path = urlPath;

  // Strip leading slash if present (normalize before processing)
  if (path.startsWith("/")) {
    path = path.slice(1);
  }

  // Remove .html extension
  if (path.endsWith(".html")) {
    path = path.slice(0, -5);
  }

  // Handle empty path or bare "index"
  if (path === "" || path === "index") {
    return "/";
  }

  // Handle nested index pages: "about/index" → "about/"
  if (path.endsWith("/index")) {
    return "/" + path.slice(0, -5); // Remove "index", keep trailing slash
  }

  return "/" + path;
}

/**
 * Extract clean, human-readable path from a full URL.
 *
 * - Decodes percent-encoded characters (including CJK)
 * - Removes .html extension
 * - Converts /index to /
 * - Converts /path/index to /path/
 */
export function extractDisplayPath(url: string): string {
  try {
    // Handle special URLs
    if (url === "about:blank" || url.startsWith("blob:")) {
      return "/";
    }

    const urlObj = new URL(url);
    // Decode percent-encoded characters (e.g., %E4%B8%AD -> 中)
    let pathname = decodeURIComponent(urlObj.pathname);

    // Remove .html extension
    if (pathname.endsWith(".html")) {
      pathname = pathname.slice(0, -5);
    }

    // Handle index pages (both root and nested)
    // /index → /
    // /research/index → /research/
    if (pathname === "/index" || pathname === "") {
      return "/";
    }
    if (pathname.endsWith("/index")) {
      return pathname.slice(0, -5); // Remove "index", keep trailing slash
    }

    // Ensure leading slash
    if (!pathname.startsWith("/")) {
      pathname = "/" + pathname;
    }

    return pathname;
  } catch {
    return "/";
  }
}
