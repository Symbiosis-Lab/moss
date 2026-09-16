/**
 * Tests for the iframe-bridge morph handler (#771 keystone).
 *
 * The bridge (an IIFE injected into the preview iframe) handles `moss-morph` by
 * re-fetching its own URL and reconciling <head>/<body> in place with idiomorph.
 * The KEYSTONE: the injected bridge <script> must survive a morph WITHOUT being
 * re-inserted (which would re-execute it and duplicate every listener). It is
 * marked `id="moss-bridge" data-moss-permanent`, and a `beforeNodeMorphed` veto
 * skips anything under `[data-moss-permanent]`.
 *
 * Since the bridge is an IIFE (not importable without running its setup), we:
 *   (a) run a LOCAL HARNESS that mirrors the handler's exact morph calls against
 *       real idiomorph + jsdom, proving the permanent node keeps its identity
 *       across a morph that shifts its sibling position while content updates;
 *   (b) assert the shipped, bundled artifact still carries the keystone shape so
 *       it cannot regress silently.
 *
 * Note: jsdom does not execute <script> in innerHTML, so we prove the keystone
 * via NODE IDENTITY (a preserved node is reused, not re-inserted → in a real
 * browser, not re-executed). The actual no-re-exec is verified on WebKit by the
 * e2e keystone driver + `__bridgeInitCount`.
 */
import { describe, it, test, expect, beforeEach } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Idiomorph } from "idiomorph";
import {
  scriptsDiffer,
  differingScriptKeys,
  describeScriptKey,
  describeScriptChange,
  syncBodyAttributes,
  captureServerBody,
  resetBodyAttributeMemory,
} from "../morph-guard";

/** Mirrors the iframe-bridge.ts moss-morph handler exactly.
 *  Keep in sync with the beforeNodeMorphed + beforeNodeRemoved +
 *  beforeAttributeUpdated vetoes there. */
function applyMorph(html: string): void {
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
  Idiomorph.morph(document.head, fragmentOf(doc.head), {
    morphStyle: "innerHTML",
    ignoreActiveValue: true,
    callbacks: { beforeNodeMorphed, beforeNodeRemoved, beforeAttributeUpdated },
  });
  Idiomorph.morph(document.body, fragmentOf(doc.body), {
    morphStyle: "innerHTML",
    ignoreActiveValue: true,
    restoreFocus: true,
    callbacks: { beforeNodeMorphed, beforeNodeRemoved, beforeAttributeUpdated },
  });
  // NOT mirrored — the bridge and this harness call the SAME function, so the
  // <body> attribute sync cannot drift from what ships.
  syncBodyAttributes(doc.body);
}

/** The shipped bridge bundle — the exact bytes Rust injects into every preview
 *  page as `<script id="moss-bridge" data-moss-permanent>`. Read from disk (not
 *  approximated) because the second-<body>-nesting regression was a property of the REAL bundle's text. */
const BUNDLE_PATH = resolve(
  __dirname,
  "../../../ops/serve/js/iframe-bridge.js",
);
const bundleExists = existsSync(BUNDLE_PATH);

