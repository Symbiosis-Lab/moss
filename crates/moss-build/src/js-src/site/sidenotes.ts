/**
 * Footnotes beside the text, or as one liftable sheet — the two presentations
 * of the same notes, chosen by width.
 *
 * Wide (a real gutter): a copy of each note floats in the right margin beside
 * the line that cites it. The placement is the browser's, not ours: `float:
 * right` + `clear: right` stacks close-together notes downward on their own,
 * reflowing for free on resize, font-scale change and print. Once every note
 * has a margin twin, the endnote list at the article's foot retires
 * (`moss-footnotes-cloned`, display:none under the same media scope that
 * shows the margin) — the design's rule that a reader meets each note exactly
 * once. The retirement is atomic: one unclonable note and the class never
 * lands, so the list stays and nothing is lost.
 *
 * Narrow (no gutter): the endnote list at the page bottom IS the interface —
 * one surface with two homes. A marker tap lifts the SAME section as a
 * bottom sheet (`position: sticky; bottom: 0` + a height clip), peeking
 * exactly the tapped note; dragging up expands it to a scrollable glossary at
 * 62% of the viewport. Nothing is cloned, opened, or re-parented: lifting is
 * a class, and scrolling the page to the section's true place docks it back
 * into flow — the same element, so the dock is a layout identity, not a
 * choreographed handover.
 *
 * The sheet is non-modal: no scrim, and the page scrolls freely behind it —
 * that freedom is what lets the reader scroll to the end and watch the sheet
 * become the list again. Esc, tapping the article, or dragging down dismiss.
 *
 * Ships as its own bundle, gated on `SiteAssets::has_footnotes` — a site
 * with no footnotes serves none of this (`emit::scripts::SITE_SCRIPTS`).
 */

import { langBucket, type Lang } from "./subscribe/i18n";

/** Section heading + the grabber's accessible name. Same hand-kept table
 * pattern as `selection-actions.ts`. */
const NOTES_TITLE: Record<Lang, string> = {
  en: "Notes",
  "zh-hans": "注释",
  "zh-hant": "註釋",
};
const ALL_NOTES: Record<Lang, string> = {
  en: "All notes",
  "zh-hans": "全部注释",
  "zh-hant": "全部註釋",
};

/** The endnote's content, stripped of what only the canonical copy should own. */
function noteBody(note: HTMLElement): Node[] {
  const clone = note.cloneNode(true) as HTMLElement;
  // The return arrow points home from the endnote list; from the margin it
  // would point at the line the reader is already on.
  clone
    .querySelectorAll("a.moss-footnote-backref")
    .forEach((a) => a.remove());
  // Every id in here already exists on the canonical note. Taking the
  // children rather than the clone itself covers the root's own id too.
  clone.querySelectorAll("[id]").forEach((el) => el.removeAttribute("id"));
  return [...clone.childNodes];
}

