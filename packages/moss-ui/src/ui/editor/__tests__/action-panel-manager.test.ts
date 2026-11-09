/**
 * Tests for ActionPanelManager — the one-view-at-a-time manager
 * for the action panel.
 *
 * Uses plain PanelView stubs (not EditorPanelView) so the tests
 * verify manager behavior without depending on Tauri commands.
 */

import { describe, test, expect, vi, beforeEach } from "vitest";

vi.mock("../../log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
  uiEvent: vi.fn(),
}));

import { ActionPanelManager } from "../action-panel-manager";
import type { PanelView } from "../panel-view";
import { uiEvent as mockedUiEvent } from "../../log";

/**
 * The unsaved-changes guard now confirms via a moss alert (moss-alert.ts) —
 * a real DOM modal, not the Tauri dialog plugin — so these tests drive it by
 * clicking its buttons rather than mocking a plugin call. `mossConfirm()`'s
 * executor opens the modal SYNCHRONOUSLY before the outer Promise is
 * returned, so the button is on screen by the time `switchTo`/`close`'s
 * returned promise is captured, even though nothing has been awaited yet.
 */
function clickModalAction(action: "confirm" | "cancel"): void {
  const btn = document.querySelector<HTMLButtonElement>(
    `.moss-modal-actions [data-action="${action}"]`,
  );
  if (!btn) throw new Error(`no moss alert action="${action}" button on screen`);
  btn.click();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Create a stub PanelView with vi.fn() mock functions. */
function createView(): PanelView & {
  mount: ReturnType<typeof vi.fn>;
  unmount: ReturnType<typeof vi.fn>;
  hasUnsavedChanges: ReturnType<typeof vi.fn>;
} {
  const view = {
    mount: vi.fn(),
    unmount: vi.fn(),
    hasUnsavedChanges: vi.fn().mockReturnValue(false),
  };
  return view;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("ActionPanelManager", () => {
  let manager: ActionPanelManager;

  beforeEach(() => {
    manager = new ActionPanelManager();
    document.body.innerHTML = "";
  });

  // --- getCurrentView ---

  test("getCurrentView returns null initially", async () => {
    expect(manager.getCurrentView()).toBeNull();
  });

  // --- switchTo ---

  test("switchTo mounts view and returns true", async () => {
    const view = createView();
    const result = await manager.switchTo(view);

    expect(result).toBe(true);
    expect(view.mount).toHaveBeenCalledTimes(1);
    expect(view.mount).toHaveBeenCalledTimes(1);
    expect(manager.getCurrentView()).toBe(view);
  });

  test("switchTo same view is a no-op and returns true", async () => {
    const view = createView();
    await manager.switchTo(view);
    view.mount.mockClear();

    const result = await manager.switchTo(view);

    expect(result).toBe(true);
    expect(view.mount).not.toHaveBeenCalled();
    expect(view.unmount).not.toHaveBeenCalled();
  });

  test("switchTo unmounts current view before mounting new one", async () => {
    const viewA = createView();
    const viewB = createView();
    await manager.switchTo(viewA);

    const callOrder: string[] = [];
    viewA.unmount.mockImplementation(() => callOrder.push("unmountA"));
    viewB.mount.mockImplementation(() => callOrder.push("mountB"));

    await manager.switchTo(viewB);

    expect(callOrder).toEqual(["unmountA", "mountB"]);
    expect(manager.getCurrentView()).toBe(viewB);
  });

  test("switchTo with unsaved changes prompts user", async () => {
    const viewA = createView();
    viewA.hasUnsavedChanges.mockReturnValue(true);
    await manager.switchTo(viewA);

    const viewB = createView();
    const resultPromise = manager.switchTo(viewB);

    const title = document.querySelector(".moss-modal-title");
    expect(title?.textContent).toBe("You have unsaved changes. Discard and switch?");
    // User confirms discard.
    clickModalAction("confirm");
    const result = await resultPromise;

    expect(result).toBe(true);
    expect(viewA.unmount).toHaveBeenCalled();
    expect(viewB.mount).toHaveBeenCalledTimes(1);
    expect(manager.getCurrentView()).toBe(viewB);
  });

  test("switchTo cancelled when user declines unsaved changes prompt", async () => {
    const viewA = createView();
    viewA.hasUnsavedChanges.mockReturnValue(true);
    await manager.switchTo(viewA);
    vi.mocked(mockedUiEvent).mockClear();

    const viewB = createView();
    const resultPromise = manager.switchTo(viewB);
    // User cancels.
    clickModalAction("cancel");
    const result = await resultPromise;

    expect(result).toBe(false);
    expect(viewA.unmount).not.toHaveBeenCalled();
    expect(viewB.mount).not.toHaveBeenCalled();
    expect(manager.getCurrentView()).toBe(viewA);
    // The decline breadcrumb: a switch a user asked for that
    // silently goes nowhere must leave a trace of why, not just a `false`
    // return value a Send Logs bundle can't see.
    expect(vi.mocked(mockedUiEvent)).toHaveBeenCalledWith(
      "panel.switch-refused",
      expect.objectContaining({ reason: "unsaved-changes" }),
    );
  });

  test("switchTo does not prompt when hasUnsavedChanges is not implemented", async () => {
    const viewA: PanelView = {
      mount: vi.fn(),
      unmount: vi.fn(),
      // hasUnsavedChanges intentionally omitted
    };
    await manager.switchTo(viewA);

    const viewB = createView();
    const result = await manager.switchTo(viewB);

    expect(result).toBe(true);
    expect(document.querySelector(".moss-modal-title")).toBeNull();
  });

  test("switchTo does not prompt when hasUnsavedChanges returns false", async () => {
    const viewA = createView();
    viewA.hasUnsavedChanges.mockReturnValue(false);
    await manager.switchTo(viewA);

    const viewB = createView();
    await manager.switchTo(viewB);

    expect(document.querySelector(".moss-modal-title")).toBeNull();
  });

  // --- close ---

  test("close unmounts current view and returns true", async () => {
    const view = createView();
    await manager.switchTo(view);

    const result = await manager.close();

    expect(result).toBe(true);
    expect(view.unmount).toHaveBeenCalledTimes(1);
    expect(manager.getCurrentView()).toBeNull();
  });

  test("close returns true when no view is active", async () => {
    const result = await manager.close();
    expect(result).toBe(true);
    expect(manager.getCurrentView()).toBeNull();
  });

  test("close with unsaved changes prompts user and proceeds on confirm", async () => {
    const view = createView();
    view.hasUnsavedChanges.mockReturnValue(true);
    await manager.switchTo(view);

    const resultPromise = manager.close();
    expect(document.querySelector(".moss-modal-title")?.textContent).toBe(
      "You have unsaved changes. Discard and close?",
    );
    clickModalAction("confirm");
    const result = await resultPromise;

    expect(result).toBe(true);
    expect(view.unmount).toHaveBeenCalled();
    expect(manager.getCurrentView()).toBeNull();
  });

  test("close cancelled when user declines unsaved changes prompt", async () => {
    const view = createView();
    view.hasUnsavedChanges.mockReturnValue(true);
    await manager.switchTo(view);

    const resultPromise = manager.close();
    clickModalAction("cancel");
    const result = await resultPromise;

    expect(result).toBe(false);
    expect(view.unmount).not.toHaveBeenCalled();
    expect(manager.getCurrentView()).toBe(view);
  });

  // --- clearCurrentView ---

  test("clearCurrentView sets view to null without calling unmount", async () => {
    const view = createView();
    await manager.switchTo(view);
    view.unmount.mockClear();

    manager.clearCurrentView();

    expect(view.unmount).not.toHaveBeenCalled();
    expect(manager.getCurrentView()).toBeNull();
  });

  // --- setCurrentView ---

  test("setCurrentView restores the mirror without calling mount", async () => {
    const view = createView();
    // Simulate "webview opened outside switchTo" — the view was cleared by a
    // stale ActionPanelClosed but the webview is actually live.
    manager.setCurrentView(view);

    expect(view.mount).not.toHaveBeenCalled();
    expect(manager.getCurrentView()).toBe(view);
  });

  test("setCurrentView re-syncs after clearCurrentView so gates pass again", async () => {
    const view = createView();
    await manager.switchTo(view);
    // External ActionPanelClosed clears the mirror even though the editor
    // webview is about to be navigated/reused.
    manager.clearCurrentView();
    expect(manager.getCurrentView()).toBeNull();

    // ActionPanelOpened restores it — no extra mount, no churn.
    view.mount.mockClear();
    manager.setCurrentView(view);
    expect(manager.getCurrentView()).toBe(view);
    expect(view.mount).not.toHaveBeenCalled();
  });

  test("setCurrentView is idempotent when already current", async () => {
    const view = createView();
    await manager.switchTo(view);
    view.mount.mockClear();
    view.unmount.mockClear();

    manager.setCurrentView(view);

    expect(view.mount).not.toHaveBeenCalled();
    expect(view.unmount).not.toHaveBeenCalled();
    expect(manager.getCurrentView()).toBe(view);
  });

  // --- Sequential operations ---

  test("close then switchTo works correctly", async () => {
    const viewA = createView();
    await manager.switchTo(viewA);
    expect(await manager.close()).toBe(true);

    const viewB = createView();
    await manager.switchTo(viewB);

    expect(viewB.mount).toHaveBeenCalledTimes(1);
    expect(manager.getCurrentView()).toBe(viewB);
  });

  test("clearCurrentView then switchTo does not unmount the old view", async () => {
    const viewA = createView();
    await manager.switchTo(viewA);
    viewA.unmount.mockClear();

    manager.clearCurrentView();

    const viewB = createView();
    await manager.switchTo(viewB);

    // viewA.unmount should NOT have been called again (it was already cleared)
    expect(viewA.unmount).not.toHaveBeenCalled();
    expect(viewB.mount).toHaveBeenCalledTimes(1);
  });

  // --- view-changed logging ---

  test("switchTo logs the view change with from/to names", async () => {
    class ViewA implements PanelView {
      mount = vi.fn();
      unmount = vi.fn();
    }
    class ViewB implements PanelView {
      mount = vi.fn();
      unmount = vi.fn();
    }
    const viewA = new ViewA();
    const viewB = new ViewB();

    await manager.switchTo(viewA);
    expect(vi.mocked(mockedUiEvent)).toHaveBeenCalledWith(
      "panel.switch",
      expect.objectContaining({ from: "none", to: "ViewA" }),
    );

    vi.mocked(mockedUiEvent).mockClear();
    await manager.switchTo(viewB);
    expect(vi.mocked(mockedUiEvent)).toHaveBeenCalledWith(
      "panel.switch",
      expect.objectContaining({ from: "ViewA", to: "ViewB" }),
    );
  });

  test("switchTo to the same view does not log a redundant view change", async () => {
    const view = createView();
    await manager.switchTo(view);
    vi.mocked(mockedUiEvent).mockClear();

    await manager.switchTo(view);

    expect(vi.mocked(mockedUiEvent)).not.toHaveBeenCalled();
  });

  test("multiple switches maintain single active view", async () => {
    const viewA = createView();
    const viewB = createView();
    const viewC = createView();

    await manager.switchTo(viewA);
    await manager.switchTo(viewB);
    await manager.switchTo(viewC);

    expect(manager.getCurrentView()).toBe(viewC);
    expect(viewA.unmount).toHaveBeenCalledTimes(1);
    expect(viewB.unmount).toHaveBeenCalledTimes(1);
    expect(viewC.unmount).not.toHaveBeenCalled();
  });
});


describe("ActionPanelManager — views that flush before their webview is destroyed", () => {
  function flushingView(order: string[], flushDone: Promise<void>): PanelView {
    return {
      mount: vi.fn(),
      unmount: vi.fn(() => order.push("unmount")),
      flush: vi.fn(async () => {
        order.push("flush-start");
        await flushDone;
        order.push("flush-done");
      }),
    };
  }

  test("switchTo awaits the current view's flush before unmounting it", async () => {
    const order: string[] = [];
    let release!: () => void;
    const manager = new ActionPanelManager();
    await manager.switchTo(flushingView(order, new Promise<void>((r) => { release = r; })));
    const next = createView();
    const switching = manager.switchTo(next);
    await Promise.resolve();
    await Promise.resolve();
    expect(order).toEqual(["flush-start"]); // webview still alive
    release();
    await switching;
    expect(order).toEqual(["flush-start", "flush-done", "unmount"]);
    expect(next.mount).toHaveBeenCalled();
  });

  test("close awaits the current view's flush before unmounting it", async () => {
    const order: string[] = [];
    const manager = new ActionPanelManager();
    await manager.switchTo(flushingView(order, Promise.resolve()));
    await manager.close();
    expect(order).toEqual(["flush-start", "flush-done", "unmount"]);
  });

  test("a flush that rejects does not trap the user in the panel", async () => {
    const view: PanelView = {
      mount: vi.fn(),
      unmount: vi.fn(),
      flush: vi.fn(async () => { throw new Error("webview gone"); }),
    };
    const manager = new ActionPanelManager();
    await manager.switchTo(view);
    await expect(manager.close()).resolves.toBe(true);
    expect(view.unmount).toHaveBeenCalled();
  });
});
