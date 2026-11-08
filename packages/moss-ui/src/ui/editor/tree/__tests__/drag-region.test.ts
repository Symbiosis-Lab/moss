/**
 * The breadcrumb strip's bare area (to the right of the segments, freed now
 * that the leaf no longer absorbs trailing surplus) is a window drag handle:
 * grabbing the empty region moves the window. Allowlist model — the strip is
 * marked with `data-tauri-drag-region`; the clickable segments are their own
 * mousedown targets and never drag.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { installPointerEventPolyfill } from "./test-helpers";

import { DRAG_REGION_ATTR } from "../../../window-drag";

installPointerEventPolyfill();

const { mockListTree, mockGetProjectPath } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: vi.fn(),
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: "ok", data: "/" }),
  },
}));


import { FileTree } from "../controller";
import { EntryRegistry } from "../../entry-registry";
import { dir, file } from './test-helpers';

const ROOT = "/Users/me/Sites/blog";

function makeTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
    ], { name: "blog" });
}

describe("breadcrumb strip drag region", () => {
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: "ok", data: ROOT });
    mockListTree.mockResolvedValue({ status: "ok", data: makeTree() });

    const registry = new EntryRegistry();
    const fileTree = new FileTree(registry);
    container = document.createElement("div");
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
  });

  it("marks .nav-bc-strip as a window drag region", () => {
    const strip = container.querySelector(".nav-bc-strip");
    expect(strip).not.toBeNull();
    expect(strip!.hasAttribute(DRAG_REGION_ATTR)).toBe(true);
  });

  it("does NOT mark the breadcrumb segments — they are click targets, not drag handles", () => {
    // Allowlist relies on Tauri's exact-target match: the strip drags, but its
    // segments (which navigate on click) must be their OWN unmarked targets so
    // clicking a crumb never triggers a window drag. Assert the jsdom-testable
    // half of that contract (the real drag behavior is an e2e concern).
    const segs = container.querySelectorAll(".nav-bc-strip .seg.bc");
    expect(segs.length).toBeGreaterThan(0);
    segs.forEach((seg) =>
      expect(seg.hasAttribute(DRAG_REGION_ATTR)).toBe(false),
    );
  });
});
