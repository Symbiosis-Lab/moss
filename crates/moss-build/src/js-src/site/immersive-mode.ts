/**
 * Immersive Mode - FLIP-Based Click-Triggered Fullscreen Toggle
 *
 * The sole gate is DOM structure: every LOCAL iframe that is a direct child of
 * `main > article.container` is wrapped in a container with a floating
 * fullscreen toggle button. External embeds (src starting with `http(s)://` or
 * `//`) are left untouched — only your own embedded apps get the controls, and
 * every such local iframe on the page is wrapped (not just the first). No
 * server-side body class is required — any page whose content lands a local
 * iframe in that position gets the controls. Uses the FLIP (First, Last,
 * Invert, Play) technique for smooth enter/exit transitions.
 *
 * FLIP overview:
 * 1. First: capture element's current getBoundingClientRect()
 * 2. Last: apply the final state (position: fixed; inset: 0)
 * 3. Invert: set a CSS transform so element visually appears at First position
 * 4. Play: transition transform to identity -> smooth animation
 *
 * Native Fullscreen API:
 * When available, requestFullscreen() is called before the FLIP animation on
 * enter. On exit, document.exitFullscreen() is called and the fullscreenchange
 * event triggers the FLIP exit animation. Falls back to CSS-only mode when the
 * Fullscreen API is unavailable (e.g. cross-origin iframes, older browsers).
 */

import { langBucket } from "./subscribe/i18n";

const IMMERSIVE_COPY = {
  en: {
    enterFullscreen: "Enter fullscreen",
    exitFullscreen: "Exit fullscreen",
    openInNewWindow: "Open in new window",
  },
  "zh-hans": {
    enterFullscreen: "进入全屏",
    exitFullscreen: "退出全屏",
    openInNewWindow: "在新窗口打开",
  },
  "zh-hant": {
    enterFullscreen: "進入全螢幕",
    exitFullscreen: "退出全螢幕",
    openInNewWindow: "在新視窗開啟",
  },
} as const;

function immersiveCopy(): typeof IMMERSIVE_COPY["en"] {
  return IMMERSIVE_COPY[langBucket(document.documentElement.lang)];
}

// Material Design icons (Chrome, Firefox, Edge)
const MD_ENTER_FS = '<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>';
const MD_EXIT_FS = '<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>';
// Lucide external-link — same glyph for both Material and WebKit code paths
const ICON_OPEN_NEW_SVG = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6"/></svg>';

// WebKit icons (Safari + Tauri WKWebView) -- from official WebKit source (full precision paths)
const WK_ENTER_FS = '<svg width="20" height="20" viewBox="0 0 15 15" fill="currentColor"><path d="M0.791840426,6.36849468 C1.20478723,6.36849468 1.50087766,6.05682979 1.50087766,5.64386702L1.50087766,4.98935638 L1.34505319,2.27004787 L3.3942766,4.42835638 L5.79411702,6.84378723C5.92657979,6.98403723 6.09799468,7.04638298 6.285,7.04638298C6.72912766,7.04638298 7.05638298,6.75807979 7.05638298,6.32173936C7.05638298,6.11137234 6.98625,5.92436702 6.846,5.78411702L4.43835638,3.38426064 L2.28004787,1.33503723 L5.00715957,1.49087766L5.65386702,1.49087766C6.06682979,1.49087766 6.38629787,1.20259043 6.38629787,0.781824468C6.38629787,0.361074468 6.07461702,0.065 5.65386702,0.065L1.32946277,0.065C0.534702128,0.065 0.075,0.524702128 0.075,1.31946277L0.075,5.64386702C0.075,6.04904255 0.37887766,6.36849468 0.791840426,6.36849468ZM9.33935106,14.9315957L13.6637553,14.9315957C14.4585319,14.9315957 14.9260851,14.4718888 14.9260851,13.677133L14.9260851,9.35272872C14.9260851,8.94755319 14.6220957,8.62810106 14.2014574,8.62810106C13.7962181,8.62810106 13.4923404,8.93976596 13.4923404,9.35272872L13.4923404,10.0072394 L13.6559681,12.7265431 L11.5989574,10.5682394 L9.2068883,8.15280851C9.07444149,8.01255851 8.8952234,7.95021277 8.70821809,7.95021277C8.27189362,7.95021277 7.93685106,8.23851596 7.93685106,8.67484043C7.93685106,8.8852234 8.01475532,9.07222872 8.15500532,9.21247872L10.5548617,11.6123255 L12.7209574,13.6615489 L9.9938617,13.5057149L9.33935106,13.5057149C8.9263883,13.5057149 8.60693617,13.7940085 8.60693617,14.2147617C8.60693617,14.6355149 8.9263883,14.9315957 9.33935106,14.9315957Z"/></svg>';
const WK_EXIT_FS = '<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor"><path d="M1.66493878,7.19582449L6.08936327,7.19582449C6.90250612,7.19582449 7.37285714,6.71750612 7.37285714,5.90436327L7.37285714,1.48792245C7.37285714,1.07337551 7.06195102,0.746518367 6.63943673,0.746518367C6.21692245,0.746518367 5.91398367,1.06540816 5.91398367,1.48792245L5.91398367,2.1575551 L6.08139592,4.93975918 L3.97680816,2.73153061 L1.52942857,0.260232653C1.39390204,0.116738776 1.21053878,0.045 1.01922449,0.045C0.56482449,0.045 0.23,0.347922449 0.23,0.794355102C0.23,1.00162041 0.309722449,1.20091837 0.453216327,1.34442857L2.90856327,3.79977551 L5.11679184,5.89639592 L2.33458776,5.73695102L1.66493878,5.73695102C1.24242449,5.73695102 0.915583673,6.03192245 0.915583673,6.46240408C0.915583673,6.88491837 1.23445714,7.19582449 1.66493878,7.19582449ZM9.31003265,15.2873412C9.73254694,15.2873412 10.0354857,14.9764351 10.0354857,14.5459502L10.0354857,13.7886168 L9.86807347,11.0143878 L11.9726612,13.2226098 L14.4758449,15.7417347C14.6113551,15.8852286 14.786702,15.9569837 14.986049,15.9569837C15.4324163,15.9569837 15.7672735,15.6540449 15.7672735,15.207622C15.7672735,15.0003518 15.6956,14.8010522 15.5520898,14.6575584L13.0409061,12.1463976 L10.8247102,10.0497837 L13.6148816,10.2092122L14.3722204,10.2092122C14.794702,10.2092122 15.1215592,9.91425714 15.1215592,9.49174286C15.1215592,9.06126122 14.802702,8.75832245 14.3722204,8.75832245L9.8600898,8.75832245C9.04696327,8.75832245 8.57661224,9.22867347 8.57661224,10.0418L8.57661224,14.5459502C8.57661224,14.9684645 8.87955102,15.2873412 9.31003265,15.2873412Z"/></svg>';
const isWebKit = (typeof CSS !== "undefined" && CSS.supports?.("hanging-punctuation", "first")) ?? false;
const ICON_ENTER_FS = isWebKit ? WK_ENTER_FS : MD_ENTER_FS;
const ICON_EXIT_FS = isWebKit ? WK_EXIT_FS : MD_EXIT_FS;
const ICON_OPEN_NEW = ICON_OPEN_NEW_SVG;

