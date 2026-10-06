import { onQuoteComment } from "./quote-events";

export interface QuoteFloat {
  cancel(): void;
  /** Markdown blockquote prefix to prepend to comment content; "" when not in float mode. */
  getQuotePrefix(): string;
  /** Raw selected text; "" when not in float mode. */
  getQuoteText(): string;
}

export interface QuoteFloatOptions {
  form: HTMLFormElement;
  textarea: HTMLTextAreaElement;
  defaultSlot: HTMLElement;
  /** Called before opening the shell, e.g. to clear an active reply state. */
  onBeforeOpen?: () => void;
}

// Singleton scope: per IIFE bundle. The artalk bundle owns one copy of this
// module, and only one bundle runs per page. A second `installQuoteFloat` call
// on the same page returns the existing instance (HMR safety).
let currentInstance: QuoteFloat | null = null;
let currentDispose: (() => void) | null = null;

export function installQuoteFloat(opts: QuoteFloatOptions): QuoteFloat {
  if (currentInstance) return currentInstance;

  const { form, textarea, defaultSlot, onBeforeOpen } = opts;

  let backdrop: HTMLElement | null = null;
  let shell: HTMLElement | null = null;
  let quoteText = "";

  function ensureShell(): { shell: HTMLElement; inner: HTMLElement } {
    if (!shell) {
      backdrop = document.createElement("div");
      backdrop.className = "comment-float-backdrop";
      backdrop.addEventListener("click", () => cancel());
      document.body.appendChild(backdrop);

      shell = document.createElement("div");
      shell.className = "comment-float-shell";
      const inner = document.createElement("div");
      inner.className = "comment-float-inner";
      shell.appendChild(inner);
      document.body.appendChild(shell);
    }
    const inner = shell.querySelector(".comment-float-inner") as HTMLElement;
    return { shell, inner };
  }

  function open(text: string): void {
    if (!text) return;
    onBeforeOpen?.();
    const { shell, inner } = ensureShell();

    inner.innerHTML = "";
    const quoteEl = document.createElement("div");
    quoteEl.className = "comment-float-quote";
    quoteEl.textContent = text;
    inner.appendChild(quoteEl);
    inner.appendChild(form);

    quoteText = text;
    backdrop!.classList.add("visible");
    shell.classList.add("open");
    textarea.focus();
  }

  function cancel(): void {
    if (!shell || !shell.classList.contains("open")) return;
    shell.classList.remove("open");
    backdrop?.classList.remove("visible");
    if (form.parentElement !== defaultSlot) {
      defaultSlot.appendChild(form);
    }
    quoteText = "";
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && shell?.classList.contains("open")) cancel();
  }

  document.addEventListener("keydown", onKeydown);
  const offQuote = onQuoteComment(({ text }) => open(text));

  // Production lifetime equals page lifetime — dispose is test-only. HMR for
  // these IIFE bundles doesn't fire (they're embedded server-side via
  // include_str! and emitted as a <script> tag at page load).
  currentDispose = () => {
    document.removeEventListener("keydown", onKeydown);
    offQuote();
    backdrop?.remove();
    shell?.remove();
  };

  currentInstance = {
    cancel,
    getQuoteText: () => quoteText,
    getQuotePrefix: () => formatQuoteAsBlockquote(quoteText),
  };
  return currentInstance;
}

// TODO: drop once moss-seta accepts a separate `quote_text` field on comment
// POSTs — providers will pass the quote alongside content instead of inlining
// it as a markdown blockquote.
function formatQuoteAsBlockquote(text: string): string {
  if (!text) return "";
  // Trim trailing newlines so a selection ending in "\n" doesn't produce
  // a stray "> " line at the end of the blockquote.
  const trimmed = text.replace(/\n+$/, "");
  if (!trimmed) return "";
  return trimmed.split("\n").map(l => "> " + l).join("\n") + "\n\n";
}

/** Test-only: tear down the singleton (listeners + DOM) between tests. */
export function _resetQuoteFloatForTests(): void {
  currentDispose?.();
  currentInstance = null;
  currentDispose = null;
}