describe("iframe-bridge morph handler (local harness)", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
    document.body.innerHTML = "";
    for (const a of Array.from(document.body.attributes)) document.body.removeAttribute(a.name);
    resetBodyAttributeMemory();
  });

  test("syncs <body>'s own attributes, which an innerHTML morph never touches", () => {
    // shell.html emits these from per-page frontmatter; they are the editor's
    // Width and Typesetting chips. Before the sync they froze at page load, so a
    // chip changed the content and not the layout until a manual reload.
    document.body.setAttribute("data-content-width", "wide");
    document.body.setAttribute("data-typesetting", "vertical");
    captureServerBody(); // page load: this is what the server sent
    document.body.classList.add("immersive-fs-active"); // runtime-only, added after

    applyMorph(
      `<html><head></head><body data-content-width="narrow"><p>x</p></body></html>`,
    );

    expect(document.body.getAttribute("data-content-width")).toBe("narrow");
    // Turning a chip off drops the attribute entirely — a stale value must go.
    expect(document.body.hasAttribute("data-typesetting")).toBe(false);
    // ...but the runtime-only class survives the removal pass.
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
  });

  test("a morph mid-lightbox does not unlock background scroll", () => {
    // frontend/site/fullscreen.ts sets this to lock scrolling while the overlay
    // is open. The server never emits an inline style on <body>, so a blind
    // "remove what the new document lacks" strips it and the page scrolls behind
    // a still-open lightbox. Found in review of the first cut of this sync.
    document.body.setAttribute("data-content-width", "wide");
    captureServerBody(); // page load: this is what the server sent
    document.body.style.overflow = "hidden";

    applyMorph(`<html><head></head><body data-content-width="wide"><p>x</p></body></html>`);

    expect(document.body.style.overflow).toBe("hidden");
  });

  test("a runtime class added after load is not clobbered, and a server class it replaces is", () => {
    document.body.setAttribute("class", "theme-a");
    captureServerBody(); // page load: theme-a is the SERVER's class
    document.body.classList.add("immersive-fs-active"); // runtime-only

    applyMorph(`<html><head></head><body class="theme-b"><p>x</p></body></html>`);

    // The server's own class is replaceable...
    expect(document.body.classList.contains("theme-a")).toBe(false);
    expect(document.body.classList.contains("theme-b")).toBe(true);
    // ...the runtime's is not, and nothing had to name it.
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
  });

  test("preserves the permanent bridge node across a morph while content updates", () => {
    // Old document: a content paragraph, then the injected bridge script.
    document.body.innerHTML =
      `<p id="content">old</p>` +
      `<script type="text/javascript" id="moss-bridge" data-moss-permanent></script>`;
    const bridge = document.getElementById("moss-bridge")!;
    // Stamp a runtime-only property: it survives ONLY if the node itself is
    // reused (idiomorph did not replace it).
    (bridge as unknown as { __alive: boolean }).__alive = true;

    // Rebuilt page: a NEW content script is inserted BEFORE the paragraph,
    // shifting the bridge's sibling position — the exact shape that made the
    // un-hardened (no-id) bridge re-execute. Content text also changes.
    applyMorph(
      `<html><head></head><body>` +
        `<script id="site-analytics"></script>` +
        `<p id="content">new</p>` +
        `<script type="text/javascript" id="moss-bridge" data-moss-permanent></script>` +
        `</body></html>`,
    );

    // Keystone: the SAME bridge node persists (identity + stamped property).
    const after = document.getElementById("moss-bridge")!;
    expect(after).toBe(bridge);
    expect((after as unknown as { __alive?: boolean }).__alive).toBe(true);

    // The non-permanent content was reconciled, and the new content script landed.
    expect(document.getElementById("content")!.textContent).toBe("new");
    expect(document.getElementById("site-analytics")).not.toBeNull();
  });

  test("a JS-appended [data-moss-permanent] node survives a morph that would otherwise remove it", () => {
    // A site theme script appends an overlay to <body> at runtime (the canonical
    // case: the sunlight leaf video + wash). It is absent from the served bytes,
    // so a body innerHTML morph treats it as a surplus node and would REMOVE it —
    // idiomorph's removal is gated only by beforeNodeRemoved. The beforeNodeMorphed
    // veto does NOT cover removal, so the escape hatch needs beforeNodeRemoved too.
    document.body.innerHTML = `<p id="content">old</p>`;
    const overlay = document.createElement("div");
    overlay.id = "overlay";
    overlay.setAttribute("data-moss-permanent", "");
    document.body.appendChild(overlay);
    // Stamp a runtime-only property: it survives ONLY if the node itself is kept.
    (overlay as unknown as { __alive: boolean }).__alive = true;

    // Rebuilt page: content changes; the overlay is NOT in the served HTML.
    applyMorph(`<html><head></head><body><p id="content">new</p></body></html>`);

    // The overlay node survives (identity + stamped property); content reconciled.
    const after = document.getElementById("overlay");
    expect(after).toBe(overlay);
    expect((after as unknown as { __alive?: boolean }).__alive).toBe(true);
    expect(document.getElementById("content")!.textContent).toBe("new");
  });

  test("a permanent node WRAPPED in an unmarked surplus container survives (querySelector veto)", () => {
    // Marker on an INNER node: a theme appends <div id="fx"><video permanent>.
    // idiomorph fires beforeNodeRemoved only on the OUTERMOST surplus node (#fx,
    // which is unmarked), so the veto must also look downward or the wrapped
    // permanent node is dropped with its container.
    document.body.innerHTML = `<p id="content">old</p>`;
    const wrap = document.createElement("div");
    wrap.id = "fx";
    const inner = document.createElement("video");
    inner.setAttribute("data-moss-permanent", "");
    wrap.appendChild(inner);
    document.body.appendChild(wrap);

    applyMorph(`<html><head></head><body><p id="content">new</p></body></html>`);

    expect(document.getElementById("fx")).toBe(wrap);
    expect(document.querySelector("#fx video[data-moss-permanent]")).toBe(inner);
    expect(document.getElementById("content")!.textContent).toBe("new");
  });

  test("a plain (non-permanent) surplus node is STILL removed (veto polarity not inverted)", () => {
    // Guards against an always-true veto that would keep stale content forever
    // and silently pass every other test.
    document.body.innerHTML = `<p id="keep">a</p><aside id="stale">gone</aside>`;
    applyMorph(`<html><head></head><body><p id="keep">a</p></body></html>`);
    expect(document.getElementById("stale")).toBeNull();
    expect(document.getElementById("keep")).not.toBeNull();
  });

  test("morphs <head> content (e.g. a changed stylesheet href) in place", () => {
    document.head.innerHTML = `<link rel="stylesheet" href="/site.css?v=1">`;
    applyMorph(
      `<html><head><link rel="stylesheet" href="/site.css?v=2"></head><body></body></html>`,
    );
    expect(document.head.querySelector("link")!.getAttribute("href")).toBe("/site.css?v=2");
  });

  test("recreated elements keep correct-case localNames in every namespace (createNode uses localName, not tagName)", () => {
    // A node with an id'd descendant forces idiomorph's createNode id-preserving
    // path, which recreates the node via `document.createElementNS(namespaceURI,
    // localName)`. localName (NOT tagName) is the only correct key:
    //   - HTML: localName is lowercase ("nav"); tagName is UPPERCASE ("NAV") and
    //     createElementNS does not case-normalize, so tagName yields an
    //     uppercase-localName HTML node that misses lowercase type-selector CSS
    //     (`nav {…}`, `article {…}`) → the reading-column layout collapses and
    //     content shifts flush-left.
    //   - SVG: localName preserves case ("svg"/"clipPath"); a plain createElement
    //     would make an HTMLUnknownElement and lowercase `viewBox`→`viewbox`,
    //     rendering the icon invisible.
    // Regression guard for the two-part idiomorph createNode patch.
    document.body.innerHTML =
      `<button id="tgl"><svg viewBox="0 0 32 32">` +
      `<clipPath id="cut"><path d="M0 0"/></clipPath></svg></button>`;

    // Rebuild wraps the id'd button in a FRESH <nav>: idiomorph must CREATE the
    // <nav> (its subtree carries tracked ids) via createNode → createElementNS.
    applyMorph(
      `<html><head></head><body>` +
        `<nav class="main-nav"><button id="tgl"><svg viewBox="0 0 32 32">` +
        `<clipPath id="cut"><path d="M0 0"/></clipPath></svg></button></nav>` +
        `</body></html>`,
    );

    const nav = document.body.querySelector("nav");
    expect(nav).not.toBeNull();
    // HTML: recreated in the HTML namespace with a LOWERCASE localName. With the
    // tagName bug this is "NAV" (uppercase) → the layout-collapse regression.
    expect(nav!.localName).toBe("nav");
    expect(nav!.namespaceURI).toBe("http://www.w3.org/1999/xhtml");
    // SVG: recreated in the SVG namespace with case-sensitive names + camelCase
    // attributes intact (guards the original createElement→createElementNS fix).
    const svg = nav!.querySelector("svg")!;
    expect(svg.namespaceURI).toBe("http://www.w3.org/2000/svg");
    expect(svg.getAttribute("viewBox")).toBe("0 0 32 32");
    expect(svg.getAttribute("viewbox")).toBeNull();
    expect(nav!.querySelector("clipPath")).not.toBeNull();
  });

  test("<details open> in the live DOM survives a morph whose served bytes have plain <details>", () => {
    // Regression: idiomorph 0.7.4 morphAttributes removes absent attributes
    // (treats the served snapshot as ground truth). Without the
    // `beforeAttributeUpdated` veto, a user-opened <details> (comments panel)
    // would slam shut on every watch-rebuild morph.
    document.body.innerHTML = `<details open><summary>Comments</summary><p>content</p></details>`;
    const details = document.querySelector("details")!;
    expect(details.hasAttribute("open")).toBe(true); // pre-morph sanity

    // Served bytes have plain <details> — no `open` attribute.
    applyMorph(
      `<html><head></head><body><details><summary>Comments</summary><p>content updated</p></details></body></html>`,
    );

    // The `open` attribute must survive — user state is preserved.
    const afterMorph = document.querySelector("details")!;
    expect(afterMorph.hasAttribute("open")).toBe(true);
    // Content was still reconciled.
    expect(afterMorph.querySelector("p")!.textContent).toBe("content updated");
  });

  // The second-<body>-nesting regression. Every other test in this file fixtures the bridge as an EMPTY
  // `<script id="moss-bridge">`, and that is precisely why the suite watched the
  // bug ship: idiomorph's document-sniff fires on the TEXT of the morph source,
  // and an empty script contributes no text. The real bundle does — it carries
  // idiomorph's own fallback parse path as a plain unescaped string literal
  // (`parseFromString("<body><template>" + … + "</template></body>", …)`), so
  // `doc.body.innerHTML` ALWAYS contained a literal `</body>`. The fixture below
  // therefore embeds the shipped bytes verbatim; anything smaller re-opens the
  // same blind spot.
  it.runIf(bundleExists)(
    "the real bridge bundle in <script id=moss-bridge> does not nest the page in a second <body>",
    () => {
      const bundle = readFileSync(BUNDLE_PATH, "utf8");
      // Guard the fixture's premise: if the bundle ever stops carrying a literal
      // `</body>`, this test would keep passing for the wrong reason.
      expect(bundle).toContain("</body>");

      // Rust injects the bridge before `</body>`, so it is in BOTH the live DOM
      // and the served bytes of every rebuild.
      const bridgeScript =
        `<script type="text/javascript" id="moss-bridge" data-moss-permanent>${bundle}</script>`;
      const served = (text: string) =>
        `<html><head></head><body><p id="content">${text}</p>${bridgeScript}</body></html>`;

      document.body.innerHTML = `<p id="content">v0</p>${bridgeScript}`;
      const bridge = document.getElementById("moss-bridge")!;
      // Keystone stamp: survives only if the node object itself is reused, i.e.
      // the bridge IIFE was never re-executed.
      (bridge as unknown as { __alive: boolean }).__alive = true;
      const tagsBefore = Array.from(document.body.children).map((el) => el.tagName);

      // Three consecutive rebuilds: the pre-fix nesting compounded one level per
      // morph, so a single-morph assertion under-tests it.
      for (const version of ["v1", "v2", "v3"]) {
        applyMorph(served(version));

        // The whole point: the page stays where CSS expects it. Pre-fix,
        // document.body.children became exactly ["HEAD", "BODY"] and every
        // `body[...]`/`body:not([...])` descendant rule became a lie — most
        // loudly the `.moss-subscribe-form` hide rule in email.css.
        expect(document.body.querySelector("body")).toBeNull();
        expect(document.body.querySelector("head")).toBeNull();
        expect(Array.from(document.body.children).map((el) => el.tagName)).toEqual(
          tagsBefore,
        );
        // …and the morph still did its job.
        expect(document.getElementById("content")!.textContent).toBe(version);
      }

      // G3 keystone holds across all three morphs.
      const after = document.getElementById("moss-bridge")!;
      expect(after).toBe(bridge);
      expect((after as unknown as { __alive?: boolean }).__alive).toBe(true);
    },
  );
});

