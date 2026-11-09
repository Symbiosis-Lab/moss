// Path matching utilities for preview system
// All functions are pure - no side effects, no DOM dependencies

/** Recognized index file stems (case-insensitive). Must mirror INDEX_STEMS in crates/moss-core/src/home.rs (public Symbiosis-Lab/moss repo). */
const INDEX_STEMS = ['index', 'readme', '_index', 'main'];

function isIndexStem(stem: string): boolean {
  return INDEX_STEMS.includes(stem.toLowerCase());
}

/**
 * Normalize a path by removing leading slashes.
 */
function normalizePath(path: string): string {
  return path.replace(/^\/+/, '');
}

/**
 * Check if a source file path matches the current HTML page.
 *
 * Handles various path mappings:
 * - "article.md" -> "article.html"
 * - "folder/article.md" -> "folder/article.html"
 * - Index files (index/readme/_index/main.md) -> "folder/index.html" or "folder/"
 * - Direct matches for non-markdown files
 *
 * NOTE: The Rust watcher, the only production producer of `deleted_paths` /
 * `moved_output_paths`, emits OUTPUT-domain paths exclusively, so the `.md`-handling
 * branches below are dead on the production path and run only from test fixtures.
 *
 * @param sourcePath - Source file path (e.g., "blog/post.md")
 * @param currentHtmlPath - Current HTML page path (e.g., "blog/post.html")
 * @returns true if the source path corresponds to the HTML page
 */
export function pathMatchesCurrentPage(
  sourcePath: string,
  currentHtmlPath: string
): boolean {
  const normalizedSource = normalizePath(sourcePath);
  const normalizedHtml = normalizePath(currentHtmlPath);

  // Direct match (for non-markdown files like images)
  if (normalizedSource === normalizedHtml) {
    return true;
  }

  // .md -> .html conversion
  const htmlFromMd = normalizedSource.replace(/\.md$/, '.html');
  if (htmlFromMd === normalizedHtml) {
    return true;
  }

  // Index file inference: folder/{stem}.md -> folder/ or folder/index.html
  // where stem is any recognized index stem (index, readme, _index, main)
  if (normalizedSource.endsWith('.md')) {
    const lastSlash = normalizedSource.lastIndexOf('/');
    const fileName = lastSlash >= 0 ? normalizedSource.slice(lastSlash + 1) : normalizedSource;
    const stem = fileName.slice(0, -3); // remove .md

    if (isIndexStem(stem)) {
      if (lastSlash >= 0) {
        // Nested: folder/{stem}.md -> folder/ or folder/index.html
        const folderPath = normalizedSource.slice(0, lastSlash + 1);
        if (normalizedHtml === folderPath ||
            normalizedHtml === folderPath + 'index.html') {
          return true;
        }
      } else {
        // Root: {stem}.md -> index.html or empty path
        if (normalizedHtml === 'index.html' || normalizedHtml === '') {
          return true;
        }
      }
    }
  }

  return false;
}

/**
 * Extract relative path from a preview URL.
 *
 * @param url - Full URL (e.g., "http://localhost:3000/blog/post.html")
 * @returns Relative path (e.g., "blog/post.html") or null if invalid
 */
export function extractPathFromUrl(url: string): string | null {
  try {
    const urlObj = new URL(url);
    let pathname = urlObj.pathname;

    // Remove leading slash
    if (pathname.startsWith('/')) {
      pathname = pathname.substring(1);
    }

    // Decode percent-encoded characters to match backend format
    // Backend sends filesystem paths (decoded), browser URLs are encoded
    pathname = decodeURIComponent(pathname);

    // Normalize empty path or root index to "index.html"
    if (pathname === '' || pathname === 'index.html') {
      return 'index.html';
    }

    // Normalize directory paths: "slug/" → "slug/index.html"
    // Browser shows "/slug/" for pretty URLs, but backend hash keys use "slug/index.html"
    if (pathname.endsWith('/')) {
      return pathname + 'index.html';
    }

    return pathname;
  } catch {
    return null;
  }
}

/**
 * Build a preview URL from port and optional path.
 *
 * @param port - Server port number
 * @param path - Optional path (e.g., "blog/post.html")
 * @returns Full URL (e.g., "http://localhost:3000/blog/post.html")
 */
export function buildPreviewUrl(port: number, path?: string): string {
  const base = `http://localhost:${port}`;
  if (!path || path === 'index.html') {
    return base;
  }
  return `${base}/${path}`;
}

/**
 * Convert a source markdown path to the corresponding HTML output path.
 *
 * @param sourcePath - Source file path (e.g., "blog/post.md")
 * @returns HTML output path (e.g., "blog/post.html")
 */
export function sourcePathToHtmlPath(sourcePath: string): string {
  return sourcePath.replace(/\.md$/, '.html');
}
