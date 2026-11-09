import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../shared/ui/toast/toast-manager", () => ({ showToast: vi.fn() }));

import { showToast } from "../../shared/ui/toast/toast-manager";
import { initializeCommentControls } from "../comment-owner-controls";
import { host } from "../../host";

// The host's comment actions: the code under test reads them when it initializes.
const commands = {
  hideComment: vi.fn(),
  unhideComment: vi.fn(),
};
host.hideComment = commands.hideComment as typeof host.hideComment;
host.unhideComment = commands.unhideComment as typeof host.unhideComment;

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("comment owner controls", () => {
  let teardown: (() => void) | undefined;
  beforeEach(() => {
    vi.clearAllMocks();
    commands.hideComment.mockResolvedValue({ status: "ok", data: null });
    commands.unhideComment.mockResolvedValue({ status: "ok", data: null });
  });
  afterEach(() => { teardown?.(); teardown = undefined; }); // remove the listener so tests don't stack

  it("hides a comment and shows an Undo toast on moss-hide-comment", async () => {
    teardown = initializeCommentControls(() => "/my/folder");
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-hide-comment", source: "matters", id: "m1", pageKey: "uid", siteName: "site" },
    }));
    await flush();
    expect(commands.hideComment).toHaveBeenCalledWith("/my/folder", "matters", "m1", "uid", "site");
    expect(showToast).toHaveBeenCalledTimes(1);
  });

  it("Undo action calls unhideComment with the same args", async () => {
    teardown = initializeCommentControls(() => "/my/folder");
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-hide-comment", source: "artalk", id: "7", pageKey: "uid", siteName: "site" },
    }));
    await flush();
    // pull the Undo action out of the showToast call and invoke it
    const advisory = (showToast as unknown as { mock: { calls: any[][] } }).mock.calls[0][0];
    (commands.unhideComment as unknown as { mockResolvedValue: (v: unknown) => void })
      .mockResolvedValue({ status: "ok", data: null });
    await advisory.action.onClick();
    expect(commands.unhideComment).toHaveBeenCalledWith("/my/folder", "artalk", "7", "uid", "site");
  });

  it("shows a Blocking toast if hideComment returns a Rust Err (real binding path)", async () => {
    (commands.hideComment as unknown as { mockResolvedValueOnce: (v: unknown) => void })
      .mockResolvedValueOnce({ status: "error", error: "owner identity unavailable" });
    teardown = initializeCommentControls(() => "/my/folder");
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-hide-comment", source: "artalk", id: "1", pageKey: "p", siteName: "s" },
    }));
    await flush();
    // Exactly one toast, and it must NOT be the success toast.
    expect(showToast).toHaveBeenCalledTimes(1);
    const advisory = (showToast as unknown as { mock: { calls: any[][] } }).mock.calls[0][0];
    expect(advisory.severity).toBe("Blocking");
    expect(advisory.what).toContain("owner identity unavailable");
    expect(advisory.what).not.toContain("Comment hidden");
  });

  it("shows a Blocking toast on transport Error (ipc down)", async () => {
    (commands.hideComment as unknown as { mockRejectedValueOnce: (e: unknown) => void })
      .mockRejectedValueOnce(new Error("ipc down"));
    teardown = initializeCommentControls(() => "/my/folder");
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-hide-comment", source: "artalk", id: "1", pageKey: "p", siteName: "s" },
    }));
    await flush();
    const advisory = (showToast as unknown as { mock: { calls: any[][] } }).mock.calls[0][0];
    expect(advisory.severity).toBe("Blocking");
  });

  it("ignores unrelated messages", async () => {
    teardown = initializeCommentControls(() => "/my/folder");
    window.dispatchEvent(new MessageEvent("message", { data: { type: "something-else" } }));
    await flush();
    expect(commands.hideComment).not.toHaveBeenCalled();
  });

  it("no-ops when no folder is active, but logs why (forwarded to Send Logs)", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    teardown = initializeCommentControls(() => null);
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-hide-comment", source: "artalk", id: "1", pageKey: "p", siteName: "s" },
    }));
    await flush();
    expect(commands.hideComment).not.toHaveBeenCalled();
    // The previously-silent path now leaves a trace (LOG-DE4F regression guard).
    expect(warnSpy).toHaveBeenCalledTimes(1);
    expect(warnSpy.mock.calls[0][0]).toContain("no active folder");
    warnSpy.mockRestore();
  });
});
