/**
 * Sunlight Mode — "kindle under sunlight" ambient reading experience.
 *
 * Activation:
 *   - Long-press the theme toggle button (hold ≥500ms)
 *   - Keyboard: press [S] to toggle sunlight mode on/off
 *   - Default on first visit: activates automatically unless the user has
 *     already chosen a theme this session
 *
 * Deactivation:
 *   - Long-press the theme toggle again
 *   - Press [S] again (returns to light mode)
 *   - Short-click the light/dark theme toggle (normal toggle still works)
 *
 * How it works:
 *   Sets data-theme="sunlight" on <html>. All visual changes are handled by
 *   CSS selectors in style.css targeting [data-theme="sunlight"]. This script
 *   only manages activation state and video playback.
 *
 * Layer stack (when active):
 *   z-999: #sunlight-leaves — looping MP4 video, mix-blend-mode: multiply
 *   z-998: #sunlight-wash   — warm rgba overlay, mix-blend-mode: multiply
 *   body:  paper grain texture via background-image
 *   :root: warmer CSS custom property values
 */

(function () {
  "use strict";
  // ── Create overlay DOM elements ──
  // These elements exist in the DOM always but are invisible (opacity: 0)
  // until [data-theme="sunlight"] activates them via CSS.
  //
  // data-moss-permanent tells moss's live-preview morph to leave these nodes
  // alone: they're JS-appended and absent from the served HTML, so a body
  // morph would otherwise reconcile them away (the moss-morph-patched re-hook
  // below is the fallback for older moss builds that don't honor the marker).

  var video = document.createElement("video");
  video.id = "sunlight-leaves";
  video.setAttribute("data-moss-permanent", "");
  // moss injects window.mossTheme.base (absolute URL of the theme mount) before
  // this script, so the asset resolves wherever moss serves the theme — never
  // hardcode a site-root path like "/leaves.mp4".
  video.src = new URL("leaves.mp4", window.mossTheme.base).href;
  video.loop = true;
  video.muted = true;
  video.playsInline = true;
  video.preload = "none";
  document.body.appendChild(video);

  var wash = document.createElement("div");
  wash.id = "sunlight-wash";
  wash.setAttribute("data-moss-permanent", "");
  document.body.appendChild(wash);

  // ── State helpers ──

  var videoReadyTimer = null;
  var VIDEO_TIMEOUT_MS = 8000;
  // Mobile browsers (iOS Low-Power Mode, data-saver, strict autoplay) reject
  // muted autoplay, so video.play() fails on first load and the leaf video
  // never reaches .video-ready. We then retry on the first user gesture — a
  // tap satisfies the autoplay policy — so the leaves still appear on mobile.
  var gestureRetry = null;

  function onVideoPlaying() {
    clearTimeout(videoReadyTimer);
    videoReadyTimer = null;
    video.classList.add("video-ready");
    video.removeEventListener("playing", onVideoPlaying);
  }

  function disarmGestureRetry() {
    if (!gestureRetry) return;
    document.removeEventListener("pointerdown", gestureRetry);
    gestureRetry = null;
  }

  function armGestureRetry() {
    if (gestureRetry) return;
    gestureRetry = function () {
      disarmGestureRetry();
      if (!isSunlight()) return;
      // Re-arm the playing listener and retry now that a user gesture exists.
      video.addEventListener("playing", onVideoPlaying);
      video.play().catch(function () {});
    };
    // pointerdown covers touch + mouse; passive — we never preventDefault.
    document.addEventListener("pointerdown", gestureRetry, { passive: true });
  }

  function enterSunlight() {
    // Clean up any in-flight state from a prior call
    clearTimeout(videoReadyTimer);
    video.removeEventListener("playing", onVideoPlaying);

    // Phase 1: color shift + grain + warm wash (CSS-driven, instant)
    document.documentElement.setAttribute("data-theme", "sunlight");
    sessionStorage.setItem("theme", "sunlight");

    // Phase 2: video fades in only after actually playing
    // Reset for rapid re-entry: clear previous ready state before re-registering
    video.classList.remove("video-ready");
    video.addEventListener("playing", onVideoPlaying);
    video.play().catch(function () {
      // Autoplay blocked (common on mobile / iOS Low-Power Mode). Keep the
      // static layers and retry playback on the first user gesture so the
      // leaf video still appears on mobile after a tap.
      clearTimeout(videoReadyTimer);
      videoReadyTimer = null;
      armGestureRetry();
    });

    // Safety: if video hasn't started in 8s, leave it hidden
    videoReadyTimer = setTimeout(function () {
      video.removeEventListener("playing", onVideoPlaying);
      videoReadyTimer = null;
    }, VIDEO_TIMEOUT_MS);
  }

  function exitSunlight() {
    clearTimeout(videoReadyTimer);
    videoReadyTimer = null;
    disarmGestureRetry();
    video.removeEventListener("playing", onVideoPlaying);
    video.classList.remove("video-ready");
    video.pause();
    video.currentTime = 0;
  }

  function isSunlight() {
    return document.documentElement.getAttribute("data-theme") === "sunlight";
  }

  // ── Keyboard shortcut: [S] to toggle ──

  document.addEventListener("keydown", function (e) {
    // Don't activate when typing in form fields or contenteditable
    var tag = e.target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || e.target.isContentEditable) {
      return;
    }
    if (e.key === "s" || e.key === "S") {
      toggleSunlight();
    }
  });

  // ── React to moss theme toggle (light/dark button click) ──
  // When the user clicks the nav theme toggle, moss dispatches
  // moss-theme-change. If we were in sunlight mode, clean up.

  document.documentElement.addEventListener("moss-theme-change", function (e) {
    if (e.detail && e.detail.previous === "sunlight") {
      exitSunlight();
    }
  });

  // ── Long-press theme toggle: hold for sunlight mode ──
  // Short click = normal light/dark toggle (handled by onclick="toggleTheme()")
  // Long press (≥500ms) = enter/exit sunlight mode

  function toggleSunlight() {
    if (isSunlight()) {
      document.documentElement.setAttribute("data-theme", "light");
      sessionStorage.setItem("theme", "light");
      exitSunlight();
    } else {
      enterSunlight();
    }
  }

  var LONG_PRESS_MS = 500;
  var MOVE_TOLERANCE = 10; // px — cancel if pointer drifts beyond this
  var longPressTimer = null;
  var didLongPress = false;
  var startX = 0;
  var startY = 0;

  var themeBtn = document.querySelector(".nav-theme-btn");
  if (themeBtn) {
    // Pointer Events unify mouse, touch, and pen into one set of listeners.
    themeBtn.addEventListener("pointerdown", function (e) {
      if (e.button !== 0) return; // left-click/primary touch only
      didLongPress = false;
      startX = e.clientX;
      startY = e.clientY;
      longPressTimer = setTimeout(function () {
        didLongPress = true;
        toggleSunlight();
      }, LONG_PRESS_MS);
    });

    themeBtn.addEventListener("pointermove", function (e) {
      if (!longPressTimer) return;
      var dx = e.clientX - startX;
      var dy = e.clientY - startY;
      if (dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
    });

    themeBtn.addEventListener("pointerup", function () {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    });

    themeBtn.addEventListener("pointercancel", function () {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    });

    themeBtn.addEventListener("pointerleave", function () {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    });

    // Suppress the normal onclick toggle if long press was detected.
    // useCapture: true fires before the onclick attribute handler.
    themeBtn.addEventListener("click", function (e) {
      if (didLongPress) {
        e.preventDefault();
        e.stopImmediatePropagation();
        didLongPress = false;
      }
    }, true);

    // Prevent browser context menu during long-press on touch devices
    themeBtn.addEventListener("contextmenu", function (e) {
      if (longPressTimer || didLongPress) {
        e.preventDefault();
      }
    });
  }

  // ── Initialization ──
  // Sunlight is the default first-visit experience. Skip only if the user
  // already chose a different theme this session.

  var savedTheme = sessionStorage.getItem("theme");
  if (savedTheme === "sunlight" || !savedTheme) {
    enterSunlight();
  }

  // ── Preview morph resilience ──
  // moss's in-place preview refreshes the page by morphing <body>'s innerHTML
  // to the freshly-built HTML (idiomorph, morphStyle "innerHTML"). Our overlay
  // nodes are appended at runtime and never exist in the served bytes, so the
  // morph reconciles them away — and since data-theme="sunlight" lives on <html>
  // (which the morph doesn't touch), the page is left "in sunlight" with no leaf
  // video. moss dispatches `moss-morph-patched` after every morph precisely so
  // once-bound site scripts can re-attach (the built-in theme.js listens too).
  // The script itself is not re-executed across morphs (idiomorph reuses the
  // matched <script> node), so these closure-scoped nodes survive; we just need
  // to re-parent them. Idempotent: re-append only when detached, and resume
  // playback only when sunlight is still the active theme.
  document.addEventListener("moss-morph-patched", function () {
    var reattached = false;
    if (!video.isConnected) {
      document.body.appendChild(video);
      reattached = true;
    }
    if (!wash.isConnected) {
      document.body.appendChild(wash);
      reattached = true;
    }
    // A detached <video> is paused; only when we actually re-attached (and
    // sunlight is still active) do we replay — enterSunlight() restarts
    // playback and the fade-in. Morphs that left the overlay intact are no-ops.
    if (reattached && isSunlight()) enterSunlight();
  });
})();

