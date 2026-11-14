import { Idiomorph } from "idiomorph";
import { syncBodyAttributes, forceRepaint, morphCounter } from "../morph-guard";

export /** Mirrors the iframe-bridge.ts moss-morph handler exactly.
 *  Keep in sync with the beforeNodeMorphed + beforeNodeRemoved +
 *  beforeAttributeUpdated vetoes there. */
function applyMorph(html: string): { added: number; removed: number } {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const beforeNodeMorphed = (oldNode: Node) =>
    !(oldNode instanceof Element && oldNode.closest("[data-moss-permanent]"));
  // Mirror the beforeNodeRemoved veto from iframe-bridge.ts — a JS-appended
  // [data-moss-permanent] node (absent from the served bytes) is a surplus node
  // idiomorph would remove; removal is gated only here, NOT by beforeNodeMorphed.
  // closest (self/ancestor) OR querySelector (permanent node inside an unmarked
  // surplus wrapper — idiomorph only fires this on the outermost surplus node).
  const beforeNodeRemoved = (node: Node) =>
    !(
      node instanceof Element &&
      (node.closest("[data-moss-permanent]") ||
        node.querySelector("[data-moss-permanent]"))
    );
  // Mirror the <details open> veto from iframe-bridge.ts — user-opened state
  // must survive morphs whose served bytes never carry `open`.
  const beforeAttributeUpdated = (
    name: string,
    node: Node,
    _type: "update" | "remove",
  ) => !(name === "open" && (node as Element).tagName === "DETAILS");
  // Mirror the DocumentFragment source from iframe-bridge.ts (the second-<body>-nesting regression). Passing
  // `.innerHTML` STRINGS here is what shipped the bug: idiomorph sniffs a string
  // source for a literal `</html>`/`</head>`/`</body>` and, on a hit, re-parses
  // it as a whole document — nesting the page inside a second <body>. A fragment
  // has no text to sniff, and unlike passing `doc.body` itself there is no
  // element for idiomorph's normalizeParent to wrap.
  const fragmentOf = (source: HTMLElement): DocumentFragment => {
    const holder = document.createElement("template");
    const inert = holder.content.ownerDocument;
    for (const node of Array.from(source.childNodes)) {
      holder.content.appendChild(inert.importNode(node, true));
    }
    return holder.content;
  };
  const tally = morphCounter();
  const callbacks = { beforeNodeMorphed, beforeNodeRemoved, beforeAttributeUpdated, ...tally.callbacks };
  Idiomorph.morph(document.head, fragmentOf(doc.head), {
    morphStyle: "innerHTML",
    ignoreActiveValue: true,
    callbacks,
  });
  const active = document.activeElement as HTMLElement | null;
  Idiomorph.morph(document.body, fragmentOf(doc.body), {
    morphStyle: "innerHTML",
    ignoreActiveValue: !!active && (active.matches("input, textarea, select") || active.isContentEditable),
    callbacks,
  });
  // NOT mirrored — the bridge and this harness call the SAME function, so the
  // <body> attribute sync cannot drift from what ships.
  syncBodyAttributes(doc.body);
  forceRepaint();
  return tally.counts;
}