describe("script-change diagnostics", () => {
  const parse = (html: string) =>
    new DOMParser().parseFromString(html, "text/html");

  test("differingScriptKeys names the culprit, and agrees with scriptsDiffer", () => {
    // The verdict alone was undiagnosable in the log: a reload firing on every
    // rebuild of an unedited page read identically to a real theme edit.
    const live = parse(
      `<body><script src="/a.ABC.js"></script><script src="/b.js"></script></body>`,
    );
    const next = parse(
      `<body><script src="/a.DEF.js"></script><script src="/b.js"></script></body>`,
    );
    expect(differingScriptKeys(live, next)).toEqual(["src:/a.DEF.js"]);
    expect(scriptsDiffer(live, next)).toBe(true);
    // Emptiness IS the predicate — the two can never disagree about what tripped.
    expect(differingScriptKeys(live, live)).toEqual([]);
    expect(scriptsDiffer(live, live)).toBe(false);
    // What the bridge actually calls: descriptors, or null to proceed with the morph.
    expect(describeScriptChange(live, next)).toEqual(["src:/a.DEF.js"]);
    expect(describeScriptChange(live, live)).toBeNull();
  });

  test("describeScriptKey bounds an inline key to a loggable length", () => {
    // moss emits a ~5 KB subscribe bundle inline, so the raw key is the whole
    // body. The byte length is what separates two inline scripts sharing a head.
    const body = `window.x=1;${"/* pad */".repeat(200)}`;
    const short = describeScriptKey(`inline:${body}`);
    expect(short).toMatch(new RegExp(`^inline\\[${body.length}B\\]:window\\.x=1;`));
    expect(short.length).toBeLessThan(120);
    // Whitespace is collapsed so a multi-line body stays one log line.
    expect(describeScriptKey("inline:  a\n\n  b  ")).toBe("inline[10B]:a b");
    // An external key is already a path — passed through untouched.
    expect(describeScriptKey("src:/theme/s.ABC.js")).toBe("src:/theme/s.ABC.js");
  });
});

