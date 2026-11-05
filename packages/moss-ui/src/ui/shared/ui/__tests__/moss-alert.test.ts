/**
 * Unit tests for moss-alert.ts — confirm and message dialogs drawn in the
 * webview, with the promise-returning shape of native ones, backed by a real
 * BaseModal/OutcomeModal DOM modal instead of the Tauri dialog plugin.
 */
import { describe, it, expect, afterEach } from "vitest";
import { mossConfirm, mossAlert } from "../moss-alert";

function clickAction(action: "confirm" | "cancel"): void {
  const btn = document.querySelector<HTMLButtonElement>(
    `.moss-modal-actions [data-action="${action}"]`,
  );
  if (!btn) throw new Error(`no [data-action="${action}"] button on screen`);
  btn.click();
}

function pressEscape(): void {
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
}

describe("mossConfirm", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("resolves true when the confirm button is clicked", async () => {
    const resultPromise = mossConfirm("Delete “Notes”?", { confirmLabel: "Delete" });
    clickAction("confirm");
    expect(await resultPromise).toBe(true);
  });

  it("resolves false when the cancel button is clicked", async () => {
    const resultPromise = mossConfirm("Delete “Notes”?", { confirmLabel: "Delete" });
    clickAction("cancel");
    expect(await resultPromise).toBe(false);
  });

  it("resolves false on Escape (same as Cancel)", async () => {
    const resultPromise = mossConfirm("Delete “Notes”?", { confirmLabel: "Delete" });
    pressEscape();
    expect(await resultPromise).toBe(false);
  });

  it("renders the message as the modal title and wires aria-labelledby", () => {
    void mossConfirm("Remove user@example.com?", { confirmLabel: "Remove" });
    const container = document.querySelector(".moss-modal") as HTMLElement;
    const title = container.querySelector(".moss-modal-title") as HTMLElement;
    expect(title.textContent).toBe("Remove user@example.com?");
    expect(container.getAttribute("aria-labelledby")).toBe(title.id);
    clickAction("cancel");
  });

  it("uses the danger button style only when destructive is set", () => {
    void mossConfirm("Discard changes?", { confirmLabel: "Discard", destructive: true });
    expect(document.querySelector('[data-action="confirm"]')!.className).toContain("moss-btn-danger");
    clickAction("cancel");

    void mossConfirm("Save changes?", { confirmLabel: "Save" });
    expect(document.querySelector('[data-action="confirm"]')!.className).toContain("moss-btn-primary");
    clickAction("cancel");
  });

  it("defaults the Cancel label and lets a caller override it", async () => {
    void mossConfirm("Discard changes?", { confirmLabel: "Discard" });
    expect(document.querySelector('[data-action="cancel"]')!.textContent).toBe("Cancel");
    clickAction("cancel");

    void mossConfirm("Discard changes?", { confirmLabel: "Discard", cancelLabel: "Keep editing" });
    expect(document.querySelector('[data-action="cancel"]')!.textContent).toBe("Keep editing");
    clickAction("cancel");
  });
});

describe("mossAlert", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("resolves once the single OK/Done button is clicked, with no Cancel button", async () => {
    const resultPromise = mossAlert("Could not delete: disk full");
    expect(document.querySelector('[data-action="cancel"]')).toBeNull();
    const ok = document.querySelector<HTMLButtonElement>('[data-action="confirm"]')!;
    expect(ok.textContent).toBe("OK");
    ok.click();
    await expect(resultPromise).resolves.toBeUndefined();
  });

  it("resolves on Escape too, since there is nothing to cancel back to", async () => {
    const resultPromise = mossAlert("Could not delete: disk full");
    pressEscape();
    await expect(resultPromise).resolves.toBeUndefined();
  });

  it("never uses the danger style — nothing here is a choice", () => {
    void mossAlert("Could not delete: disk full");
    expect(document.querySelector('[data-action="confirm"]')!.className).not.toContain("moss-btn-danger");
    document.querySelector<HTMLButtonElement>('[data-action="confirm"]')!.click();
  });
});
