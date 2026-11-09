import { host } from "../host";
import { showToast } from "../shared/ui/toast/toast-manager";

interface HideMessage {
  type: "moss-hide-comment";
  source: string;
  id: string;
  pageKey: string;
  siteName: string;
}

function isHideMessage(d: unknown): d is HideMessage {
  if (typeof d !== "object" || d === null) return false;
  const m = d as Record<string, unknown>;
  return m.type === "moss-hide-comment" &&
    typeof m.source === "string" && typeof m.id === "string" &&
    typeof m.pageKey === "string" && typeof m.siteName === "string";
}

/**
 * Listen for Hide clicks posted from the preview iframe and write an
 * owner-signed hide event (reversible via the Undo toast). Preview-only;
 * the control is never present in deployed output. Returns an unlisten fn.
 */
export function initializeCommentControls(getFolderPath: () => string | null): () => void {
  const handler = async (event: MessageEvent) => {
    if (!isHideMessage(event.data)) return;
    // A host without comment moderation has no Hide to answer.
    const { hideComment, unhideComment } = host;
    if (!hideComment || !unhideComment) return;
    // Capture the folder at hide time; the Undo closure intentionally reuses it
    // (Undo should target the project where the hide happened). v1 limitation:
    // if the user switches projects within the toast window, Undo no-ops on the
    // old project — acceptable; documented in the spec.
    const folder = getFolderPath();
    if (!folder) {
      // A silent no-op here was the exact failure behind ticket LOG-DE4F: a Hide
      // click with no active folder did nothing and left no trace. console.warn
      // is forwarded into the log ring by install-log-bridge, so the next
      // user-initiated "Send Logs" report shows WHY the click did nothing.
      console.warn(
        `[comment] hide ignored: no active folder (source=${event.data.source} id=${event.data.id})`,
      );
      return;
    }
    const { source, id, pageKey, siteName } = event.data;

    try {
      const res = await hideComment(folder, source, id, pageKey, siteName);
      if (res.status === "error") {
        console.error(`[comment] hideComment failed (id=${id}): ${res.error}`);
        showToast({ what: `Couldn't hide comment: ${res.error}`, severity: "Blocking" });
        return;
      }
    } catch (e) {
      console.error(`[comment] hideComment threw (id=${id}): ${e}`);
      showToast({ what: `Couldn't hide comment: ${e}`, severity: "Blocking" });
      return;
    }

    showToast(
      {
        what: "Comment hidden",
        severity: "ShippedDegraded", // neutral/info tier
        action: {
          label: "Undo",
          onClick: async () => {
            try {
              const r = await unhideComment(folder, source, id, pageKey, siteName);
              if (r.status === "error") {
                console.error(`[comment] unhideComment failed (id=${id}): ${r.error}`);
                showToast({ what: `Couldn't undo: ${r.error}`, severity: "Blocking" });
              }
            } catch (e) {
              console.error(`[comment] unhideComment threw (id=${id}): ${e}`);
              showToast({ what: `Couldn't undo: ${e}`, severity: "Blocking" });
            }
          },
        },
      },
      { duration: 6000 },
    );
  };

  window.addEventListener("message", handler);
  return () => window.removeEventListener("message", handler);
}