describe("scriptsDiffer (script-change → reload guard)", () => {
  const parse = (html: string) =>
    new DOMParser().parseFromString(html, "text/html");

  test("identical scripts → no change (a content-only edit stays a morph)", () => {
    const a = parse(`<body><p>x</p><script src="/theme/s.ABC.js"></script></body>`);
    const b = parse(`<body><p>y</p><script src="/theme/s.ABC.js"></script></body>`);
    expect(scriptsDiffer(a, b)).toBe(false);
  });

  test("a changed content-hash src → changed (force a reload)", () => {
    const a = parse(`<body><script src="/theme/s.ABC.js"></script></body>`);
    const b = parse(`<body><script src="/theme/s.DEF.js"></script></body>`);
    expect(scriptsDiffer(a, b)).toBe(true);
  });

  test("a genuinely new served script → changed", () => {
    const before = parse(`<body><script src="/a.js"></script></body>`);
    const after = parse(
      `<body><script src="/a.js"></script><script src="/b.js"></script></body>`,
    );
    expect(scriptsDiffer(before, after)).toBe(true);
  });

  test("a REMOVED served script → not flagged (its run code is harmless; clears on next reload)", () => {
    const before = parse(
      `<body><script src="/a.js"></script><script src="/b.js"></script></body>`,
    );
    const after = parse(`<body><script src="/a.js"></script></body>`);
    expect(scriptsDiffer(before, after)).toBe(false);
  });

  test("a changed inline (executable) script → changed", () => {
    const a = parse(`<body><script>window.x=1</script></body>`);
    const b = parse(`<body><script>window.x=2</script></body>`);
    expect(scriptsDiffer(a, b)).toBe(true);
  });

  test("a changed non-executable data block (JSON-LD) → NOT a reload", () => {
    // moss emits <script type="application/ld+json"> carrying title/description/
    // date; editing the title changes its text every build. It is DATA, not
    // code — a morph updates it in place. Reloading here would defeat the morph
    // on the most common edits.
    const a = parse(
      `<body><script type="application/ld+json">{"name":"Old"}</script></body>`,
    );
    const b = parse(
      `<body><script type="application/ld+json">{"name":"New"}</script></body>`,
    );
    expect(scriptsDiffer(a, b)).toBe(false);
  });

  test("a runtime-injected script (in live, never served) → NOT a reload storm", () => {
    // A theme may document.createElement('script') for analytics/embeds — in the
    // LIVE doc but never in served bytes. An ordered/length diff would reload on
    // every morph; the subset check ignores live-only scripts.
    const live = parse(
      `<body><script src="/theme.ABC.js"></script><script src="/analytics.js"></script></body>`,
    );
    const served = parse(`<body><script src="/theme.ABC.js"></script></body>`);
    expect(scriptsDiffer(live, served)).toBe(false);
    // …yet a real theme edit is still caught even with an injected script present.
    const servedEdited = parse(`<body><script src="/theme.DEF.js"></script></body>`);
    expect(scriptsDiffer(live, servedEdited)).toBe(true);
  });

  test("permanent (bridge) scripts are excluded — never a change", () => {
    // The bridge <script> is [data-moss-permanent]; even if its inline body
    // differed between the live doc and the served page, it must not force a
    // reload (it is moss-managed, not site content).
    const a = parse(
      `<body><script id="moss-bridge" data-moss-permanent>A</script></body>`,
    );
    const b = parse(
      `<body><script id="moss-bridge" data-moss-permanent>B-different</script></body>`,
    );
    expect(scriptsDiffer(a, b)).toBe(false);
  });
});