export function initImmersiveMode(): void {
  const article = document.querySelector(
    "main > article.container"
  ) as HTMLElement | null;
  if (!article) return;

  const iframes = [
    ...article.querySelectorAll(":scope > iframe"),
  ] as HTMLIFrameElement[];
  for (const iframe of iframes) {
    // Local vs external: use the RAW attribute, not the resolved `iframe.src`
    // (which is always an absolute http(s) URL even for relative embeds).
    const raw = iframe.getAttribute("src") || "";
    const isExternal = /^https?:\/\//i.test(raw) || raw.startsWith("//");
    if (isExternal) continue;
    setupImmersiveIframe(iframe);
  }
}

/**
 * Wrap a single LOCAL iframe with its own fullscreen + open-in-new-tab controls
 * and independent FLIP fullscreen state. All handlers close over THIS iframe.
 */
function setupImmersiveIframe(iframe: HTMLIFrameElement): void {
  // Wrap iframe in container for button positioning and FLIP animation
  const wrapper = document.createElement("div");
  wrapper.className = "immersive-iframe-wrapper";

  const button = document.createElement("button");
  button.type = "button";
  button.className = "immersive-fullscreen-btn";
  button.setAttribute("aria-label", immersiveCopy().enterFullscreen);
  button.innerHTML = ICON_ENTER_FS;

  // Anchor (not button) so tauri-plugin-opener intercepts the click in moss
  // preview; window.open is swallowed by WKWebview (Tauri issue #9912).
  const newWindowBtn = document.createElement("a");
  newWindowBtn.className = "immersive-new-window-btn";
  newWindowBtn.setAttribute("aria-label", immersiveCopy().openInNewWindow);
  newWindowBtn.target = "_blank";
  newWindowBtn.rel = "noopener noreferrer";
  newWindowBtn.href = new URL(iframe.src, window.location.href).href;
  newWindowBtn.innerHTML = ICON_OPEN_NEW;

  // Move iframe into wrapper without removing from DOM tree (preserves iframe state)
  iframe.parentNode!.insertBefore(wrapper, iframe);
  wrapper.appendChild(iframe);
  wrapper.appendChild(button);
  wrapper.appendChild(newWindowBtn);

  let isFullscreen = false;
  let inlineRect: DOMRect | null = null;

  async function enterFullscreen(): Promise<void> {
    if (isFullscreen) return;

    // FLIP: First -- capture current position
    const firstRect = wrapper.getBoundingClientRect();
    inlineRect = firstRect;

    // Request native fullscreen first so FLIP calculates after viewport change
    try { await document.documentElement.requestFullscreen?.(); } catch {}

    // FLIP: Last -- apply fullscreen state
    document.body.classList.add("immersive-fs-active");
    isFullscreen = true;
    button.innerHTML = ICON_EXIT_FS;
    button.setAttribute("aria-label", immersiveCopy().exitFullscreen);

    // Force layout to get the "Last" rect
    const lastRect = wrapper.getBoundingClientRect();

    // FLIP: Invert -- calculate compensating transform
    const deltaX = firstRect.left - lastRect.left;
    const deltaY = firstRect.top - lastRect.top;
    const scaleX = lastRect.width > 0 ? firstRect.width / lastRect.width : 1;
    const scaleY = lastRect.height > 0 ? firstRect.height / lastRect.height : 1;

    wrapper.style.transform =
      `translate(${deltaX}px, ${deltaY}px) scale(${scaleX}, ${scaleY})`;

    // FLIP: Play -- animate to identity transform
    requestAnimationFrame(() => {
      wrapper.classList.add("fs-animating-enter");
      wrapper.style.transform = "";
      button.disabled = true;
      newWindowBtn.setAttribute("aria-disabled", "true");

      function onEnterEnd(e: Event) {
        if ((e as TransitionEvent).propertyName !== "transform") return;
        wrapper.removeEventListener("transitionend", onEnterEnd);
        wrapper.classList.remove("fs-animating-enter");
        button.disabled = false;
        newWindowBtn.removeAttribute("aria-disabled");
      }
      wrapper.addEventListener("transitionend", onEnterEnd);
    });
  }

  function exitFullscreen(): void {
    if (!isFullscreen || !inlineRect) return;

    // If native fullscreen is active, exit it first.
    // The fullscreenchange listener will call runExitAnimation().
    if (document.fullscreenElement) {
      document.exitFullscreen?.().catch(() => {});
      return;
    }

    // CSS-only mode (no native fullscreen): animate directly
    runExitAnimation();
  }

  function runExitAnimation(): void {
    if (!isFullscreen || !inlineRect) return;

    const firstRect = wrapper.getBoundingClientRect();

    // Calculate transform from current (fullscreen) to stored inline position
    const deltaX = inlineRect.left - firstRect.left;
    const deltaY = inlineRect.top - firstRect.top;
    const scaleX = firstRect.width > 0 ? inlineRect.width / firstRect.width : 1;
    const scaleY = firstRect.height > 0 ? inlineRect.height / firstRect.height : 1;

    // Start exit animation while still in fullscreen
    wrapper.classList.add("fs-animating-exit");
    wrapper.style.transform =
      `translate(${deltaX}px, ${deltaY}px) scale(${scaleX}, ${scaleY})`;
    button.disabled = true;
    newWindowBtn.setAttribute("aria-disabled", "true");

    function onExitEnd(e: Event) {
      if ((e as TransitionEvent).propertyName !== "transform") return;
      wrapper.removeEventListener("transitionend", onExitEnd);
      wrapper.classList.remove("fs-animating-exit");
      wrapper.style.transform = "";
      document.body.classList.remove("immersive-fs-active");
      isFullscreen = false;
      button.innerHTML = ICON_ENTER_FS;
      button.setAttribute("aria-label", immersiveCopy().enterFullscreen);
      button.disabled = false;
      newWindowBtn.removeAttribute("aria-disabled");
    }
    wrapper.addEventListener("transitionend", onExitEnd);
  }

  function toggleFullscreen(): void {
    if (isFullscreen) {
      exitFullscreen();
    } else {
      enterFullscreen();
    }
  }

  button.addEventListener("click", (e: MouseEvent) => {
    e.stopPropagation();
    toggleFullscreen();
  });

  newWindowBtn.addEventListener("click", (e: MouseEvent) => {
    e.stopPropagation();
    if (newWindowBtn.getAttribute("aria-disabled") === "true") {
      e.preventDefault();
    }
  });

  // Sync state when native fullscreen is exited (e.g. browser Esc handling)
  document.addEventListener("fullscreenchange", () => {
    if (!document.fullscreenElement && isFullscreen) {
      runExitAnimation();
    }
  });

  // Fallback Escape handler for CSS-only mode (no native fullscreen)
  document.addEventListener("keydown", (e: KeyboardEvent) => {
    if (e.key === "Escape" && isFullscreen && !document.fullscreenElement) {
      exitFullscreen();
      e.preventDefault();
    }
  });

  // Handle Escape forwarded from interactive iframe via iframe-bridge postMessage
  // Fallback for CSS-only mode (native fullscreen handles its own Esc)
  window.addEventListener("message", (e: MessageEvent) => {
    if (e.origin !== window.location.origin) return;
    if (e.data?.type === "moss-escape-key" && isFullscreen && !document.fullscreenElement) {
      exitFullscreen();
    }
  });
}
