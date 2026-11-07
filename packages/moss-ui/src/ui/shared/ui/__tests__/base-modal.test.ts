/**
 * Unit tests for BaseModal
 *
 * Tests the abstract modal base class: open/close lifecycle, backdrop dismiss,
 * escape key dismiss, focus trapping, focus restoration, and destroy cleanup.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { BaseModal } from "../base-modal";
import { initTooltips, destroyTooltips } from "../../../components/tooltip";

/**
 * Concrete test subclass of BaseModal.
 * Provides a button and an input so focus trap has elements to cycle through.
 */
class TestModal extends BaseModal {
  renderContent(): HTMLElement {
    const wrapper = document.createElement("div");
    wrapper.className = "test-modal-content";

    const btn = document.createElement("button");
    btn.textContent = "Test Button";
    btn.className = "test-btn";
    wrapper.appendChild(btn);

    const input = document.createElement("input");
    input.type = "text";
    input.className = "test-input";
    wrapper.appendChild(input);

    return wrapper;
  }
}

/**
 * A modal shaped like the confirm-dialog contract: `.moss-modal-title` +
 * `.moss-modal-actions` with a leading Cancel and a trailing default button.
 * `rebuild()` mimics ReverifyConfirmModal/OnionPressPublishModal's in-place
 * re-render (a full content replacement that does not go through `open()`
 * again), so the aria-labelledby test can prove the label follows a
 * REPLACED title element, not just the first one.
 */
class ConfirmShapedModal extends BaseModal {
  public cancelClicks = 0;
  public primaryClicks = 0;
  private root!: HTMLElement;

  renderContent(): HTMLElement {
    this.root = document.createElement("div");
    this.paint("Are you sure?");
    return this.root;
  }

  rebuild(titleText: string): void {
    this.root.innerHTML = "";
    this.paint(titleText);
    this.wireAriaLabel();
  }

  private paint(titleText: string): void {
    const title = document.createElement("h2");
    title.className = "moss-modal-title";
    title.textContent = titleText;
    this.root.appendChild(title);

    const actions = document.createElement("div");
    actions.className = "moss-modal-actions";

    const cancel = document.createElement("button");
    cancel.className = "moss-btn moss-btn-secondary";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", () => this.cancelClicks++);
    actions.appendChild(cancel);

    const primary = document.createElement("button");
    primary.className = "moss-btn moss-btn-primary";
    primary.textContent = "Confirm";
    primary.addEventListener("click", () => this.primaryClicks++);
    actions.appendChild(primary);

    this.root.appendChild(actions);
  }

  get textarea(): HTMLTextAreaElement {
    const el = document.createElement("textarea");
    this.root.appendChild(el);
    return el;
  }
}

