/**
 * Tests for installQuoteFloat — the bottom-sheet that opens when the user
 * picks "Comment" from the selection tooltip.
 *
 * Covers:
 *  - opens shell with quote text + form, focus on textarea
 *  - cancel via ESC, backdrop click, explicit cancel(); idempotency
 *  - A → B re-dispatch keeps a single .comment-float-quote
 *  - getQuoteText() and getQuotePrefix() lifecycle
 *  - multi-line quote prefixes "> " per line
 *  - reply-state cleared via onBeforeOpen
 *  - double-install returns same instance (HMR safety)
 */

import { describe, test, expect, beforeEach, afterEach } from "vitest";
import {
  installQuoteFloat,
  _resetQuoteFloatForTests,
} from "../quote-float";
import { dispatchQuoteComment } from "../quote-events";

interface Harness {
  form: HTMLFormElement;
  textarea: HTMLTextAreaElement;
  defaultSlot: HTMLElement;
}

function mountHarness(): Harness {
  document.body.innerHTML = `
    <section class="moss-comments">
      <div class="comment-form-slot" id="default-form-slot">
        <form id="moss-comment-form">
          <textarea id="moss-comment-text" placeholder="Say something..."></textarea>
        </form>
      </div>
    </section>
  `;
  const form = document.getElementById("moss-comment-form") as HTMLFormElement;
  const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
  const defaultSlot = document.getElementById("default-form-slot")!;
  return { form, textarea, defaultSlot };
}

beforeEach(() => {
  _resetQuoteFloatForTests();
  document.body.innerHTML = "";
});

afterEach(() => {
  document.body.innerHTML = "";
});

describe("installQuoteFloat — open", () => {
  test("creates float shell with quote and moves form in, focuses textarea", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const qf = installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("hello world");

    const shell = document.querySelector(".comment-float-shell");
    const quote = document.querySelector(".comment-float-quote");
    expect(shell?.classList.contains("open")).toBe(true);
    expect(quote?.textContent).toBe("hello world");
    expect(form.parentElement?.classList.contains("comment-float-inner")).toBe(true);
    expect(document.activeElement).toBe(textarea);
    expect(qf.getQuoteText()).toBe("hello world");
  });

  test("backdrop becomes visible when shell opens", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("hi");

    const backdrop = document.querySelector(".comment-float-backdrop");
    expect(backdrop?.classList.contains("visible")).toBe(true);
  });

  test("does not open with empty text", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("");

    expect(document.querySelector(".comment-float-shell")).toBeNull();
  });

  test("re-dispatch (A then B) shows only the latest quote", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("first");
    dispatchQuoteComment("second");

    const quotes = document.querySelectorAll(".comment-float-quote");
    expect(quotes.length).toBe(1);
    expect(quotes[0].textContent).toBe("second");
  });
});

describe("installQuoteFloat — cancel", () => {
  test("ESC closes shell and returns form to default slot", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("text");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    const shell = document.querySelector(".comment-float-shell");
    expect(shell?.classList.contains("open")).toBe(false);
    expect(form.parentElement).toBe(defaultSlot);
  });

  test("backdrop click cancels", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("text");
    const backdrop = document.querySelector(".comment-float-backdrop") as HTMLElement;
    backdrop.click();

    expect(form.parentElement).toBe(defaultSlot);
    expect(document.querySelector(".comment-float-shell")?.classList.contains("open")).toBe(false);
  });

  test("explicit cancel() works and clears quote text", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const qf = installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("text");
    expect(qf.getQuoteText()).toBe("text");
    qf.cancel();

    expect(qf.getQuoteText()).toBe("");
    expect(form.parentElement).toBe(defaultSlot);
  });

  test("cancel() is idempotent when shell is not open", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const qf = installQuoteFloat({ form, textarea, defaultSlot });

    expect(() => qf.cancel()).not.toThrow();
    expect(qf.getQuoteText()).toBe("");

    dispatchQuoteComment("text");
    qf.cancel();
    expect(() => qf.cancel()).not.toThrow();
    expect(qf.getQuoteText()).toBe("");
  });
});

describe("installQuoteFloat — quote prefix", () => {
  test("getQuotePrefix returns '' before any open and after cancel", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const qf = installQuoteFloat({ form, textarea, defaultSlot });

    expect(qf.getQuotePrefix()).toBe("");
    dispatchQuoteComment("hi");
    expect(qf.getQuotePrefix()).toBe("> hi\n\n");
    qf.cancel();
    expect(qf.getQuotePrefix()).toBe("");
  });

  test("multi-line text prefixes '> ' per line", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const qf = installQuoteFloat({ form, textarea, defaultSlot });

    dispatchQuoteComment("line1\nline2\nline3");
    expect(qf.getQuotePrefix()).toBe("> line1\n> line2\n> line3\n\n");
  });
});

describe("installQuoteFloat — onBeforeOpen", () => {
  test("invokes onBeforeOpen each time before opening", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    let calls = 0;
    installQuoteFloat({
      form,
      textarea,
      defaultSlot,
      onBeforeOpen: () => { calls += 1; },
    });

    dispatchQuoteComment("a");
    dispatchQuoteComment("b");

    expect(calls).toBe(2);
  });

  test("onBeforeOpen runs before the form moves into the shell", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    // Simulate a reply: move form out of default slot into a fake reply container.
    const reply = document.createElement("div");
    reply.className = "reply-target";
    document.body.appendChild(reply);
    reply.appendChild(form);

    let parentBeforeOpen: Element | null = null;
    installQuoteFloat({
      form,
      textarea,
      defaultSlot,
      onBeforeOpen: () => {
        // Provider's cancelReply() would put form back in defaultSlot here.
        defaultSlot.appendChild(form);
        parentBeforeOpen = form.parentElement;
      },
    });

    dispatchQuoteComment("text");

    expect(parentBeforeOpen).toBe(defaultSlot);
    // After open, form is now inside .comment-float-inner, not defaultSlot.
    expect(form.parentElement?.classList.contains("comment-float-inner")).toBe(true);
  });
});

describe("installQuoteFloat — install once", () => {
  test("double-install returns same instance, single set of listeners", () => {
    const { form, textarea, defaultSlot } = mountHarness();
    const a = installQuoteFloat({ form, textarea, defaultSlot });
    const b = installQuoteFloat({ form, textarea, defaultSlot });
    expect(a).toBe(b);

    dispatchQuoteComment("text");
    // Single listener => single shell, single quote element.
    expect(document.querySelectorAll(".comment-float-shell").length).toBe(1);
    expect(document.querySelectorAll(".comment-float-quote").length).toBe(1);
  });
});
