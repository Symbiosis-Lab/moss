// The base every demo surface element extends. Owns everything a documentation page's one real
// moss interface needs regardless of which surface it frames: the iframe, one load at a time (a
// single AbortController per load), theme + locale sync, the pointer overlay, playing a scene
// through player.js/driver.js, reader input stopping playback at once, the WebKit scroll backstop,
// and announcing state to whichever marker is listening. Knows nothing about where markers are,
// how the page is laid out, or anything surface-specific — see site/ui/demo/README.md's module
// table.
//
// A concrete element (site/ui/demo/moss-editor-demo.js today) subclasses this and supplies one
// surface adapter (site/ui/demo/surfaces/*.js — the interface is documented at the top of
// surfaces/editor.js): `get surface()` returning its exports. Nothing here imports a surface
// module directly, which is what lets a second surface (a `<moss-preview-demo>`, say) reuse this
// base with zero changes to this file — only a new surfaces/*.js and a new custom element.
import { playChain } from './player.js';
import { createDriver } from './driver.js';
import { string } from './strings.js';

const MODULE_BASE = new URL('.', import.meta.url);

if (!document.querySelector('link[data-moss-demo]')) {
  const link = document.createElement('link');
  link.rel = 'stylesheet';
  link.href = new URL('demo.css', MODULE_BASE).href;
  link.dataset.mossDemo = '';
  document.head.appendChild(link);
}

const sceneCache = new Map();

function loadSceneData(name) {
  let cached = sceneCache.get(name);
  if (!cached) {
    cached = fetch(new URL(`scenes/${name}.json`, MODULE_BASE)).then((res) => {
      if (!res.ok) throw new Error(`Scene "${name}" failed to load: ${res.status}`);
      return res.json();
    });
    // A rejected promise stays rejected forever, so caching a failed fetch would wedge this scene
    // for the rest of the page's life — evict on failure so pressing the (still-enabled) button
    // again actually retries the network request instead of replaying the same rejection.
    cached.catch(() => sceneCache.delete(name));
    sceneCache.set(name, cached);
  }
  return cached;
}

/**
 * Walks `scene.after` (site/ui/demo/README.md, "Scene format"), loading each ancestor by name,
 * and returns the chain ordered eldest ancestor first, `name`/`scene` — the one actually requested
 * — last: the order player.js's `playChain` runs in. This is the "scene loading" half of `after`
 * composition (the module table in site/ui/demo/README.md); running the resolved chain is
 * player.js's job. Throws loudly if a name repeats: an `after` cycle would otherwise fetch forever.
 */
async function resolveChain(name, scene) {
  const ancestors = [];
  const seen = new Set([name]);
  let current = scene;
  while (current.after) {
    const nextName = current.after;
    if (seen.has(nextName)) {
      throw new Error(`Scene "${name}"'s "after" chain cycles back to "${nextName}"`);
    }
    seen.add(nextName);
    const nextScene = await loadSceneData(nextName);
    ancestors.unshift({ name: nextName, scene: nextScene });
    current = nextScene;
  }
  return [...ancestors, { name, scene }];
}

/** The page's own effective theme (site/ui/demo/README.md, "Fitting the page"). This site's toggle
 * (theme.ts) always sets `data-theme` pre-paint and never changes it on its own — see
 * crates/moss-build/src/js-src/site/theme.ts — so the attribute is the only source. */
function pageTheme() {
  return document.documentElement.dataset.theme || 'light';
}

/** Reports a named scene's phase to whichever marker is listening (site/ui/demo/README.md, "Who
 * controls the demo"). The frame never looks for markers; this is the only channel back. */
function announce(name, phase) {
  document.dispatchEvent(new CustomEvent('moss-demo-state', { detail: { name, phase } }));
}

