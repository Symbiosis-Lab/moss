/**
 * The toast surface every window mounts (here the editor panel's "action-panel"
 * label): `mountToasts` listens for its own drain ping and dismiss event, drains
 * its own queue at mount, and `renderQueued` draws each ShowToast/ShowToastUpdate
 * it takes. Rendering is tested through the queue because that is the only way
 * a toast reaches the screen.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";

const { pending, plain } = vi.hoisted(() => ({
  pending: [] as unknown[],
  plain: new Map<string, Function[]>(),
}));

const take = () => Promise.resolve(pending.splice(0, pending.length) as { kind: string; payload?: unknown }[]);
const listen = (name: string, handler: Function) => {
  plain.set(name, [...(plain.get(name) ?? []), handler]);
  return Promise.resolve(() => {});
};

import { mountToasts, renderQueued, sendLogTicketId } from "../app-toast-render";
import * as toastManager from "../toast-manager";

const TOAST = { what: "Logs sent", severity: "ShippedDegraded" as const, ack: true, actions: null, duration: null, persistent: false, dismissible: true, id: null };
const FAILED = {
  ...TOAST, what: "Couldn't check for updates.", severity: "NeedsAction" as const, ack: false, id: "update-check",
  actions: [{ label: "Retry", event: "update:check" }],
};
const onAction = vi.fn();
const surface = { label: "action-panel" as const, listen: listen as never, take, onAction };
const fire = (name: string, payload: unknown = null) => {
  for (const cb of plain.get(name) ?? []) cb(payload);
};

describe("app-toast: the action-panel toast surface", () => {
  let container: HTMLElement;
  beforeEach(() => {
    pending.length = 0;
    plain.clear();
    onAction.mockClear();
    container = document.createElement("div");
    container.id = "moss-toast-container";
    document.body.appendChild(container);
  });
  afterEach(() => {
    toastManager._clearTrackedToasts();
    container.remove();
  });

  test("renders a queued toast when its own ping fires", async () => {
    await mountToasts(surface);
    pending.push({ kind: "ShowToast", payload: TOAST });
    fire("app-feedback-pending-action-panel");
    await vi.waitFor(() => {
      expect(container.querySelectorAll(".moss-toast").length).toBe(1);
    });
  });

  // Rust keeps one queue per surface, so what is waiting here is this panel's
  // own: a ping that landed while the webview was booting or reloading.
  test("drains its own queue at mount, so a ping that beat the mount is not lost", async () => {
    pending.push({ kind: "ShowToast", payload: TOAST });
    await mountToasts(surface);
    await vi.waitFor(() => {
      expect(container.querySelectorAll(".moss-toast").length).toBe(1);
    });
  });

  // The update offer: one button is the thing to do, the other a way out, and
  // the download it starts shows a bar that the next step takes away again.
  test("an offer's buttons carry their weight, and its download draws a bar", async () => {
    const offer = {
      ...TOAST, ack: false, id: "update-check", persistent: true,
      actions: [
        { label: "Update & Restart", event: "update:install", variant: "primary" as const },
        { label: "Release notes", event: "open-url:https://example.com/notes", variant: "secondary" as const },
      ],
    };
    renderQueued(surface, { kind: "ShowToast", payload: offer });
    expect(container.querySelector(".moss-toast-action--primary")?.textContent).toBe("Update & Restart");
    expect(container.querySelector(".moss-toast-action--secondary")).not.toBeNull();
    expect(container.querySelector(".moss-toast-progress")).toBeNull();

    (container.querySelector(".moss-toast-action--primary") as HTMLElement).click();
    expect(onAction).toHaveBeenCalledWith("update:install");

    renderQueued(surface, { kind: "ShowToast", payload: { ...offer, actions: null, progress: -1 } });
    expect(container.querySelector(".moss-toast-progress")).not.toBeNull();
    expect(container.querySelector(".moss-toast-action--primary")).toBeNull();

    renderQueued(surface, { kind: "ShowToastUpdate", payload: { ...offer, actions: null, progress: 40 } });
    expect((container.querySelector(".moss-progress-bar-fill") as HTMLElement).style.width).toBe("40%");
    expect(container.querySelectorAll(".moss-toast").length).toBe(1);

    renderQueued(surface, { kind: "ShowToastUpdate", payload: { ...offer, actions: null } });
    expect(container.querySelector(".moss-toast-progress")).toBeNull();
  });

  describe("a persistent toast stays until it is closed", () => {
    beforeEach(() => { vi.useFakeTimers(); });
    afterEach(() => { vi.useRealTimers(); });
    const offer = { ...FAILED, what: "moss v1.2.3 is ready to install.", severity: "ShippedDegraded" as const, persistent: true };

    test("an advisory, and an ack", () => {
      renderQueued(surface, { kind: "ShowToast", payload: offer });
      renderQueued(surface, { kind: "ShowToast", payload: { ...TOAST, id: "checking", persistent: true } });
      vi.advanceTimersByTime(60_000);
      expect(container.querySelectorAll(".moss-toast").length).toBe(2);
      expect(container.querySelector(".moss-toast-close")).not.toBeNull();
    });

    test("a patch that makes a toast persistent stops its countdown", () => {
      renderQueued(surface, { kind: "ShowToast", payload: { ...offer, persistent: false, duration: "8000" } });
      renderQueued(surface, { kind: "ShowToastUpdate", payload: offer });
      vi.advanceTimersByTime(60_000);
      expect(container.querySelectorAll(".moss-toast").length).toBe(1);
    });

    test("one that is not persistent still leaves", () => {
      renderQueued(surface, { kind: "ShowToast", payload: { ...offer, persistent: false } });
      vi.advanceTimersByTime(60_000);
      expect(container.querySelectorAll(".moss-toast").length).toBe(0);
    });
  });

  test("ignores queued events it cannot render", async () => {
    await mountToasts(surface);
    pending.push({ kind: "PublishReceipt", payload: { url: "https://example.com" } });
    fire("app-feedback-pending-action-panel");
    await new Promise((r) => setTimeout(r, 0));
    expect(container.querySelectorAll(".moss-toast").length).toBe(0);
  });

  test("listens for its own ping and the dismiss, and for no other surface's ping", async () => {
    await mountToasts(surface);
    expect([...plain.keys()]).toEqual(["app-feedback-pending-action-panel", "app-feedback-dismiss-action-panel"]);
  });

  // Rust moves a toast to the surface the user can see and tells the one it
  // left to drop its copy — otherwise a persistent spinner is stranded there.
  test("removes a toast when Rust says it has moved to another surface", async () => {
    await mountToasts(surface);
    pending.push({ kind: "ShowToast", payload: { ...TOAST, what: "Checking for updates…", id: "update-check", persistent: true } });
    fire("app-feedback-pending-action-panel");
    await vi.waitFor(() => expect(container.querySelector('[data-toast-id="update-check"]')).not.toBeNull());

    fire("app-feedback-dismiss-action-panel", { id: "update-check" });

    await vi.waitFor(() => expect(container.querySelector('[data-toast-id="update-check"]')).toBeNull());
  });

  test("a button carrying an action key sends that key back to Rust", async () => {
    await mountToasts(surface);
    pending.push({ kind: "ShowToast", payload: FAILED });
    fire("app-feedback-pending-action-panel");
    await vi.waitFor(() => expect(container.querySelector(".moss-toast-action")).not.toBeNull());

    (container.querySelector(".moss-toast-action") as HTMLElement).click();

    expect(onAction).toHaveBeenCalledExactlyOnceWith("update:check");
  });

  test("an update patches the toast it follows in place", async () => {
    await mountToasts(surface);
    pending.push(
      { kind: "ShowToast", payload: { ...TOAST, what: "Sending logs…", id: "send-logs", persistent: true } },
      { kind: "ShowToastUpdate", payload: { ...TOAST, what: "Logs sent: LOG-1", id: "send-logs" } },
    );
    fire("app-feedback-pending-action-panel");
    await vi.waitFor(() => {
      const toasts = container.querySelectorAll(".moss-toast");
      expect(toasts.length).toBe(1);
      expect(toasts[0].textContent).toContain("Logs sent: LOG-1");
    });
  });

  test("an update keeps the button's action key", () => {
    renderQueued(surface, { kind: "ShowToast", payload: FAILED });
    renderQueued(surface, { kind: "ShowToastUpdate", payload: { ...FAILED, what: "Couldn't check for updates — DNS lookup failed." } });

    expect(container.querySelector(".moss-toast-message")!.textContent).toContain("DNS lookup failed");
    (container.querySelector(".moss-toast-action") as HTMLElement).click();
    expect(onAction).toHaveBeenCalledExactlyOnceWith("update:check");
  });

  test("an action that names nothing to do gets no button", () => {
    renderQueued(surface, { kind: "ShowToast", payload: { ...TOAST, actions: [{ label: "Broken", event: "" }] } });

    expect(container.querySelector(".moss-toast-action")).toBeNull();
  });

  test("a link action opens its URL and never goes to Rust", () => {
    renderQueued(surface, { kind: "ShowToast", payload: { ...TOAST, actions: [{ label: "View", event: "open-url:https://example.com/" }] } });

    const link = container.querySelector(".moss-toast-action") as HTMLAnchorElement;
    expect(link.dataset.url).toBe("https://example.com/");
    expect(onAction).not.toHaveBeenCalled();
  });
});

describe("app-toast: sendLogTicketId", () => {
  test("extracts the ticket id from the send-log success message", () => {
    expect(sendLogTicketId("Logs sent: LOG-ABCD-0001")).toBe("LOG-ABCD-0001");
    expect(sendLogTicketId("Logs sent: LOG-XXXX-T5555")).toBe("LOG-XXXX-T5555");
  });

  test("returns undefined for a non-matching message", () => {
    expect(sendLogTicketId("Deploy succeeded")).toBeUndefined();
    expect(sendLogTicketId("")).toBeUndefined();
    // An older server reply carries no ticket id — nothing to copy.
    expect(sendLogTicketId("Logs sent")).toBeUndefined();
  });
});