describe("iframe-bridge morph handler (shipped bundle regression)", () => {
  if (!bundleExists) {
    // eslint-disable-next-line no-console
    console.warn(
      `[iframe-bridge-morph] Skipping bundle regression test: ${BUNDLE_PATH} not found. Run \`pnpm run frontend:build\` to generate it.`,
    );
  }

  it.runIf(bundleExists)("bundle carries the morph + keystone shape", () => {
    const src = readFileSync(BUNDLE_PATH, "utf8");
    // Morph message protocol.
    expect(src).toContain("moss-morph-applied");
    expect(src).toContain("moss-morph-failed");
    // Post-morph re-init hook: an in-document CustomEvent (distinct from the
    // cross-frame postMessage protocol above) so site scripts like theme.ts
    // can re-scan for interactive markup that idiomorph just created fresh
    // (e.g. the font panel appearing for the first time after `date:`
    // frontmatter is added mid-session — see theme-post-morph-reinit.test.ts).
    expect(src).toContain("moss-morph-patched");
    // Keystone markers: the permanent-node veto + the init-count probe.
    expect(src).toContain("data-moss-permanent");
    expect(src).toContain("__bridgeInitCount");
    // Fix #1: a changed site <script> forces a full reload rather than a silent
    // no-op morph (idiomorph reuses matched <script> nodes → no re-exec). The
    // bridge posts moss-morph-failed with this marker so the shell reloads.
    // (No bundle marker for Fix #2's beforeNodeRemoved veto: idiomorph ships its
    // own default `beforeNodeRemoved` key, so a substring check can't tell ours
    // apart — the local-harness "overlay survives" test guards that behavior.)
    expect(src).toContain("script-changed");
    // WebKit-safe shape: idiomorph is invoked with an explicit `morphStyle`
    // option (the head/body innerHTML morph), not a bare documentElement morph.
    // `morphStyle` is a specific idiomorph option key (survives minification),
    // so this is a sharper guard than a bare "innerHTML" substring would be.
    expect(src).toContain("morphStyle");
    // <details open> veto: the beforeAttributeUpdated callback must be present
    // to prevent user-opened state from being wiped on every morph.
    // "DETAILS" is a safe substring — esbuild minifies but doesn't rename
    // string literals; the tagName comparison always uses the uppercase form.
    expect(src).toContain("DETAILS");
    // idiomorph createNode patch: id-bearing nodes are recreated via
    // createElementNS keyed on `.localName` (correct case in every namespace),
    // NOT `.tagName` (uppercase for HTML → collapses type-selector layout).
    // Scope the negative check to the createElementNS call — idiomorph uses
    // `.tagName` legitimately elsewhere (id-set tag comparisons).
    expect(src).toMatch(/createElementNS\([^)]*\.localName\)/);
    expect(src).not.toMatch(/createElementNS\([^)]*\.tagName\)/);
  });

  it.runIf(bundleExists)("bundle carries the 404-marker + retry reporting", () => {
    const src = readFileSync(BUNDLE_PATH, "utf8");
    // The shell reads this server-stamped marker to detect the synthetic 404
    // and retry a rename that raced the generation swap (preview-actions.ts
    // `decideRenameRetry`). If this is missing, the bundle is stale — re-run
    // `node scripts/build-backend-scripts.mjs`.
    expect(src).toContain("moss-not-found");
    // notifyNavigation must report the notFound flag to the parent.
    expect(src).toContain("notFound");
  });
});