describe("BaseModal", () => {
  let modal: TestModal;

  beforeEach(() => {
    document.body.innerHTML = "";
    vi.clearAllMocks();
  });

  afterEach(() => {
    modal?.destroy();
    document.body.innerHTML = "";
  });

  describe("open", () => {
    it("should append backdrop and container to DOM with correct classes", () => {
      modal = new TestModal();
      modal.open();

      const backdrop = document.querySelector(".moss-modal-backdrop");
      const container = document.querySelector(".moss-modal");

      expect(backdrop).not.toBeNull();
      expect(container).not.toBeNull();
      expect(document.body.contains(backdrop)).toBe(true);
      expect(document.body.contains(container)).toBe(true);
    });

    it("should add .visible class to backdrop and container", () => {
      modal = new TestModal();
      modal.open();

      const backdrop = document.querySelector(".moss-modal-backdrop");
      const container = document.querySelector(".moss-modal");

      expect(backdrop?.classList.contains("visible")).toBe(true);
      expect(container?.classList.contains("visible")).toBe(true);
    });

    it("should render content from renderContent() into the container", () => {
      modal = new TestModal();
      modal.open();

      const container = document.querySelector(".moss-modal");
      const content = container?.querySelector(".test-modal-content");
      expect(content).not.toBeNull();
    });

    it("keeps the first focusable element as the default when no subclass opts in", () => {
      modal = new TestModal();
      modal.open();

      expect(document.activeElement).toBe(
        document.querySelector(".test-btn")
      );
    });
  });

  describe("close", () => {
    it("should close on Escape key", () => {
      modal = new TestModal();
      modal.open();

      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

      expect(modal.isOpen()).toBe(false);
      expect(document.querySelector(".moss-modal-backdrop")).toBeNull();
      expect(document.querySelector(".moss-modal")).toBeNull();
    });

    it("should close on backdrop click", () => {
      modal = new TestModal();
      modal.open();

      const backdrop = document.querySelector(".moss-modal-backdrop") as HTMLElement;
      backdrop.click();

      expect(modal.isOpen()).toBe(false);
    });

    it("should NOT close when clicking inside the container", () => {
      modal = new TestModal();
      modal.open();

      const container = document.querySelector(".moss-modal") as HTMLElement;
      container.click();

      expect(modal.isOpen()).toBe(true);
    });

    it("should remove elements from DOM on close", () => {
      modal = new TestModal();
      modal.open();
      modal.close();

      expect(document.querySelector(".moss-modal-backdrop")).toBeNull();
      expect(document.querySelector(".moss-modal")).toBeNull();
    });
  });

  describe("focus trap", () => {
    it("should wrap Tab from last focusable to first", () => {
      modal = new TestModal();
      modal.open();

      const container = document.querySelector(".moss-modal") as HTMLElement;
      const btn = container.querySelector(".test-btn") as HTMLElement;
      const input = container.querySelector(".test-input") as HTMLElement;

      // Focus the last element (input)
      input.focus();
      expect(document.activeElement).toBe(input);

      // Press Tab (not shift) on the last element
      const tabEvent = new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: false,
        bubbles: true,
      });
      const prevented = !container.dispatchEvent(tabEvent);
      // In jsdom, we check that focus moved to first element
      // The handler calls preventDefault and focuses first element
      expect(document.activeElement).toBe(btn);
    });

    it("should wrap Shift+Tab from first focusable to last", () => {
      modal = new TestModal();
      modal.open();

      const container = document.querySelector(".moss-modal") as HTMLElement;
      const btn = container.querySelector(".test-btn") as HTMLElement;
      const input = container.querySelector(".test-input") as HTMLElement;

      // Focus the first element (button)
      btn.focus();
      expect(document.activeElement).toBe(btn);

      // Press Shift+Tab on the first element
      const shiftTabEvent = new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
      });
      container.dispatchEvent(shiftTabEvent);

      expect(document.activeElement).toBe(input);
    });
  });

  describe("focus restoration", () => {
    it("should restore focus to previously focused element on close", () => {
      // Create an external button that has focus before modal opens
      const externalBtn = document.createElement("button");
      externalBtn.textContent = "External";
      document.body.appendChild(externalBtn);
      externalBtn.focus();
      expect(document.activeElement).toBe(externalBtn);

      modal = new TestModal();
      modal.open();

      // Focus should have moved into the modal
      expect(document.activeElement).not.toBe(externalBtn);

      modal.close();

      // Focus should be restored to the external button
      expect(document.activeElement).toBe(externalBtn);
    });
  });

  describe("destroy", () => {
    it("should remove all DOM elements", () => {
      modal = new TestModal();
      modal.open();
      modal.destroy();

      expect(document.querySelector(".moss-modal-backdrop")).toBeNull();
      expect(document.querySelector(".moss-modal")).toBeNull();
    });

    it("should not respond to Escape after destroy", () => {
      modal = new TestModal();
      modal.open();
      modal.destroy();

      // Escape should not throw or do anything
      expect(() => {
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
      }).not.toThrow();
    });

    it("should be safe to call destroy multiple times", () => {
      modal = new TestModal();
      modal.open();
      modal.destroy();

      expect(() => modal.destroy()).not.toThrow();
    });
  });

  describe("duplicate open prevention", () => {
    it("should not create duplicate DOM when already open", () => {
      modal = new TestModal();
      modal.open();
      modal.open(); // second call

      const backdrops = document.querySelectorAll(".moss-modal-backdrop");
      const containers = document.querySelectorAll(".moss-modal");

      expect(backdrops.length).toBe(1);
      expect(containers.length).toBe(1);
    });
  });

  describe("isOpen", () => {
    it("should return false before open", () => {
      modal = new TestModal();
      expect(modal.isOpen()).toBe(false);
    });

    it("should return true after open", () => {
      modal = new TestModal();
      modal.open();
      expect(modal.isOpen()).toBe(true);
    });

    it("should return false after close", () => {
      modal = new TestModal();
      modal.open();
      modal.close();
      expect(modal.isOpen()).toBe(false);
    });

    it("should return false after destroy", () => {
      modal = new TestModal();
      modal.open();
      modal.destroy();
      expect(modal.isOpen()).toBe(false);
    });
  });

  /**
   * Regression: repeated Publish clicks constructed a NEW modal each time
   * (deploy-handler clears its re-entrancy flag immediately before
   * constructing), and every instance appended its own backdrop. The stacked
   * `rgba(0,0,0,0.5)` layers compounded and light-dismiss closed only the
   * topmost, so the modal was inescapable and the screen kept darkening.
   */
  describe("one instance per class", () => {
    class OtherModal extends TestModal {}

    it("a second instance of the same class does not stack a backdrop", () => {
      modal = new TestModal();
      modal.open();
      const second = new TestModal();
      second.open();

      expect(document.querySelectorAll(".moss-modal-backdrop").length).toBe(1);
      expect(second.isOpen()).toBe(false);
      expect(modal.isOpen()).toBe(true);
      second.destroy();
    });

    it("one light-dismiss is enough to clear the screen after repeat opens", () => {
      modal = new TestModal();
      modal.open();
      const extras = [new TestModal(), new TestModal()];
      extras.forEach((m) => m.open());

      const backdrop = document.querySelector(
        ".moss-modal-backdrop"
      ) as HTMLElement;
      backdrop.click();

      expect(document.querySelectorAll(".moss-modal-backdrop").length).toBe(0);
      expect(document.querySelectorAll(".moss-modal").length).toBe(0);
      extras.forEach((m) => m.destroy());
    });

    it("closing releases the slot so the modal can be reopened later", () => {
      modal = new TestModal();
      modal.open();
      modal.close();

      const reopened = new TestModal();
      reopened.open();
      expect(reopened.isOpen()).toBe(true);
      expect(document.querySelectorAll(".moss-modal-backdrop").length).toBe(1);
      reopened.destroy();
    });

    it("does not let a modal whose DOM was wiped block the next open", () => {
      // A host that clears its container (or a reload-like reset) leaves an
      // instance still claiming _isOpen. Deferring to that zombie would make
      // the modal permanently unopenable.
      modal = new TestModal();
      modal.open();
      document.body.innerHTML = "";

      const next = new TestModal();
      next.open();
      expect(next.isOpen()).toBe(true);
      expect(document.querySelectorAll(".moss-modal-backdrop").length).toBe(1);
      next.destroy();
    });

    it("a different modal class may still stack (confirm over settings)", () => {
      modal = new TestModal();
      modal.open();
      const other = new OtherModal();
      other.open();

      expect(other.isOpen()).toBe(true);
      expect(document.querySelectorAll(".moss-modal-backdrop").length).toBe(2);
      other.destroy();
    });
  });
});

