import { describe, it, expect, vi, beforeEach } from "vitest";
import { eventBus, Events } from "../../context/event-bus";
import { dispatchAppOp } from "../app-op-dispatch";
import { host } from "../../host";
import { showAck, showOrUpdateToast, showToast } from "../../shared/ui/toast/toast-manager";

vi.mock("../../shared/ui/toast/toast-manager", () => ({
  showAck: vi.fn(),
  showToast: vi.fn(),
  showOrUpdateToast: vi.fn(),
  removeToastById: vi.fn(),
}));

describe("dispatchAppOp (Action::InApp op -> effect)", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("SignIn with verify_email reason opens Settings at the DEPLOYMENT section", () => {
    // "deployment" is the registered section id where the email verify-email UI
    // lives (the email-verify form, api-key input, email status indicator).
    // "email" selects the newsletter Subscribers tab, which cannot complete sign-in.
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("SignIn", { reason: "verify_email", domain: "example.test" });
    expect(emit).toHaveBeenCalledWith(
      Events.OPEN_SETTINGS_MODAL,
      expect.objectContaining({ section: "deployment" }),
    );
  });

  it("bare SignIn (no reason) also opens Settings at the DEPLOYMENT section", () => {
    // verify_email and a plain SignIn both fall through
    // to "deployment" unconditionally. The case arm has no branching, but the
    // the suite pins both.
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("SignIn", null);
    expect(emit).toHaveBeenCalledWith(
      Events.OPEN_SETTINGS_MODAL,
      expect.objectContaining({ section: "deployment" }),
    );
  });

  it("OpenBilling opens Settings", () => {
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("OpenBilling", null);
    expect(emit).toHaveBeenCalledWith(
      Events.OPEN_SETTINGS_MODAL,
      expect.any(Object),
    );
  });

  it("unwired ops (MoveFile/RecheckDns) are a no-op, not a throw", () => {
    const emit = vi.spyOn(eventBus, "emit");
    expect(() => dispatchAppOp("MoveFile", null)).not.toThrow();
    expect(() => dispatchAppOp("RecheckDns", null)).not.toThrow();
    expect(emit).not.toHaveBeenCalled();
  });
});

describe("DownloadTool (the advisory's Download button)", () => {
  const downloadTool = vi.fn();
  beforeEach(() => {
    vi.clearAllMocks();
    host.downloadTool = downloadTool;
  });

  it("downloads the tool and asks for a rebuild of the open folder on ok", async () => {
    downloadTool.mockResolvedValue({ status: "ok", data: null });
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("DownloadTool", { tool: "ffmpeg" });
    await vi.waitFor(() =>
      expect(emit).toHaveBeenCalledWith(Events.REBUILD_OPEN_FOLDER, expect.anything()),
    );
    expect(downloadTool).toHaveBeenCalledWith("ffmpeg");
  });

  it("does not ask for a folder rebuild after a git download", async () => {
    downloadTool.mockResolvedValue({ status: "ok", data: null });
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("DownloadTool", { tool: "git" });
    await vi.waitFor(() => expect(showAck).toHaveBeenCalled());
    expect(emit.mock.calls.some(([e]) => e === Events.REBUILD_OPEN_FOLDER)).toBe(false);
  });

  it("shows the error and does not rebuild when the download fails", async () => {
    downloadTool.mockResolvedValue({
      status: "error",
      error: "network unreachable",
    });
    const emit = vi.spyOn(eventBus, "emit");
    dispatchAppOp("DownloadTool", { tool: "ffmpeg" });
    await vi.waitFor(() => expect(showToast).toHaveBeenCalled());
    expect(emit.mock.calls.some(([e]) => e === Events.REBUILD_OPEN_FOLDER)).toBe(false);
  });

  it("is inert in a host that cannot download tools", () => {
    delete host.downloadTool;
    dispatchAppOp("DownloadTool", { tool: "ffmpeg" });
    expect(showOrUpdateToast).not.toHaveBeenCalled();
  });

  it("ignores args without a string tool name", () => {
    dispatchAppOp("DownloadTool", { tool: 42 });
    dispatchAppOp("DownloadTool", null);
    expect(downloadTool).not.toHaveBeenCalled();
  });
});
