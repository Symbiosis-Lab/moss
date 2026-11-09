/**
 * preview/scrollbar.ts — Shell-side fake scrollbar over the preview iframe.
 *
 * Architecture: chrome and content live in separate layers. The iframe paints
 * content edge-to-edge in its own coordinate system. The shell paints chrome
 * (titlebar overlay, this scrollbar) on top. The iframe is the single source
 * of scroll truth; this module reflects what it reports and forwards user
 * scrollbar gestures back to the iframe.
 *
 * Protocol:
 *   iframe → shell  { type: "moss-iframe-scroll",
 *                     scrollTop, scrollHeight, clientHeight, gen }
 *   shell  → iframe { type: "moss-iframe-scroll-to", scrollTop }
 *
 * `gen` is a navigation generation counter; the iframe increments it on
 * pageshow, and we drop messages older than the latest seen `gen` so a
 * just-navigated-away page can't re-position the thumb after the new page
 * has already reported its initial geometry.
 *
 * Wheel-over-chrome: the browser routes `wheel` events to the element under
 * the cursor. The titlebar and the scrollbar track sit above the iframe in
 * the shell DOM, so wheel events there don't reach the iframe. We intercept
 * those and forward as scroll-to commands. Without this, scrolling feels
 * broken whenever the cursor strays into chrome.
 */

const HIDE_DELAY_MS = 800;

export interface ScrollbarOptions {
  iframe: HTMLIFrameElement;
  track: HTMLElement;
  thumb: HTMLElement;
  /** Element painted at the top of the preview, occluding scroll content.
   *  Wheel events over this are forwarded to the iframe. */
  titlebar: HTMLElement | null;
  /** The preview container (positioning context for the track). The track's
   *  top is `var(--moss-titlebar-height)` in BOTH desktop and mobile mode —
   *  the titlebar floats over the preview in both — so `mobile-mode` does not
   *  alter scrollbar geometry; `trackGeometry()` reads the track's own rect. */
  previewContainer: HTMLElement;
}

export class PreviewScrollbar {
  private readonly iframe: HTMLIFrameElement;
  private readonly track: HTMLElement;
  private readonly thumb: HTMLElement;
  private readonly titlebar: HTMLElement | null;
  private readonly previewContainer: HTMLElement;

  // Latest geometry reported by the iframe.
  private scrollTop = 0;
  private scrollHeight = 0;
  private clientHeight = 0;
  // Highest navigation generation seen so far. Reports with lower `gen`
  // are stale (from a previous document) and ignored.
  private gen = 0;

  // Drag state.
  private dragging = false;
  private dragStartY = 0;
  private dragStartScrollTop = 0;

  // Auto-hide timer.
  private hideTimer: number | null = null;

  // Render coalescing.
  private renderRafScheduled = false;

  constructor(opts: ScrollbarOptions) {
    this.iframe = opts.iframe;
    this.track = opts.track;
    this.thumb = opts.thumb;
    this.titlebar = opts.titlebar;
    this.previewContainer = opts.previewContainer;

    window.addEventListener("message", this.onMessage);
    this.thumb.addEventListener("pointerdown", this.onPointerDown);
    document.addEventListener("pointermove", this.onPointerMove);
    document.addEventListener("pointerup", this.onPointerUp);

    // Wheel forwarding from chrome (titlebar + scrollbar track) into the iframe.
    if (this.titlebar) {
      this.titlebar.addEventListener("wheel", this.onChromeWheel, { passive: false });
    }
    this.track.addEventListener("wheel", this.onChromeWheel, { passive: false });

    // Re-render on container resize (viewport-dependent track height).
    window.addEventListener("resize", this.scheduleRender);

    // Divider drags change the iframe inset via --action-panel-width WITHOUT
    // a window resize (stable-shell geometry: the shell webview never resizes
    // on drag), so the old resize listener no longer fires for drags. The
    // iframe's own resize/ResizeObserver re-report covers thumb geometry
    // asynchronously; this direct signal (dispatched by divider.ts's
    // setPanelWidthVar) re-renders the track deterministically on the same
    // tick.
    window.addEventListener("moss-divider-changed", this.scheduleRender);
  }

  private onMessage = (e: MessageEvent): void => {
    const data = e.data;
    if (!data || data.type !== "moss-iframe-scroll") return;
    // Validate origin: only accept messages from our preview iframe.
    // (Cross-origin sandboxed iframes still expose the same WindowProxy,
    // but be defensive in case other iframes appear.)
    if (e.source !== this.iframe.contentWindow) return;

    const incomingGen = Number(data.gen) || 0;
    if (incomingGen < this.gen) return; // stale message from previous document
    this.gen = incomingGen;

    this.scrollTop = Number(data.scrollTop) || 0;
    this.scrollHeight = Number(data.scrollHeight) || 0;
    this.clientHeight = Number(data.clientHeight) || 0;

    this.reveal();
    this.scheduleRender();
  };

