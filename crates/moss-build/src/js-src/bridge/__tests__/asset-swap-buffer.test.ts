import { describe, it, expect, vi } from "vitest";
import { createAssetSwapBuffer } from "../asset-swap-buffer";

describe("createAssetSwapBuffer", () => {
  it("applies immediately when the element already matches (not buffered)", () => {
    const apply = vi.fn().mockReturnValue(1); // 1 element matched
    const buf = createAssetSwapBuffer({ apply, currentUrl: () => "u1" });
    expect(buf.handle("image", "/a.webp")).toBe("applied");
    expect(buf.size()).toBe(0);
    expect(apply).toHaveBeenCalledWith("image", "/a.webp");
  });

  it("buffers a swap that matches nothing yet, then applies it on drain once the element appears", () => {
    let present = false;
    const apply = vi.fn((_t: string, _p: string) => (present ? 1 : 0));
    const buf = createAssetSwapBuffer({ apply, currentUrl: () => "u1" });

    // Arrives before the element exists → buffered, not applied.
    expect(buf.handle("image", "/a.webp")).toBe("buffered");
    expect(buf.size()).toBe(1);

    // Element still absent → drain keeps it buffered.
    buf.drain();
    expect(buf.size()).toBe(1);

    // Element appears → drain applies and evicts it.
    present = true;
    buf.drain();
    expect(buf.size()).toBe(0);
  });

  it("drops all buffered swaps when the document navigates to a different URL", () => {
    let url = "u1";
    const apply = vi.fn().mockReturnValue(0); // never matches → always buffers
    const buf = createAssetSwapBuffer({ apply, currentUrl: () => url });

    buf.handle("image", "/old.webp");
    expect(buf.size()).toBe(1);

    // Navigate away — a swap for the old page must not linger for the new one.
    url = "u2";
    buf.handle("image", "/new.webp"); // handle() checks nav before buffering
    expect(buf.size()).toBe(1); // old cleared, only the new one remains

    // Same via drain(): navigating clears stale buffered swaps.
    url = "u3";
    buf.drain();
    expect(buf.size()).toBe(0);
  });

  it("evicts a never-matching entry after maxDrainAttempts so size() can reach 0 (observer can disconnect)", () => {
    const apply = vi.fn().mockReturnValue(0); // never matches (e.g. OG card only in <meta>)
    const buf = createAssetSwapBuffer({ apply, currentUrl: () => "u1", maxDrainAttempts: 3 });

    buf.handle("image", "/og/home.png");
    expect(buf.size()).toBe(1);

    buf.drain(); // attempt 1
    buf.drain(); // attempt 2
    expect(buf.size()).toBe(1); // not yet given up
    buf.drain(); // attempt 3 → evict
    expect(buf.size()).toBe(0); // gave up → observer can now disconnect
  });

  it("dedups identical (assetType,path) and stays bounded under overflow", () => {
    const apply = vi.fn().mockReturnValue(0);
    const buf = createAssetSwapBuffer({ apply, currentUrl: () => "u1", maxPending: 3 });

    buf.handle("image", "/a.webp");
    buf.handle("image", "/a.webp"); // duplicate → no growth
    expect(buf.size()).toBe(1);

    buf.handle("image", "/b.webp");
    buf.handle("image", "/c.webp");
    buf.handle("image", "/d.webp"); // over the cap of 3 → dropped
    expect(buf.size()).toBe(3);
  });
});
