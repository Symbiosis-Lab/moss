/**
 * Tests for preview refresh visual feedback (Tier 1).
 *
 * When the user presses Cmd+R, the URL text in the titlebar should
 * briefly flash to confirm the refresh happened. This is especially
 * important for fast-loading local pages where the iframe reload
 * itself may not be visually noticeable.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";

// ---------------------------------------------------------------------------
// vi.hoisted() — variables referenced inside vi.mock() factories
// ---------------------------------------------------------------------------
const { mockNavigationManager } = vi.hoisted(() => ({
  mockNavigationManager: {
    getCurrentUrl: vi.fn().mockReturnValue("http://localhost:1420/"),
    getRequestRevision: vi.fn().mockReturnValue(0),
    beginNavigation: vi.fn(),
    initialize: vi.fn(),
    handleNavigationUpdate: vi.fn(),
    updateDisplay: vi.fn(),
    // With morphing enabled by default, refresh() morphs in place first, falling back to
    // reload() then cache-busting iframe.src. Mock both rungs of that ladder.
    morph: vi.fn().mockResolvedValue(true),
    reload: vi.fn().mockResolvedValue(true),
    isNavigationPending: vi.fn().mockReturnValue(false),
    reset: vi.fn(),
    setFolderPath: vi.fn(),
    isMobilePreview: vi.fn().mockReturnValue(false),
    // PreviewManager wires the rename 404-retry observer in its constructor.
    setNavigationResultListener: vi.fn(),
    setPreviewRequirementListener: vi.fn(),
    // PreviewManager wires the device-mode change listener (zoom re-apply).
    setDeviceModeChangeListener: vi.fn(),
  },
}));

// ---------------------------------------------------------------------------
// Mock modules
// ---------------------------------------------------------------------------
vi.mock("../preview/navigation-manager", () => ({
  NavigationManager: vi.fn().mockImplementation(() => mockNavigationManager),
}));

// The rebuild hairline normally defers the pill flash until its progress bar
// animation reaches full (rAF/transition-driven — jsdom won't run it). Mock it
// so complete(cb) invokes the flash callback synchronously.
vi.mock("../preview/preview-hairline", () => ({
  PreviewHairline: vi.fn().mockImplementation(() => ({
    cancelPendingDissolve: vi.fn(),
    dissolveAfter: vi.fn(),
    setBuildProgress: vi.fn(),
    complete: vi.fn((cb?: () => void) => cb?.()),
  })),
}));

// ---------------------------------------------------------------------------
// Import after mocks
// ---------------------------------------------------------------------------
import { PreviewManager } from "../preview/preview-manager";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function setupDOM(): void {
  document.body.innerHTML = `
    <div class="moss-titlebar" id="moss-titlebar">
      <div class="moss-nav-pill">
        <span class="moss-url-text" id="moss-url-text">/</span>
      </div>
    </div>
    <div class="moss-preview-container" id="moss-preview-container">
      <iframe id="moss-preview-iframe"></iframe>
      <div id="moss-preview-overlay"></div>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
describe("Preview refresh visual feedback", () => {
  let previewManager: PreviewManager;

  beforeEach(() => {
    setupDOM();
    vi.clearAllMocks();
    previewManager = new PreviewManager('moss-preview-iframe');
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  // Default morphing reports completion by flashing the whole nav pill (.moss-nav-pill)
  // that fires when the in-place update APPLIES (post-morph), not synchronously at
  // call time — so target the pill and await the refresh.
  test("refresh flashes the nav pill (refreshing class)", async () => {
    const pill = document.querySelector(".moss-nav-pill")!;
    expect(pill.classList.contains("refreshing")).toBe(false);

    await previewManager.refresh();

    expect(pill.classList.contains("refreshing")).toBe(true);
  });

  test("repeated refresh re-triggers the flash, however close together", async () => {
    const pill = document.querySelector(".moss-nav-pill")!;

    await previewManager.refresh();
    expect(pill.classList.contains("refreshing")).toBe(true);
    pill.classList.remove("refreshing"); // prove the next flash re-adds it

    await previewManager.refresh();
    expect(pill.classList.contains("refreshing")).toBe(true);
  });

  test("refresh does not throw when the nav pill is absent", async () => {
    document.querySelector(".moss-nav-pill")?.remove();

    // flashPill guards on a missing pill; refresh must still resolve cleanly.
    await expect(previewManager.refresh()).resolves.toBeUndefined();
  });
});

describe("Preview refresh scroll preservation", () => {
  let previewManager: PreviewManager;

  beforeEach(() => {
    setupDOM();
    vi.clearAllMocks();
    // Default: reload succeeds (RPC available)
    mockNavigationManager.reload.mockResolvedValue(true);
    previewManager = new PreviewManager('moss-preview-iframe');
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  // The in-place mechanism is now morph (default-ON); reload is the first
  // fallback, cache-busting iframe.src the last. Verify the whole ladder.
  test("refresh morphs in place (default) and does not reload or touch iframe.src", async () => {
    const iframe = document.getElementById("moss-preview-iframe") as HTMLIFrameElement;
    const originalSrc = iframe.src;

    await previewManager.refresh();

    expect(mockNavigationManager.morph).toHaveBeenCalled();
    expect(mockNavigationManager.reload).not.toHaveBeenCalled();
    expect(iframe.src).toBe(originalSrc);
  });

  test("refresh falls back to reload() when morph fails", async () => {
    mockNavigationManager.morph.mockResolvedValue(false);
    const iframe = document.getElementById("moss-preview-iframe") as HTMLIFrameElement;
    const originalSrc = iframe.src;

    await previewManager.refresh();

    expect(mockNavigationManager.reload).toHaveBeenCalled();
    expect(iframe.src).toBe(originalSrc); // reload succeeded → no iframe.src change
  });

  test("refresh falls back to cache-busting iframe.src when morph AND reload fail", async () => {
    mockNavigationManager.morph.mockResolvedValue(false);
    mockNavigationManager.reload.mockResolvedValue(false);

    const iframe = document.getElementById("moss-preview-iframe") as HTMLIFrameElement;

    await previewManager.refresh();

    if (mockNavigationManager.getCurrentUrl()) {
      expect(iframe.src).toContain("_t=");
    }
  });
});
