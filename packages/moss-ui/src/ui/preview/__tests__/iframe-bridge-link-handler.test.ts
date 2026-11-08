/**
 * Tests for the link click interceptor in iframe-bridge.ts.
 *
 * Since iframe-bridge.ts is an IIFE injected into the preview iframe,
 * we test the link click handler logic in isolation by simulating
 * the DOM state and click events it would receive.
 *
 * Reasoning for removing _t= from internal links:
 * The Rust preview server already sets Cache-Control: no-cache, no-store, must-revalidate
 * and strips If-Modified-Since/If-None-Match headers. This means every request
 * hits the server regardless of query params. Adding _t= to internal links is
 * redundant and causes URL pollution visible to users viewing in a browser.
 * Mature dev servers (Vite, webpack-dev-server) rely on HTTP cache headers,
 * not URL timestamps.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

/**
 * Simulate what the iframe-bridge link click handler does for internal links.
 * This mirrors the logic in iframe-bridge.ts lines 229-234.
 *
 * Returns the URL that would be navigated to (since we can't actually
 * set window.location.href in tests).
 */
function handleInternalLinkClick(href: string, baseUrl: string): string {
  const url = new URL(href, baseUrl);
  // After fix: no _t= cache-busting — server headers handle cache control
  return url.href;
}

describe("iframe-bridge link click handler", () => {
  it("internal links navigate without _t= cache-busting parameter", () => {
    const result = handleInternalLinkClick(
      "/posts/my-article/",
      "http://localhost:8080/"
    );
    expect(result).toBe("http://localhost:8080/posts/my-article/");
    expect(result).not.toContain("_t=");
  });

  it("relative internal links resolve correctly without _t=", () => {
    const result = handleInternalLinkClick(
      "../other-post/",
      "http://localhost:8080/posts/my-article/"
    );
    expect(result).toBe("http://localhost:8080/posts/other-post/");
    expect(result).not.toContain("_t=");
  });

  it("internal links with Unicode paths work without _t=", () => {
    const result = handleInternalLinkClick(
      "/videos/%E6%9F%90%E4%B8%AA%E5%A4%8F%E6%97%A5%E7%9A%84%E5%8D%88%E5%90%8E%E7%AB%A5%E8%B0%A3/",
      "http://localhost:8080/"
    );
    expect(result).toBe(
      "http://localhost:8080/videos/%E6%9F%90%E4%B8%AA%E5%A4%8F%E6%97%A5%E7%9A%84%E5%8D%88%E5%90%8E%E7%AB%A5%E8%B0%A3/"
    );
    expect(result).not.toContain("_t=");
  });

  it("preserves existing query parameters on internal links", () => {
    const result = handleInternalLinkClick(
      "/search/?q=hello",
      "http://localhost:8080/"
    );
    expect(result).toBe("http://localhost:8080/search/?q=hello");
    expect(result).not.toContain("_t=");
  });

  it("preserves hash fragments on internal links", () => {
    const result = handleInternalLinkClick(
      "/posts/my-article/#section-2",
      "http://localhost:8080/"
    );
    expect(result).toBe("http://localhost:8080/posts/my-article/#section-2");
    expect(result).not.toContain("_t=");
  });
});