// Authors write a plain link at the end of a paragraph or list item — `[▶](#scene=tree)`, or
// `[▶](#scene=)` for "just open the surface" with no scripted scene (site/ui/demo/README.md,
// "Markdown markers"). This upgrades every such link into a <moss-demo-marker> and, if the
// article has any, inserts the one demo frame it drives after the article's first paragraph.
// `demo.css` hides an unupgraded `a[href^="#scene="]` outright, so a reader without JavaScript
// never sees a dead ▶ that does nothing (verified: the rule is removed only by the anchor itself
// being replaced below, never by a second toggle that could fall out of sync with it).
const sceneLinks = [...document.querySelectorAll('article a[href^="#scene="]')];
if (sceneLinks.length > 0) {
  const demoFrameModule = new URL('../../ui/demo/demo-frame.js', window.mossTheme.base);
  const editorDemoModule = new URL('../../ui/demo/moss-editor-demo.js', window.mossTheme.base);
  const markerModule = new URL('../../ui/demo/moss-demo-marker.js', window.mossTheme.base);
  // Sequenced, not parallel: a marker's click dispatches `moss-demo-play`, which only reaches a
  // frame if the frame's document-level listener is already attached — so demo-frame.js (via
  // moss-editor-demo.js, which defines the element) must finish attaching that listener before
  // moss-demo-marker.js can upgrade any link into a marker that might fire before the reader even
  // finishes loading the page.
  import(demoFrameModule.href)
    .then(() => import(editorDemoModule.href))
    .then(() => import(markerModule.href))
    .then(() => {
      const article = document.querySelector('article');
      for (const link of sceneLinks) {
        const name = decodeURIComponent(link.getAttribute('href').slice('#scene='.length));
        const marker = document.createElement('moss-demo-marker');
        marker.setAttribute('name', name);
        link.replaceWith(marker);
      }
      // One surface per page today (site/ui/demo/README.md, "Surfaces") — every scene a page
      // links is checked by scripts/check-demo-scenes.mjs to name the same surface, so inserting
      // the one editor-surface frame unconditionally is correct without first fetching every
      // linked scene's own JSON just to ask which surface it wants.
      if (article && !article.querySelector('moss-editor-demo')) {
        const frame = document.createElement('moss-editor-demo');
        const firstParagraph = article.querySelector('p');
        if (firstParagraph) firstParagraph.after(frame);
        else article.prepend(frame);
      }
    })
    .catch((error) => {
      console.error('Could not load the moss demo frame', error);
    });
}
