/**
 * math-copy.ts — click-to-copy LaTeX source for math nodes.
 *
 * P2 typesets an equation to `<svg class="moss-math">` whose `aria-label`
 * carries the raw TeX (no `$` delimiters — `getAttribute` HTML-unescapes,
 * so `<`, `&` and quotes round-trip). Clicking the equation copies its
 * markdown source — delimiters restored per `data-moss-math`, matching
 * moss-core's `math_source` spelling (`$…$` / `$$…$$`) so copy-paste into
 * a markdown file rebuilds the same equation. The P1 fallback chip
 * (`<code class="moss-math">`) already renders the source delimiters-
 * included as its text; it is copied verbatim.
 *
 * Delegated document-level listener, same shape as heading-anchor.ts:
 * single-bind and MORPH-IMMUNE by construction (idiomorph swaps nodes,
 * never the document), so no `moss-morph-patched` re-attach is needed.
 */
import { showToast } from "./toast";

export {};

/**
 * The equation's markdown source for a matched math node, or null when
 * the node carries no source (defensive: a stripped aria-label).
 */
export function mathCopyPayload(el: Element): string | null {
  if (el.tagName.toLowerCase() === "svg") {
    const tex = el.getAttribute("aria-label");
    if (tex === null) return null;
    return el.getAttribute("data-moss-math") === "display" ? `$$${tex}$$` : `$${tex}$`;
  }
  // P1 <code> fallback chip: textContent IS the markdown source,
  // delimiters included (math_text.rs keeps them deliberately).
  return el.textContent || null;
}

function onMathClick(event: MouseEvent): void {
  const target = event.target as Element | null;
  const math = target?.closest<Element>("svg.moss-math, code.moss-math");
  if (!math) return;

  const payload = mathCopyPayload(math);
  if (!payload) return;

  // navigator.clipboard is unavailable in insecure contexts — degrade
  // gracefully (the equation stays readable; nothing else was promised).
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(payload).then(
      () => showToast(),
      () => {
        /* copy failed — no toast */
      },
    );
  }
}

document.addEventListener("click", onMathClick);
