import {
  eventBus,
  Events,
  type OpenSettingsModalEvent,
} from "../context/event-bus";
import type { AppOp } from "@symbiosis-lab/moss-ui";
import { host } from "../host";
import { t } from "../i18n/index";
import {
  removeToastById,
  showAck,
  showOrUpdateToast,
  showToast,
} from "../shared/ui/toast/toast-manager";

/**
 * Dispatch an `Action::InApp` operation to its effect.
 *
 * A Job's `Awaiting` interrupt (or an advisory) carries
 * `Action::InApp{op, args, label}`. The UI names the *intent* and hands it back
 * here — this module owns the `op -> effect` table. A navigation op opens a
 * surface here; one that needs work done asks the host, and a host without
 * that feature leaves the op inert.
 */
export function dispatchAppOp(op: AppOp, args: unknown): void {
  switch (op) {
    case "SignIn": {
      // A domain email-verify 403 opens Settings at the DEPLOYMENT section, where the
      // verify-email flow and the api-key input live. "email" is the Subscribers tab
      // (newsletter list), not that UI. Both verify_email and a plain SignIn (no reason)
      // land in "deployment", the DNS / domain section that hosts the email-verify form.
      // Section ids are registered in app.ts.
      const section = "deployment";
      eventBus.emit<OpenSettingsModalEvent>(Events.OPEN_SETTINGS_MODAL, {
        section,
      });
      break;
    }
    case "OpenBilling": {
      // No dedicated billing section registered yet — open Settings (default
      // section). Wire to a real id when a producer emits OpenBilling.
      eventBus.emit<OpenSettingsModalEvent>(Events.OPEN_SETTINGS_MODAL, {});
      break;
    }
    case "DownloadTool": {
      const tool = (args as { tool?: unknown } | null)?.tool;
      const download = host.downloadTool;
      if (typeof tool === "string" && download) void downloadTool(download, tool);
      break;
    }
    case "MoveFile":
    case "RecheckDns":
      // No producer emits these into the panel yet.
      // Kept as explicit arms so adding a producer is a deliberate edit here.
      break;
  }
}

/**
 * Download a tool the user asked for, then ask for a rebuild of the open folder
 * so the build finds it. The toast id is per tool, so a second click replaces
 * the first progress line instead of stacking.
 */
const TOOL_DISPLAY_NAMES: Record<string, string> = { ffmpeg: "FFmpeg", git: "Git" };

async function downloadTool(download: NonNullable<typeof host.downloadTool>, tool: string): Promise<void> {
  const name = TOOL_DISPLAY_NAMES[tool] ?? tool;
  const toastId = `download-tool:${tool}`;
  showOrUpdateToast(t("download_tool.downloading", { name }), { id: toastId, persistent: true });
  const result = await download(tool);
  removeToastById(toastId);
  if (result.status === "error") {
    showToast({
      what: t("download_tool.failed", { name, error: result.error }),
      severity: "Blocking",
    });
    return;
  }
  showAck(t("download_tool.done", { name }));
  // The GitHub plugin's own poll picks git up, so only other tools need a rebuild.
  if (tool !== "git") eventBus.emit(Events.REBUILD_OPEN_FOLDER, {});
}