/** The base class every `<moss-*-demo>` element extends. A subclass supplies exactly one thing —
 * `get surface()`, the adapter module (site/ui/demo/surfaces/*.js) — and inherits everything else:
 * lifecycle, loading, pace, and the "reader outranks the script" rules in site/ui/demo/README.md. */
export class DemoFrameElement extends HTMLElement {
  connectedCallback() {
    this.locale = (document.documentElement.lang || 'en').toLowerCase();
    this.reducedQuery = matchMedia('(prefers-reduced-motion: reduce)');
    this.reduced = this.reducedQuery.matches;
    this.phase = 'loading'; // loading | playing | error | done
    this.load = null; // { controller, name, scene } for a named scene; null while showing the default fixture
    this.handle = null; // the surface's own ready handle (surface.waitForReady's resolved value)
    this.scrollGuardY = null; // set for the duration of a 'playing' scene on the sticky layout only; see guardScrollFrame

    this.innerHTML = `
      <div class="moss-demo__frame-wrap">
        <iframe class="moss-demo__frame" title="${string(this.locale, 'frame')}" loading="lazy"></iframe>
        <div class="moss-demo__pointer" hidden aria-hidden="true">
          <span class="moss-demo__pointer-ring"></span>
          <span class="moss-demo__pointer-glyph"></span>
          <span class="moss-demo__pointer-dot"></span>
        </div>
      </div>`;

    this.frame = this.querySelector('.moss-demo__frame');
    this.frameWrap = this.querySelector('.moss-demo__frame-wrap');
    this.pointerEl = this.querySelector('.moss-demo__pointer');

    this.driver = createDriver({
      frame: this.frame,
      frameWrap: this.frameWrap,
      pointerEl: this.pointerEl,
      getHandle: () => this.handle,
      findTarget: this.surface.findTarget,
    });

    this.hostController = new AbortController();
    const { signal } = this.hostController;
    document.addEventListener('moss-demo-play', this.onScenePlayEvent, { signal });
    document.addEventListener('moss-demo-stop', this.onSceneStopEvent, { signal });
    this.reducedQuery.addEventListener('change', this.onReducedChange, { signal });
    // Synchronous half of guardScrollFrame's own job (see its comment): a 'scroll' event fires
    // before the next paint, so correcting from it closes the gap further than the per-frame poll
    // alone — measured shrinking WebKit's residual jump from a ~10ms visible dip to one no
    // stillness sample has yet caught.
    window.addEventListener('scroll', this.onWindowScroll, { signal });

    // Theme follows the page (site/ui/demo/README.md, "Fitting the page"): re-applied to every
    // freshly loaded iframe document in loadFrame, and here for a page-driven change (the reader's
    // own toggle) to an already-loaded editor.
    this.themeObserver = new MutationObserver(() => this.syncTheme());
    this.themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });

    // "The frame is never empty" (site/ui/demo/README.md): loaded on connect, no marker involved. A
    // marker's own explicit play, if it arrives before this settles, supersedes it like any other
    // load (see startScene).
    this.defaultLoad = new AbortController();
    this.loadFrame(this.surface.DEFAULT_FIXTURE, this.defaultLoad.signal)
      .catch((error) => {
        if (!this.defaultLoad.signal.aborted) console.error('moss-demo: could not load the default fixture', error);
      });
  }

  disconnectedCallback() {
    this.hostController?.abort();
    this.frameListenerController?.abort();
    this.themeObserver?.disconnect();
    // Aborts the playing load's signal, which is what stops guardScrollFrame's own
    // requestAnimationFrame loop from rescheduling forever once the element is removed mid-scene
    // (see guardScrollFrame's own comment) — `isConnected` alone would still leave one more queued
    // frame to run before noticing.
    this.load?.controller.abort();
    this.defaultLoad?.abort();
  }

  onScenePlayEvent = (event) => this.startScene(event.detail.name);
  onSceneStopEvent = (event) => {
    if (this.load?.name !== event.detail.name || this.phase !== 'playing') return;
    this.load.controller.abort();
    this.phase = 'done';
    announce(event.detail.name, this.phase);
  };
  onReducedChange = (event) => { this.reduced = event.matches; };

  /**
   * One load at a time (site/ui/demo/README.md): starting any scene here abandons whichever load
   * was previously in flight or playing — including the connect-time default-fixture load — by
   * aborting its controller, so every one of that load's still-pending continuations finds its
   * own `controller.signal.aborted` and quits instead of clobbering this newer call's state. The
   * abandoned scene's own marker, if any, is told so it can stop showing Stop.
   *
   * An empty `name` ("just open the surface" — site/ui/demo/README.md, "Markdown markers") is not
   * a scene at all: no fetch, no fresh load, no Play/Stop toggle to track — it only brings the
   * already-loaded default fixture into view and focuses it, the same as a reader who never
   * clicked anything still gets a working editor to try (see "The frame is never empty").
   */
  async startScene(name) {
    if (name === '') {
      this.settleView();
      this.frame.focus({ preventScroll: true });
      announce(name, 'done');
      return;
    }

    this.defaultLoad?.abort();
    const previous = this.load;
    if (previous?.name === name && this.phase === 'playing') return; // a marker toggles via Stop, not a second Play
    previous?.controller.abort();
    if (previous && (this.phase === 'loading' || this.phase === 'playing')) announce(previous.name, 'done');

    const controller = new AbortController();
    this.load = { controller, name, scene: null };
    this.phase = 'loading';

    let scene;
    let chain;
    try {
      scene = await loadSceneData(name);
      chain = await resolveChain(name, scene);
      await this.loadFrame(scene.fixture ?? this.surface.DEFAULT_FIXTURE, controller.signal);
    } catch (error) {
      if (controller.signal.aborted) return;
      console.error('moss-demo: could not load scene', name, error);
      this.phase = 'error';
      announce(name, 'error');
      return;
    }
    if (controller.signal.aborted) return;

    this.load.scene = scene;
    this.load.chain = chain;
    // `open` is optional (site/ui/demo/README.md, "Scene format"): a scene that never names it
    // plays against whatever the fresh fixture load itself already contains — its own authored
    // piece — rather than being silently wiped to empty first. Discovered building the "versions"
    // scene: every scene up to then set `"open": ""` out of habit, which happened to be harmless
    // because none of them cared about the document's actual text, but the versions scene's own
    // "Save a version" step snapshots whatever text is live at that moment — wiping it first meant
    // it could only ever save and diff an empty document, no test having read that far until
    // check-docs-demo.mjs's checkVersionsDiffShowsOwnText did.
    if (scene.open !== undefined) this.driver.setText(scene.open);
    this.settleView();
    // "focus goes into the editor with preventScroll" (site/ui/demo/README.md, "Pressing Play
    // moves nothing") — moves keyboard/AT focus into the live demo on every Play, without
    // `focus()`'s own default of scrolling the target into view, which would be a second source of
    // motion on top of settleView above.
    this.frame.focus({ preventScroll: true });
    await this.runPlayback(controller.signal, name);
  }

  /** "Pressing Play moves nothing" (site/ui/demo/README.md): on the wide layout the frame sits in a
   * sticky column and is already in view, so nothing may scroll — not even a call meant to be a
   * no-op. `Element.scrollIntoView` cannot be trusted for that: on a `position: sticky` box it
   * computes the needed delta from the box's un-stuck, static-flow position rather than its
   * current stuck one, so once the reader has scrolled far enough for the frame to actually be
   * pinned, `{ block: 'nearest' }` "corrects" a scroll that was never wrong — confirmed by
   * measuring a ~60px scrollY jump on Play with the frame fully inside the viewport throughout.
   * `getComputedStyle().position` is the same "is this the narrow layout" signal the layout
   * checker already asserts (`checkNarrow`, "expected inline"), so wide vs. narrow is read from
   * the browser's own resolved style rather than duplicating the CSS breakpoint here. Only the
   * narrow (non-sticky) layout may scroll at all, and only when the frame is mostly out of the
   * viewport — then smoothly, never a hard jump. Focus is handled separately (bindReaderInteraction
   * / runPlayback) with `preventScroll` so it never becomes a second source of motion here. */
  settleView() {
    if (getComputedStyle(this).position === 'sticky') return; // wide layout: already in view
    const rect = this.getBoundingClientRect();
    const visibleTop = Math.max(rect.top, 0);
    const visibleBottom = Math.min(rect.bottom, window.innerHeight);
    const visibleHeight = Math.max(0, visibleBottom - visibleTop);
    const mostlyOutOfView = rect.height === 0 || visibleHeight < rect.height * 0.5;
    if (mostlyOutOfView) this.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }

  /** Every Play is a fresh load (site/ui/demo/README.md, "Who controls the demo"): a scene can
   * leave the app anywhere — versions mode, an open menu, a different file — and reloading the
   * iframe is the only reset that is true for all of them, so this always navigates rather than
   * checking whether anything would actually differ. Checks `signal` before touching `iframe.src`
   * at all, not only after the ready-wait: a load this call's own caller has already superseded
   * must not navigate the shared iframe out from under whichever load won, however late its own
   * fetch resolves. */
  async loadFrame(fixture, signal) {
    if (signal.aborted) return;
    this.handle = null;
    const ready = new Promise((resolve, reject) => {
      const onLoad = () => {
        this.frame.removeEventListener('load', onLoad);
        this.surface.waitForReady(this.frame.contentWindow, this.frame.contentDocument).then(resolve, reject);
      };
      this.frame.addEventListener('load', onLoad);
    });
    this.frame.src = this.surface.frameUrl(fixture, this.locale);
    const handle = await ready;
    if (signal.aborted) return;
    this.handle = handle;
    this.bindReaderInteraction();
    this.syncTheme();
  }

  /** Applies the page's current effective theme to the loaded iframe document, if any. A fresh
   * load starts from the harvest's own baked-in light default (frameUrl carries no theme param —
   * the fixture's query string is about content, not chrome), so this always needs to run once a
   * document is ready, not only on a later page-theme change. */
  syncTheme() {
    if (!this.frame.contentDocument?.documentElement) return;
    this.surface.setTheme(this.frame.contentDocument, pageTheme());
  }

  bindReaderInteraction() {
    this.frameListenerController?.abort();
    this.frameListenerController = new AbortController();
    const doc = this.frame.contentDocument;
    const { signal } = this.frameListenerController;
    doc.addEventListener('pointerdown', this.onEditorInteract, { capture: true, signal });
    doc.addEventListener('keydown', this.onEditorInteract, { capture: true, signal });
  }

  /** "The reader outranks the script" (site/ui/demo/README.md): any pointer or key press inside
   * the framed surface stops a playing scene at once and keeps what is there — Play always
   * restarts from the opening state rather than resuming. Idle browsing (no scene playing) is not
   * interrupted.
   *
   * `event.isTrusted` is what tells a genuine reader keypress apart from driver.js's own
   * `typeInto`/`setTextInto` committing a field with a synthetic `keydown` (a real `KeyboardEvent`
   * dispatched on an element inside this same iframe document, so it reaches this capture-phase
   * listener too) — without this check, a scene whose own typeInto step commits mid-playback
   * self-interrupts, silently dropping every step still to come. Only surfaced because the
   * "after"-chain case puts a typeInto step (save-as-template's name prompt) mid-sequence rather
   * than last: "new-from-template" plays its own two steps AFTER that commit, and until this guard
   * they never ran — the frame still reported 'done' with no error, since this same handler is
   * also what marks a genuinely-interrupted scene 'done'. A trusted event is real input; the
   * synthetic kind driver.js dispatches for its own verbs is never trusted. */
  onEditorInteract = (event) => {
    if (!event.isTrusted) return;
    if (this.phase !== 'playing' || !this.load) return;
    const name = this.load.name;
    this.load.controller.abort();
    this.phase = 'done';
    announce(name, this.phase);
  };

  /** Backstop for the harvested document's own focus patch, active only while a scene plays on
   * the wide/sticky layout: WebKit still moves the host page (measured: 51px, down from 434px
   * unpatched) when app-editor.html auto-focuses a field mid-scene, even with its own
   * `HTMLElement.prototype.focus` patched to force `preventScroll` (the desktop app's boot
   * parameters, scripts/landing/app-editor-boot-params.ts) — neither a 'focus' listener nor a patched
   * `scrollIntoView` on the iframe's own window ever fires for it, so whatever WebKit does here
   * bypasses both of the DOM APIs a script can intercept. Comparing `window.scrollY` against the
   * position captured once at the scene's own start is the one thing left that reaches a cause
   * this module cannot name, run from two places for two different failure windows: this 'scroll'
   * listener corrects synchronously, before the next paint, closing most of the gap;
   * `guardScrollFrame` below is the once-a-frame fallback for a drift that arrives some other way a
   * 'scroll' event doesn't cover. Neither can outlive its own run or fight a reader's own scroll
   * once a scene has finished: this one checks `phase === 'playing'` via `scrollGuardY`, only set
   * for that duration; `guardScrollFrame` below additionally carries the load's own signal, so
   * removing the element mid-scene (disconnectedCallback aborting that signal) stops its
   * `requestAnimationFrame` loop even if `phase` itself never gets the chance to change. */
  onWindowScroll = () => {
    if (this.phase !== 'playing' || this.scrollGuardY === null) return;
    if (window.scrollY !== this.scrollGuardY) window.scrollTo(window.scrollX, this.scrollGuardY);
  };

  /** `signal` is the playing load's own — passed through explicitly, not read off `this.load`,
   * because the guard must stop even if `this.load` has already moved on to a newer load by the
   * time a stale frame fires. Checks `isConnected` as well as the signal: disconnectedCallback
   * aborts `this.load`'s controller, but a call already in flight when that happens still needs
   * this frame's own read of `isConnected` to refuse rescheduling, rather than relying solely on
   * `phase` (which disconnectedCallback does not itself change) ever catching up. */
  guardScrollFrame = (signal) => {
    if (this.scrollGuardY === null) return;
    if (window.scrollY !== this.scrollGuardY) window.scrollTo(window.scrollX, this.scrollGuardY);
    if (this.phase === 'playing' && this.isConnected && !signal.aborted) {
      requestAnimationFrame(() => this.guardScrollFrame(signal));
    }
  };

  async runPlayback(signal, name) {
    this.phase = 'playing';
    // Only meaningful on the wide/sticky layout, where "moves nothing" is an unconditional
    // promise (site/ui/demo/README.md, "Pressing Play moves nothing"); null leaves
    // guardScrollFrame a no-op on the narrow layout, which is allowed to scroll.
    this.scrollGuardY = getComputedStyle(this).position === 'sticky' ? window.scrollY : null;
    if (this.scrollGuardY !== null) requestAnimationFrame(() => this.guardScrollFrame(signal));
    announce(name, this.phase);
    await playChain(this.load.chain, this.driver, {
      locale: this.locale,
      instant: this.reduced,
      signal,
    });
    if (signal.aborted) return; // onEditorInteract / onSceneStopEvent / a newer startScene already announced
    // Whether play() finished every step or the reader's own interruption already returned
    // 'paused' partway through, the marker reads the same either way — see moss-demo-marker.js's
    // onDemoState, which only ever distinguishes 'playing' and 'error' from everything else.
    this.phase = 'done';
    announce(name, this.phase);
  }
}
