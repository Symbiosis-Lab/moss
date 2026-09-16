/**
 * HTTP operations for moss plugins
 *
 * These functions provide HTTP capabilities that bypass browser CORS
 * restrictions by using Rust's HTTP client under the hood.
 *
 * Project path for downloads is auto-detected from the runtime context.
 */

import { getTauriCore } from "./tauri.js";
import { getInternalContext } from "./context.js";

// ============================================================================
// Types
// ============================================================================

/**
 * Options for HTTP fetch requests
 * @category HTTP
 */
export interface FetchOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
}

/**
 * Result from an HTTP fetch operation
 * @category HTTP
 */
export interface FetchResult {
  /** HTTP status code */
  status: number;
  /** Whether the request was successful (2xx status) */
  ok: boolean;
  /** Content-Type header from response */
  contentType: string | null;
  /** Response body as Uint8Array */
  body: Uint8Array;
  /** Get response body as text */
  text(): string;
}

/**
 * Options for asset download
 * @category HTTP
 */
export interface DownloadOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
}

/**
 * Result from an asset download operation
 * @category HTTP
 */
export interface DownloadResult {
  /** HTTP status code */
  status: number;
  /** Whether the request was successful (2xx status) */
  ok: boolean;
  /** Content-Type header from response */
  contentType: string | null;
  /** Number of bytes written to disk */
  bytesWritten: number;
  /** Actual path where file was saved (relative to project) */
  actualPath: string;
}

// ============================================================================
// Internal Types (Tauri response shapes)
// ============================================================================

interface TauriFetchResult {
  status: number;
  ok: boolean;
  body_base64: string;
  content_type: string | null;
}

interface TauriDownloadResult {
  status: number;
  ok: boolean;
  content_type: string | null;
  bytes_written: number;
  actual_path: string;
}

// ============================================================================
// Functions
// ============================================================================

/**
 * Fetch a URL using Rust's HTTP client (bypasses CORS)
 *
 * @param url - URL to fetch
 * @param options - Optional fetch configuration
 * @returns Fetch result with status, body, and helpers
 * @throws Error if network request fails
 *
 * @example
 * ```typescript
 * const result = await fetchUrl("https://api.example.com/data");
 * if (result.ok) {
 *   const data = JSON.parse(result.text());
 * }
 * ```
 * @category HTTP
 */
export async function fetchUrl(
  url: string,
  options: FetchOptions = {}
): Promise<FetchResult> {
  const { timeoutMs = 30000 } = options;

  const result = await getTauriCore().invoke<TauriFetchResult>("fetch_url", {
    url,
    timeoutMs,
  });

  // Decode base64 body to Uint8Array using Uint8Array.from for better performance
  const binaryString = atob(result.body_base64);
  const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));

  return {
    status: result.status,
    ok: result.ok,
    contentType: result.content_type,
    body: bytes,
    text(): string {
      return new TextDecoder().decode(bytes);
    },
  };
}

/**
 * Options for HTTP POST requests
 * @category HTTP
 */
export interface PostOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers */
  headers?: Record<string, string>;
}

/**
 * Perform an HTTP POST request with JSON body
 *
 * Uses Rust's HTTP client to bypass browser CORS restrictions.
 * This is useful for OAuth flows and other API interactions.
 *
 * @param url - URL to POST to
 * @param body - JSON object to send as the request body
 * @param options - Optional configuration including timeout and headers
 * @returns Fetch result with status, body, and helpers
 * @throws Error if network request fails
 *
 * @example
 * ```typescript
 * // GitHub OAuth device code request
 * const result = await httpPost(
 *   "https://github.com/login/device/code",
 *   { client_id: "xxx", scope: "repo workflow" },
 *   { headers: { Accept: "application/json" } }
 * );
 * if (result.ok) {
 *   const data = JSON.parse(result.text());
 * }
 * ```
 * @category HTTP
 */
export async function httpPost(
  url: string,
  body: Record<string, unknown>,
  options: PostOptions = {}
): Promise<FetchResult> {
  const { timeoutMs = 30000, headers = {} } = options;

  const result = await getTauriCore().invoke<TauriFetchResult>("http_post", {
    url,
    body: JSON.stringify(body),
    headers,
    timeoutMs,
  });

  // Decode base64 body to Uint8Array using Uint8Array.from for better performance
  const binaryString = atob(result.body_base64);
  const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));

  return {
    status: result.status,
    ok: result.ok,
    contentType: result.content_type,
    body: bytes,
    text(): string {
      return new TextDecoder().decode(bytes);
    },
  };
}

/**
 * Convert an HTML fragment to Markdown via moss's bundled `htmd` converter —
 * the same converter the rest of the app uses. Plugins call this instead of
 * shipping their own HTML→Markdown pass, so output (notably hard breaks, which
 * htmd renders as two trailing spaces rather than a lone backslash) is
 * consistent app-wide. Returns the input HTML unchanged if conversion fails.
 * @category HTTP
 */
export async function htmlToMarkdown(html: string): Promise<string> {
  return getTauriCore().invoke<string>("html_to_markdown", { html });
}

/**
 * Options for HTTP GET requests
 * @category HTTP
 */
export interface GetOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers */
  headers?: Record<string, string>;
}

