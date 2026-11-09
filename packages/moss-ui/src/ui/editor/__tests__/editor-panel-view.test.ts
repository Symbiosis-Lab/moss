/**
 * Tests for EditorPanelView — the PanelView wrapper around the editor webview.
 *
 * Verifies that mount/unmount call the correct Tauri commands and
 * that hasUnsavedChanges returns false (auto-save handles persistence).
 */

import { describe, test, expect, vi, beforeEach } from "vitest";
import { folderHomeTakeover } from '../session/node-class';

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const { mockOpenEditor, mockCloseEditorPanel } = vi.hoisted(() => ({
  mockOpenEditor: vi.fn().mockResolvedValue({ status: "ok", data: null }),
  mockCloseEditorPanel: vi.fn().mockResolvedValue({ status: "ok", data: null }),
}));

vi.mock("../../log", () => ({
  uiEvent: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Import after mocks
// ---------------------------------------------------------------------------
import { EditorPanelView } from "../editor-panel-view";
import { host } from "../../host";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("EditorPanelView", () => {
  let view: EditorPanelView;

  beforeEach(() => {
    vi.clearAllMocks();
    host.editorPanel = { open: mockOpenEditor, close: mockCloseEditorPanel };
    view = new EditorPanelView();
  });

  test("in a host with no editor panel, mount and unmount ask for none", async () => {
    const { host } = await import("../../host");
    const kept = host.editorPanel;
    host.editorPanel = undefined;
    try {
      expect(() => { view.mount(); view.unmount(); }).not.toThrow();
      expect(mockOpenEditor).not.toHaveBeenCalled();
    } finally {
      host.editorPanel = kept;
    }
  });

  test("mount calls openEditor with null/false/null when no node is set", () => {
    view.mount();
    expect(mockOpenEditor).toHaveBeenCalledWith(null, false, null);
  });

  test("mount passes the full resolved node to openEditor", () => {
    view.setNode({ sourcePath: "/path/to/index.md", isRoot: false, takeover: null });
    view.mount();
    expect(mockOpenEditor).toHaveBeenCalledWith("/path/to/index.md", false, null);
  });

  test("mount passes the takeover for a no-home folder node", () => {
    view.setNode({ sourcePath: null, isRoot: false, takeover: folderHomeTakeover("/abs/My Blog", "My Blog") });
    view.mount();
    expect(mockOpenEditor).toHaveBeenCalledWith(null, false, folderHomeTakeover("/abs/My Blog", "My Blog"));
  });

  test("mount passes isRoot=true for the onboarding root node", () => {
    view.setNode({ sourcePath: null, isRoot: true, takeover: null });
    view.mount();
    expect(mockOpenEditor).toHaveBeenCalledWith(null, true, null);
  });

  test("mount calls onMountError callback when openEditor rejects", async () => {
    const onMountError = vi.fn();
    const viewWithCallback = new EditorPanelView({ onMountError });
    mockOpenEditor.mockRejectedValueOnce(new Error("webview error"));

    viewWithCallback.mount();

    await vi.waitFor(() => {
      expect(onMountError).toHaveBeenCalledTimes(1);
      expect(onMountError).toHaveBeenCalledWith(expect.any(Error));
    });
  });

  test("unmount calls commands.closeEditorPanel (flush-then-hide, not destroy)", () => {
    view.unmount();
    expect(mockCloseEditorPanel).toHaveBeenCalledTimes(1);
  });

  test("hasUnsavedChanges returns false (auto-save handles persistence)", () => {
    expect(view.hasUnsavedChanges()).toBe(false);
  });

  test("mount handles openEditor rejection gracefully", async () => {
    mockOpenEditor.mockRejectedValueOnce(new Error("webview error"));

    // Should not throw
    view.mount();

    // Wait for the rejected promise to be handled
    await vi.waitFor(() => {
      expect(mockOpenEditor).toHaveBeenCalledTimes(1);
    });
  });

  test("unmount handles closeEditorPanel rejection gracefully", async () => {
    mockCloseEditorPanel.mockRejectedValueOnce(new Error("not open"));

    // Should not throw
    view.unmount();

    // Wait for the rejected promise to be handled
    await vi.waitFor(() => {
      expect(mockCloseEditorPanel).toHaveBeenCalledTimes(1);
    });
  });
});