/**
 * Modal content sits at `--z-modal` + 1 (51) and a tooltip at `--z-popup`
 * (100), so this is the one tier where a stale tooltip floats OVER the surface
 * instead of under it. Modals arm no outside-dismissal — the backdrop click is
 * their own — so `armOutsideDismiss` cannot speak for them and BaseModal holds
 * the suppression itself.
 */
describe("BaseModal — no tooltip floats over an open modal", () => {
  let modal: TestModal;

  beforeEach(() => {
    initTooltips();
    modal = new TestModal();
  });

  afterEach(() => {
    modal.close();
    destroyTooltips();
  });

  function hoverTrigger(): void {
    const trigger = document.createElement("button");
    trigger.setAttribute("data-tooltip", "Publish");
    document.body.appendChild(trigger);
    trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  }

  const tooltipShown = (): boolean =>
    document.querySelector(".moss-tooltip")?.classList.contains("visible") ?? false;

  it("hides the tooltip the opening click left behind, and restores it on close", () => {
    hoverTrigger();
    expect(tooltipShown()).toBe(true);

    modal.open();
    expect(tooltipShown()).toBe(false);

    hoverTrigger();
    expect(tooltipShown()).toBe(false);

    modal.close();
    hoverTrigger();
    expect(tooltipShown()).toBe(true);
  });
});