export function initSidenotes(): void {
  const bucket = langBucket(document.documentElement.lang);
  const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

  /** Canonical `li` → its first margin aside. */
  const cloned = new Map<HTMLElement, HTMLElement>();
  /** Lifts the sheet at a note's peek — set once the sheet exists. */
  let liftAt: ((note: HTMLElement) => void) | null = null;
  /** The one currently washed element; the arrival cue names ONE landing. */
  let landed: HTMLElement | null = null;
  const wash = (el: HTMLElement) => {
    landed?.classList.remove("moss-footnote-landed");
    void el.offsetWidth; // restart the animation when re-landing
    el.classList.add("moss-footnote-landed");
    landed = el;
  };

  document
    .querySelectorAll<HTMLElement>(".moss-footnote-ref")
    .forEach((ref, i) => {
      const link = ref.querySelector("a");
      const href = link?.getAttribute("href") ?? "";
      if (!link || !href.startsWith("#")) return;
      const note = document.getElementById(href.slice(1));
      if (!note) return;

      const aside = document.createElement("aside");
      aside.className = "moss-sidenote";
      aside.id = `moss-sidenote-${i + 1}`;
      // Announced as the marker's description on focus — the design's
      // marker-driven pairing; focus itself never leaves the text.
      link.setAttribute("aria-describedby", aside.id);

      const number = document.createElement("span");
      number.className = "moss-sidenote-number";
      number.textContent = ref.textContent ?? "";
      aside.append(number, ...noteBody(note));

      ref.after(aside);
      if (!cloned.has(note)) cloned.set(note, aside);

      // Which presentation this marker leads to, asked of the cascade at
      // the moment of the tap. Both handlers below must agree, so they read
      // one predicate: a margin copy on screen means the link navigates and
      // the hashchange lands on the aside; no margin (and a sheet to lift)
      // means the sheet is the destination. A note outside the endnote
      // section has no sheet, and plain #fn-N navigation is the honest
      // fallback.
      const sheetIsHome = () =>
        liftAt !== null && getComputedStyle(aside).display === "none";
      // A mouse/touch tap must not FOCUS the link: WebKit later scrolls a
      // focused element to its flow position (deferred, ~0.5s) even when
      // it is already visible, yanking the page while the sheet peeks.
      // Keyboard focus (Tab) never passes through mousedown and keeps
      // working.
      link.addEventListener("mousedown", (e) => {
        if (sheetIsHome()) e.preventDefault();
      });
      link.addEventListener("click", (e) => {
        if (!sheetIsHome()) return;
        e.preventDefault();
        liftAt!(note);
      });
    });

  /* ---- the sheet ------------------------------------------------------ */

  const section = document.querySelector<HTMLElement>(
    "section.moss-footnotes",
  );
  const notes = section
    ? [...section.querySelectorAll<HTMLElement>("li[id]")]
    : [];
  if (!section || notes.length === 0) {
    window.addEventListener("hashchange", land);
    land();
    return;
  }
  // Atomic licence: only when every note has a margin twin may the list
  // retire at widths where the margin shows.
  if (notes.every((n) => cloned.has(n))) {
    section.classList.add("moss-footnotes-cloned");
  }

  // The section's heading — in FLOW, not only when lifted, so the docked
  // list and the sheet are the same pixels. JS-inserted because the pure
  // renderer has no language; the label table above does.
  const title = document.createElement("h2");
  title.className = "moss-footnotes-title";
  title.textContent = NOTES_TITLE[bucket];
  section.prepend(title);

  // Lifted layout needs a grabber row above a scroll area; wrap the
  // section's content once at init (layout-neutral in flow).
  const scroller = document.createElement("div");
  scroller.className = "moss-footnote-scroller";
  scroller.append(...section.childNodes);
  // `list-style: none` makes Safari/VoiceOver drop the list semantics;
  // the explicit role keeps them.
  scroller.querySelector("ol")?.setAttribute("role", "list");
  const grab = document.createElement("button");
  grab.className = "moss-footnote-grab";
  grab.setAttribute("aria-label", ALL_NOTES[bucket]);
  section.append(grab, scroller);

  // The sheet's flow slot: one zero-height div just before the section,
  // doing two jobs. Lifting clips the section's height — and a sticky
  // element keeps its flow box, so that clip is also the DOCUMENT's
  // length. Left alone the page shortens by the whole list the instant a
  // note is tapped and grows back over the settle, resizing the scrollbar
  // every frame and clamping the scroll near the foot. The slot grows by
  // exactly what the clip took, so the page is the same length lifted as
  // docked and the lift is a paint change, not a reflow. Its bottom edge
  // is then also the section's flow top, which is what tells stuck from
  // docked — measured in ONE coordinate space, immune to the
  // mobile-toolbar viewport ambiguity.
  const slot = document.createElement("div");
  // Scroll anchoring would read the slot's growth and the section leaving
  // flow as content shifting under the reader, and "correct" it by
  // scrolling the page — hundreds of pixels, on a tap that asked for none.
  slot.style.overflowAnchor = "none";
  section.before(slot);
  let flowH = 0; // the flow the clip took, measured at the lift

  /** The sheet's state: one record, all derived geometry beside it. */
  let cur: HTMLElement | null = null;
  let lifted = false;
  let h = 0; // current clip height
  let hTarget = 0; // where the height is settling to (== h at rest)
  const geo = { hPeek: 0, hFull: 0, sPeek: 0, sFull: 0 };

  const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
  const t = () =>
    Math.min(1, Math.max(0, (h - geo.hPeek) / (geo.hFull - geo.hPeek || 1)));

  // Peek clips to exactly the note (+ the list's own 8px rhythm); expanded
  // stops at 62% of the viewport so a readable band of article survives
  // above. Both scroll offsets travel one shared path, so the note glides
  // from filling the peek to resting a third down with its neighbours
  // streaming in — no reflow, no content swap.
  function geometry(li: HTMLElement) {
    const grabH = grab.offsetHeight;
    geo.hFull = Math.min(
      scroller.scrollHeight + grabH,
      0.62 * window.innerHeight,
    );
    geo.hPeek = Math.min(
      li.offsetHeight + grabH + 16,
      0.5 * window.innerHeight,
    );
    // offsetTop resolves against the sticky section (the positioned
    // ancestor), not the scroll content; subtract the scroller's offset.
    const yN = li.offsetTop - scroller.offsetTop;
    const maxS = (hh: number) =>
      Math.max(0, scroller.scrollHeight - (hh - grabH));
    geo.sPeek = Math.min(Math.max(0, yN - 8), maxS(geo.hPeek));
    geo.sFull = Math.min(
      Math.max(0, yN - (geo.hFull - grabH) / 3),
      maxS(geo.hFull),
    );
  }

  function apply() {
    section!.style.height = `${h}px`;
    slot.style.height = `${Math.max(0, flowH - h)}px`;
    scroller.scrollTop = lerp(geo.sPeek, geo.sFull, t());
    scroller.toggleAttribute("data-scrolly", h >= geo.hFull - 1);
  }

  function lift(li: HTMLElement) {
    cur = li;
    if (!lifted) {
      // How much flow the clip takes is asked of the document, not derived
      // from boxes: the docked list's inner margins collapse out through
      // it, so no offsetHeight sees the whole footprint. Provision the slot
      // generously first — a document briefly too LONG costs nothing, while
      // one briefly too short clamps the reader's scroll position and never
      // gives it back — then correct to exact. Two forced layouts, once per
      // tap; from here the slot holds the difference.
      const doc0 = document.documentElement.scrollHeight;
      lifted = true;
      section!.classList.add("moss-footnotes-lifted");
      h = 0.01;
      flowH = h + section!.offsetHeight + window.innerHeight;
      geometry(li);
      apply();
      flowH += doc0 - document.documentElement.scrollHeight;
    }
    geometry(li);
    // Apply in the same task as the class change: the class zeroes the
    // section's flow margins, and a frame that paints before the slot
    // is sized is a frame where the page is shorter.
    apply();
  }

  function putBack() {
    cancelAnimationFrame(anim);
    lifted = false;
    section!.classList.remove("moss-footnotes-lifted");
    section!.style.height = "";
    slot.style.height = "";
    section!.style.removeProperty("--moss-sheet-away");
    scroller.removeAttribute("data-scrolly");
    cur = null;
  }

  let anim = 0;
  function settleTo(target: number, done?: () => void) {
    cancelAnimationFrame(anim);
    hTarget = target;
    if (reduced()) {
      h = target;
      apply();
      done?.();
      checkDock();
      return;
    }
    const from = h;
    const d = target - from;
    const t0 = performance.now();
    const step = (now: number) => {
      const p = Math.min(1, (now - t0) / 420);
      h = from + d * (1 - Math.pow(1 - p, 3.2));
      apply();
      if (p < 1) anim = requestAnimationFrame(step);
      else {
        done?.();
        checkDock();
      }
    };
    anim = requestAnimationFrame(step);
  }
  function sink() {
    const back = cur; // focus returns to the marker the sheet came from
    settleTo(0, () => {
      putBack();
      if (back) {
        document
          .querySelector<HTMLElement>(`a[href="#${back.id}"]`)
          ?.closest<HTMLElement>(".moss-footnote-ref")
          ?.focus({ preventScroll: true });
      }
    });
  }

  // Dock: the sheet is the bottom list, so reaching the list's true place
  // makes them one again. Over the last 240px of approach the chrome fades
  // (`--moss-sheet-away`) and the inner scroll winds back to the list's
  // head — so at offset zero the stuck strip and the in-flow strip are the
  // same pixels and the release is a no-op, not a content swap. The pixel
  // anchor absorbs the lifted-vs-flow padding difference; the release only
  // ever GROWS the document (shrinking near the end triggers scroll
  // clamping, the one real jump hazard).
  // Docking is a USER action by definition. WebKit emits scrolls the
  // reader never made — a deferred reveal of a focused element (~0.5s
  // late, to its FLOW position at the document end), clamp scrolls while
  // the clip animates — and any of them can land where `off ≈ 0` and
  // silently dock a sheet the reader is using. So the dock listens only in
  // the wake of real input; everything else leaves the sheet alone.
  let intentAt = -1e4;
  const intent = () => {
    intentAt = performance.now();
  };
  // Scrolling gestures only — a TAP must not open the window, or the
  // deferred focus-reveal that follows it walks right through.
  addEventListener("wheel", intent, { passive: true, capture: true });
  addEventListener("touchmove", intent, { passive: true, capture: true });
  addEventListener("keydown", intent, { capture: true });

  let zoneS: number | null = null;
  function checkDock() {
    // Only at rest: a settle reshapes the document, and at full page
    // scroll the clamp-scrolls it emits read as `off ≈ 0` — the lift
    // would dock itself. A user's dock-scroll always happens at rest. The
    // settle therefore calls this again when it lands, because the scroll
    // that reached the dock point can end before the clip does.
    if (!lifted || Math.abs(h - hTarget) > 1) return;
    if (performance.now() - intentAt > 1500) return;

    // Measure against the sheet's SETTLED top, not its animating one — a
    // hard scroll during the lift settle would otherwise read the still-
    // tiny sheet as already docked and release it.
    const off =
      slot.getBoundingClientRect().bottom -
      (window.innerHeight - hTarget);
    const k = Math.min(1, Math.max(0, off / 240));
    section!.style.setProperty("--moss-sheet-away", String(k));
    if (k < 1) {
      if (zoneS === null) zoneS = scroller.scrollTop;
      scroller.scrollTop = zoneS * k;
    } else zoneS = null;
    if (off < 1) {
      zoneS = null;
      const anchor = notes[0];
      const before = anchor.getBoundingClientRect().top;
      putBack();
      scrollTo({
        top: scrollY + (anchor.getBoundingClientRect().top - before),
        behavior: "instant",
      });
    }
  }
  window.addEventListener("scroll", checkDock, { passive: true });

  // Drag — the gesture leaves the sheet's box immediately on an upward
  // pull, so move/up listen on the window for the pointer's lifetime. An
  // 8px slop separates a drag from a tap, so clicks inside the note
  // survive. At full height the inner list scrolls natively; a downward
  // drag takes the sheet only from the top of that scroll.
  let y0: number | null = null;
  let h0 = 0;
  let vel = 0;
  let yPrev = 0;
  let tPrev = 0;
  let dragging = false;
  let s0 = 0;
  let downTarget: EventTarget | null = null;
  const mv = (e: PointerEvent) => {
    if (y0 === null) return;
    const dy = e.clientY - y0;
    if (!dragging) {
      if (Math.abs(dy) < 8) return;
      const inner = h >= geo.hFull - 1 && !grab.contains(downTarget as Node);
      if (inner && !(dy > 0 && s0 <= geo.sFull + 1)) {
        end();
        return;
      }
      dragging = true;
    }
    if (e.cancelable) e.preventDefault();
    const dt = e.timeStamp - tPrev || 16;
    vel = 0.8 * vel + 0.2 * ((e.clientY - yPrev) / dt);
    yPrev = e.clientY;
    tPrev = e.timeStamp;
    let target = h0 - dy;
    if (target > geo.hFull) target = geo.hFull + (target - geo.hFull) * 0.25;
    h = Math.max(0, target);
    apply();
  };
  const up = () => {
    if (y0 === null) return;
    const wasDrag = dragging;
    end();
    if (!wasDrag) return;
    const fling = Math.abs(vel) > 0.4;
    if (fling ? vel < 0 : h > (geo.hPeek + geo.hFull) / 2) {
      settleTo(geo.hFull);
    } else if ((fling && vel > 0) || h < geo.hPeek * 0.7) sink();
    else settleTo(geo.hPeek);
  };
  function end() {
    y0 = null;
    dragging = false;
    removeEventListener("pointermove", mv);
    removeEventListener("pointerup", up);
    removeEventListener("pointercancel", up);
  }
  liftAt = (note) => {
    lift(note);
    settleTo(geo.hPeek);
  };

  section.addEventListener("pointerdown", (e) => {
    if (!lifted) return;
    cancelAnimationFrame(anim);
    y0 = yPrev = e.clientY;
    tPrev = e.timeStamp;
    h0 = h;
    vel = 0;
    s0 = scroller.scrollTop;
    dragging = false;
    downTarget = e.target;
    addEventListener("pointermove", mv, { passive: false });
    addEventListener("pointerup", up);
    addEventListener("pointercancel", up);
  });

  // The grabber: the drag's destination, equally there by tap, keyboard
  // and screen reader.
  grab.addEventListener("click", () => {
    if (h >= geo.hFull - 1) return;
    // No wash: expanding shows the note, and a cue for something the
    // reader can already see is one animation too many. The wash is kept
    // for arrivals the reader cannot see coming — a backref home, a deep
    // link into the margin.
    settleTo(geo.hFull);
  });

  // The arrival cue must be SEEN arriving: after a smooth scroll, the wash
  // starts only once the scroll has come to rest.
  function afterScroll(fn: () => void) {
    if (reduced()) {
      fn();
      return;
    }
    let timer = setTimeout(finish, 150);
    function onScroll() {
      clearTimeout(timer);
      timer = setTimeout(finish, 100);
    }
    function finish() {
      removeEventListener("scroll", onScroll);
      fn();
    }
    addEventListener("scroll", onScroll, { passive: true });
  }

  // The only control inside the sheet is the backref. A note is text to
  // read, not a target to hit: making one clickable would put a silent
  // hitbox under every line the reader is in the middle of.
  section.addEventListener("click", (e) => {
    const back = (e.target as Element).closest<HTMLAnchorElement>(
      "a.moss-footnote-backref",
    );
    if (back && lifted) {
      // A backref is a departure: the sheet sinks and the page travels
      // home, washing the marker it returns to.
      e.preventDefault();
      const mark = document.getElementById(
        back.getAttribute("href")?.slice(1) ?? "",
      );
      // Release instantly rather than animating the sink: shrinking the
      // sheet shrinks the document while the page sits against its end,
      // and each frame's scroll clamp would cancel the smooth travel home.
      putBack();
      if (mark) {
        mark.scrollIntoView({
          block: "center",
          behavior: reduced() ? "auto" : "smooth",
        });
        // Focus once the travel rests — focusing mid-flight would let
        // WebKit's async focus scroll cancel the smooth travel.
        afterScroll(() => {
          wash(mark);
          mark.focus({ preventScroll: true });
        });
      }
    }
  });

  // The page stays live behind the sheet: scrolling never dismisses; a
  // discrete tap outside it does (no scrim — the page is not modal).
  document.addEventListener("click", (e) => {
    if (
      lifted &&
      !section!.contains(e.target as Node) &&
      !(e.target as Element).closest(".moss-footnote-ref")
    )
      sink();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && lifted) sink();
  });

  /** A `#fn-N` arrival lands on whichever presentation of the note exists. */
  function land() {
    const id = location.hash.slice(1);
    const note = /^fn-/.test(id) ? document.getElementById(id) : null;
    const aside = note && cloned.get(note);
    if (!note) return;
    if (aside && getComputedStyle(aside).display !== "none") {
      // Margin rendered: wash it, and scroll only when it is off-screen —
      // a marker click already has it beside the pointer.
      wash(aside);
      const r = aside.getBoundingClientRect();
      if (r.top < 0 || r.bottom > window.innerHeight)
        aside.scrollIntoView({ block: "center" });
    } else if (liftAt && notes.includes(note)) {
      liftAt(note);
    }
  }
  window.addEventListener("hashchange", land);
  land();
}

initSidenotes();