  /** Track geometry inside the preview container. */
  private trackGeometry(): { trackHeight: number; thumbHeight: number; maxThumbTop: number } {
    // The track element has its own bounding rect — read it directly so we
    // don't have to know about titlebar height or mobile-mode here.
    const trackRect = this.track.getBoundingClientRect();
    const trackHeight = trackRect.height;
    if (this.scrollHeight <= 0 || this.clientHeight <= 0) {
      return { trackHeight, thumbHeight: 0, maxThumbTop: 0 };
    }
    const thumbHeight = Math.max(24, (this.clientHeight / this.scrollHeight) * trackHeight);
    return { trackHeight, thumbHeight, maxThumbTop: Math.max(0, trackHeight - thumbHeight) };
  }

  private scheduleRender = (): void => {
    if (this.renderRafScheduled) return;
    this.renderRafScheduled = true;
    requestAnimationFrame(() => {
      this.renderRafScheduled = false;
      this.render();
    });
  };

  private render(): void {
    const scrollable = this.scrollHeight - this.clientHeight;
    if (scrollable <= 1) {
      this.track.style.display = "none";
      return;
    }
    // Use block (not "") because the CSS default is `display: none` to hide
    // the scrollbar before the iframe reports any geometry.
    this.track.style.display = "block";

    const { thumbHeight, maxThumbTop } = this.trackGeometry();
    if (maxThumbTop <= 0) {
      this.track.style.display = "none";
      return;
    }

    const ratio = this.scrollTop / scrollable;
    const thumbTop = Math.min(maxThumbTop, Math.max(0, ratio * maxThumbTop));
    this.thumb.style.height = `${thumbHeight}px`;
    // transform avoids triggering layout on every render
    this.thumb.style.transform = `translateY(${thumbTop}px)`;
  }

  private reveal(): void {
    this.track.classList.add("moss-fsb-active");
    if (this.hideTimer !== null) clearTimeout(this.hideTimer);
    if (!this.dragging) {
      this.hideTimer = window.setTimeout(() => {
        this.track.classList.remove("moss-fsb-active");
        this.hideTimer = null;
      }, HIDE_DELAY_MS);
    }
  }

  /** Send a scroll-to command to the iframe. */
  private commandScrollTo(scrollTop: number): void {
    const win = this.iframe.contentWindow;
    if (!win) return;
    // Optimistic local update so the thumb tracks input without a round-trip.
    // The iframe will report back and reconcile.
    this.scrollTop = Math.max(0, Math.min(this.scrollHeight - this.clientHeight, scrollTop));
    this.scheduleRender();
    win.postMessage({ type: "moss-iframe-scroll-to", scrollTop }, "*");
  }

  private onPointerDown = (e: PointerEvent): void => {
    if (e.target !== this.thumb) return;
    this.dragging = true;
    this.dragStartY = e.clientY;
    this.dragStartScrollTop = this.scrollTop;
    this.track.classList.add("moss-fsb-dragging");
    this.thumb.setPointerCapture(e.pointerId);
    e.preventDefault();
  };

  private onPointerMove = (e: PointerEvent): void => {
    if (!this.dragging) return;
    const { thumbHeight, trackHeight } = this.trackGeometry();
    const maxThumbTop = trackHeight - thumbHeight;
    if (maxThumbTop <= 0) return;
    const dy = e.clientY - this.dragStartY;
    const scrollable = this.scrollHeight - this.clientHeight;
    const scrollPerPx = scrollable / maxThumbTop;
    this.commandScrollTo(this.dragStartScrollTop + dy * scrollPerPx);
  };

  private onPointerUp = (): void => {
    if (!this.dragging) return;
    this.dragging = false;
    this.track.classList.remove("moss-fsb-dragging");
    this.reveal();
  };

  private onChromeWheel = (e: WheelEvent): void => {
    // Convert deltaY into a scrollTop delta.
    // (Line/page modes are rare; treat them as approximate pixel deltas.)
    let delta = e.deltaY;
    if (e.deltaMode === 1 /* DOM_DELTA_LINE */) delta *= 16;
    else if (e.deltaMode === 2 /* DOM_DELTA_PAGE */) delta *= this.clientHeight;
    if (delta === 0) return;
    e.preventDefault();
    this.commandScrollTo(this.scrollTop + delta);
  };
}