/**
 * Perform an HTTP GET request
 *
 * Uses Rust's HTTP client to bypass browser CORS restrictions.
 * This is useful for API interactions that require custom headers.
 *
 * @param url - URL to GET
 * @param options - Optional configuration including timeout and headers
 * @returns Fetch result with status, body, and helpers
 * @throws Error if network request fails
 *
 * @example
 * ```typescript
 * // Buttondown API newsletter info request
 * const result = await httpGet(
 *   "https://api.buttondown.com/v1/newsletters",
 *   { headers: { Authorization: "Token xxx" } }
 * );
 * if (result.ok) {
 *   const data = JSON.parse(result.text());
 * }
 * ```
 * @category HTTP
 */
export async function httpGet(
  url: string,
  options: GetOptions = {}
): Promise<FetchResult> {
  const { timeoutMs = 30000, headers = {} } = options;

  const result = await getTauriCore().invoke<TauriFetchResult>("http_get", {
    url,
    headers,
    timeoutMs,
  });

  // Decode base64 body to Uint8Array using Uint8Array.from for better performance
  const binaryString = atob(result.body_base64);
  const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));

  return {
    status: result.status,
    ok: result.ok,
    contentType: result.content_type,
    body: bytes,
    text(): string {
      return new TextDecoder().decode(bytes);
    },
  };
}

/**
 * One ordered text field in a multipart/form-data POST.
 *
 * Order is preserved because the GraphQL multipart request spec requires
 * `operations` before `map` before the file parts.
 * @category HTTP
 */
export interface MultipartTextField {
  /** Form field name (e.g. "operations", "map"). */
  name: string;
  /** Field value (e.g. the JSON-encoded GraphQL operation). */
  value: string;
}

/**
 * One file part in a multipart/form-data POST. Bytes are passed base64-encoded —
 * e.g. straight from `readSiteFile`, which already returns base64.
 * @category HTTP
 */
export interface MultipartFilePart {
  /** Form field name for this file (e.g. "0" per the GraphQL multipart spec). */
  field: string;
  /** File name reported in the part's Content-Disposition. */
  filename: string;
  /** MIME type for the part's Content-Type header. */
  contentType: string;
  /** File contents, base64-encoded. */
  contentBase64: string;
}

/**
 * Options for a multipart POST request.
 * @category HTTP
 */
export interface MultipartPostOptions {
  /** Timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Additional headers (Content-Type is set automatically and cannot be overridden) */
  headers?: Record<string, string>;
}

/**
 * Perform an HTTP POST with a `multipart/form-data` body.
 *
 * Unlike {@link httpPost} (JSON-only), this sends ordered text fields plus
 * binary file parts — enabling uploads to GraphQL `singleFileUpload`-style
 * endpoints. File bytes are passed base64-encoded (so they survive the IPC
 * boundary and can come directly from {@link readSiteFile}); moss builds the
 * multipart body, generates the boundary, and sets the Content-Type.
 *
 * @example
 * ```typescript
 * const res = await httpPostMultipart(endpoint, {
 *   textFields: [
 *     { name: "operations", value: JSON.stringify({ query, variables }) },
 *     { name: "map", value: JSON.stringify({ "0": ["variables.input.file"] }) },
 *   ],
 *   files: [{ field: "0", filename: "photo.jpg", contentType: "image/jpeg", contentBase64 }],
 * }, { headers: { "x-access-token": token } });
 * ```
 * @category HTTP
 */
export async function httpPostMultipart(
  url: string,
  parts: { textFields?: MultipartTextField[]; files?: MultipartFilePart[] },
  options: MultipartPostOptions = {}
): Promise<FetchResult> {
  const { timeoutMs = 30000, headers = {} } = options;

  const result = await getTauriCore().invoke<TauriFetchResult>(
    "http_post_multipart",
    {
      url,
      textFields: parts.textFields ?? [],
      files: parts.files ?? [],
      headers,
      timeoutMs,
    }
  );

  const binaryString = atob(result.body_base64);
  const bytes = Uint8Array.from(binaryString, (char) => char.charCodeAt(0));

  return {
    status: result.status,
    ok: result.ok,
    contentType: result.content_type,
    body: bytes,
    text(): string {
      return new TextDecoder().decode(bytes);
    },
  };
}

/**
 * Download a URL and save directly to disk
 *
 * Downloads the file and writes it directly to disk without passing
 * the binary data through JavaScript. The filename is derived from
 * the URL, and file extension is inferred from Content-Type if needed.
 *
 * Project path is auto-detected from the runtime context.
 *
 * @param url - URL to download
 * @param targetDir - Target directory within project (e.g., "assets")
 * @param options - Optional download configuration
 * @returns Download result with actual path where file was saved
 * @throws Error if download or write fails, or called outside a hook
 *
 * @example
 * ```typescript
 * const result = await downloadAsset(
 *   "https://example.com/image",
 *   "assets"
 * );
 * if (result.ok) {
 *   console.log(`Saved to ${result.actualPath}`); // e.g., "assets/image.png"
 * }
 * ```
 * @category HTTP
 */
export async function downloadAsset(
  url: string,
  targetDir: string,
  options: DownloadOptions = {}
): Promise<DownloadResult> {
  const ctx = getInternalContext();
  const { timeoutMs = 30000 } = options;

  const result = await getTauriCore().invoke<TauriDownloadResult>(
    "download_asset",
    {
      url,
      projectPath: ctx.project_path,
      targetDir,
      timeoutMs,
    }
  );

  return {
    status: result.status,
    ok: result.ok,
    contentType: result.content_type,
    bytesWritten: result.bytes_written,
    actualPath: result.actual_path,
  };
}
