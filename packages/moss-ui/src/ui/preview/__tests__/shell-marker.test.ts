/**
 * Tests for `shell-marker.ts` — the `__moss_shell` URL marker that
 * triggers server-side `moss-shell-frame` class injection (eliminating
 * the 48px chrome-clearance first-paint flash).
 *
 * See `packages/moss-ui/src/ui/preview/shell-marker.ts` for the design rationale and
 * `crates/moss-build/src/ops/serve/iframe_bridge.rs::SHELL_MARKER` for the server
 * counterpart.
 */
import { describe, it, expect } from "vitest";
import { withShellMarker, stripShellMarker, SHELL_MARKER_PARAM } from "../shell-marker";

describe("withShellMarker", () => {
  it("appends the marker to a URL with no query string", () => {
    const result = withShellMarker("http://localhost:1420/foo");
    expect(result).toBe("http://localhost:1420/foo?__moss_shell=1");
  });

  it("appends the marker to a URL with existing query params", () => {
    const result = withShellMarker("http://localhost:1420/foo?a=1&b=2");
    // URL.searchParams.set preserves existing params.
    expect(result).toMatch(/^http:\/\/localhost:1420\/foo\?/);
    expect(result).toContain("a=1");
    expect(result).toContain("b=2");
    expect(result).toContain("__moss_shell=1");
  });

  it("preserves the URL hash", () => {
    const result = withShellMarker("http://localhost:1420/foo#section");
    expect(result).toBe("http://localhost:1420/foo?__moss_shell=1#section");
  });

  it("is idempotent: applying twice does not duplicate the marker", () => {
    const once = withShellMarker("http://localhost:1420/foo");
    const twice = withShellMarker(once);
    expect(twice).toBe(once);
    // Verify only one occurrence.
    expect(twice.match(/__moss_shell=/g)?.length).toBe(1);
  });

  it("overwrites a stale marker value (e.g. from a different shell session)", () => {
    const result = withShellMarker("http://localhost:1420/foo?__moss_shell=stale");
    expect(result).toContain("__moss_shell=1");
    expect(result).not.toContain("__moss_shell=stale");
  });

  it("uses the constant for the param key", () => {
    expect(SHELL_MARKER_PARAM).toBe("__moss_shell");
    const result = withShellMarker("http://localhost:1420/foo");
    expect(result).toContain(`${SHELL_MARKER_PARAM}=1`);
  });
});

describe("withShellMarker — production output shape", () => {
  // These tests use the real WHATWG URL class (vitest's jsdom env provides
  // it) to lock in production behavior. The preview-manager.test.ts file
  // has a partial URL mock that produces `?_t=...?__moss_shell=1` (double
  // `?`) due to incomplete query-string parsing in the mock; if those
  // mock-shaped assertions ever drift to match real behavior accidentally,
  // the test below would fail and surface the mock divergence.

  it("appends marker as `&` when other query params already present", () => {
    // The bug case: cache-busting param `_t=N` is set first, then
    // withShellMarker should APPEND `&__moss_shell=1`, not start a second
    // query string with `?`. The preview-manager.test.ts mock incorrectly
    // produces `?_t=...?__moss_shell=1`; this test asserts the real shape.
    const cacheBusted = "http://localhost:3000/foo?_t=12345";
    const result = withShellMarker(cacheBusted);
    expect(result).toBe("http://localhost:3000/foo?_t=12345&__moss_shell=1");
    // Defense-in-depth: ensure no double `?` snuck in.
    expect(result.match(/\?/g)?.length).toBe(1);
  });

  it("appends marker correctly with multiple existing query params", () => {
    const url = "http://localhost:3000/foo?a=1&b=2&_t=12345";
    const result = withShellMarker(url);
    expect(result).toBe("http://localhost:3000/foo?a=1&b=2&_t=12345&__moss_shell=1");
    expect(result.match(/\?/g)?.length).toBe(1);
  });
});

describe("stripShellMarker", () => {
  it("removes the marker from a URL that has it", () => {
    const result = stripShellMarker("http://localhost:1420/foo?__moss_shell=1");
    expect(result).toBe("http://localhost:1420/foo");
  });

  it("preserves other query params when removing the marker", () => {
    const result = stripShellMarker(
      "http://localhost:1420/foo?a=1&__moss_shell=1&b=2"
    );
    // Order may shift when params are re-serialized; check semantically.
    expect(result).toContain("a=1");
    expect(result).toContain("b=2");
    expect(result).not.toContain("__moss_shell");
  });

  it("preserves the hash when removing the marker", () => {
    const result = stripShellMarker("http://localhost:1420/foo?__moss_shell=1#sec");
    expect(result).toBe("http://localhost:1420/foo#sec");
  });

  it("returns the URL unchanged if the marker is absent", () => {
    const url = "http://localhost:1420/foo?a=1";
    expect(stripShellMarker(url)).toBe(url);
  });

  it("round-trips with withShellMarker", () => {
    const original = "http://localhost:1420/posts/hello?lang=zh#anchor";
    const round = stripShellMarker(withShellMarker(original));
    expect(round).toBe(original);
  });
});