/**
 * The modal contract: every
 * modal's container is auto-labelled from its own `.moss-modal-title`, and
 * Return activates the trailing default button unless focus is somewhere
 * that Return already means something else.
 */
describe("BaseModal — modal contract", () => {
  let modal: ConfirmShapedModal;

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  afterEach(() => {
    modal?.destroy();
    document.body.innerHTML = "";
  });

  describe("aria-labelledby auto-wiring", () => {
    it("points the container at the rendered .moss-modal-title", () => {
      modal = new ConfirmShapedModal();
      modal.open();

      const container = document.querySelector(".moss-modal") as HTMLElement;
      const title = container.querySelector(".moss-modal-title") as HTMLElement;

      expect(title.id).not.toBe("");
      expect(container.getAttribute("aria-labelledby")).toBe(title.id);
    });

    it("follows a title element that gets replaced by an in-place re-render", () => {
      // A subclass whose `renderContent()` runs once but whose state
      // transitions rebuild the whole content tree (ReverifyConfirmModal's
      // rerender(), OnionPressPublishModal's transitionTo()) must not leave
      // aria-labelledby pointing at an id that no longer exists in the DOM.
      modal = new ConfirmShapedModal();
      modal.open();
      const container = document.querySelector(".moss-modal") as HTMLElement;
      const firstId = container.getAttribute("aria-labelledby");

      modal.rebuild("Now sending…");

      const newTitle = container.querySelector(".moss-modal-title") as HTMLElement;
      expect(container.getAttribute("aria-labelledby")).toBe(newTitle.id);
      expect(document.getElementById(container.getAttribute("aria-labelledby")!)).toBe(newTitle);
      // The replaced title got its OWN id — the old one is gone with the old element.
      expect(newTitle.id).not.toBe(firstId);
    });

    it("keeps an id the title already carries, rather than minting a second one", () => {
      // DeleteConfirmModal assigns its own per-instance title id before
      // BaseModal ever sees the element; wireAriaLabel must not clobber it.
      class PreLabeledModal extends BaseModal {
        renderContent(): HTMLElement {
          const root = document.createElement("div");
          const title = document.createElement("h2");
          title.className = "moss-modal-title";
          title.id = "my-own-title-id";
          root.appendChild(title);
          return root;
        }
      }
      const preLabeled = new PreLabeledModal();
      preLabeled.open();
      const container = document.querySelector(".moss-modal") as HTMLElement;
      expect(container.getAttribute("aria-labelledby")).toBe("my-own-title-id");
      preLabeled.destroy();
    });
  });

  describe("Return activates the default button", () => {
    function pressEnter(target: HTMLElement): void {
      target.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    }

    it("clicks the trailing .moss-btn-primary when Enter fires from a non-button element", () => {
      modal = new ConfirmShapedModal();
      modal.open();
      const container = document.querySelector(".moss-modal") as HTMLElement;
      pressEnter(container);
      expect(modal.primaryClicks).toBe(1);
      expect(modal.cancelClicks).toBe(0);
    });

    it("does nothing when focus is on a <textarea> (Enter must insert a newline)", () => {
      modal = new ConfirmShapedModal();
      modal.open();
      const ta = modal.textarea;
      pressEnter(ta);
      expect(modal.primaryClicks).toBe(0);
    });

    it("does not double-fire when a non-default button already handles its own Enter", () => {
      modal = new ConfirmShapedModal();
      modal.open();
      const cancel = document.querySelector(".moss-modal-actions .moss-btn-secondary") as HTMLElement;
      pressEnter(cancel);
      // activateDefault() must ignore Enter fired FROM a <button> — the
      // button's own native activation is the only thing that should fire,
      // and this fixture's cancel button has no keydown listener of its own,
      // so a correct implementation records zero clicks either way.
      expect(modal.primaryClicks).toBe(0);
    });
  });
});
