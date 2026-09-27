// The landing page's own additions to the harvested preview shell. shell.html
// is regenerated from the desktop app, which drops any hand edit to it, so
// what the landing adds lives here and reaches the shell through what it
// exposes: window.__shell (its ring component, setPending, growTo,
// publishFlash), the URL it was opened with, and its DOM. The ring's own
// set(), paint() and the svg and host it keeps are checked before anything
// leans on them. The landing page loads this into scene 2's shell once the
// shell has booted; it lives beside the landing, not in the harvested bank
// under ui/, because nothing but the landing uses it.
(() => {
  const shell = window.__shell, button = document.querySelector('.moss-publish-button');
  if (!shell || !button || window.__landingShell) return;
  window.__landingShell = true;
  const params = new URLSearchParams(location.search);

  // Scene 2's quiet invitation: while a preview still invites its first
  // publish, the body carries moss-landing-scene2, which the landing page
  // reads to send its rings from the button. Under reduced motion there are
  // no rings, so the button keeps a still one of its own.
  if (params.has('preview') && !matchMedia('(pointer: coarse)').matches) {
    document.body.classList.add('moss-landing-scene2');
    const style = document.createElement('style');
    style.textContent = '@media (prefers-reduced-motion: reduce) { body.moss-landing-scene2 .moss-publish-button:not(:disabled)::after { content: ""; position: absolute; inset: -2px; border: 1px solid var(--moss-chrome-publish-bg); border-radius: 50%; opacity: .28; pointer-events: none; } }';
    document.head.append(style);
    const stop = () => document.body.classList.remove('moss-landing-scene2');
    button.addEventListener('click', stop, { once: true });
    const onPublishKey = (event) => { if (event.key === 'Enter' || event.key === ' ') { stop(); button.removeEventListener('keydown', onPublishKey); } };
    button.addEventListener('keydown', onPublishKey);
  }

  // The Publish ring grows in place. The app rebuilds its ring on every set(),
  // which is fine at the app's rate; the landing grows it every animation
  // frame, and a rebuild tore an SVG out of the glass Publish pill about 40
  // times a growth, which made the pill and its shadow flicker. While the
  // ring keeps the same arcs, the new geometry is drawn off the page and moved
  // onto the ring that stands. A refreshed harvest whose ring is shaped
  // otherwise keeps the ring as the app draws it, rather than breaking it.
  const ring = shell.ring, fits = !!ring && typeof ring.paint === 'function' && typeof ring.set === 'function' && 'svg' in ring && 'host' in ring;
  const paint = ring?.paint;
  if (fits) ring.paint = function (arcs, restyled) {
    const standing = this.svg, key = arcs.map((a) => a.verb).join() + (restyled > 0 ? '+' : '');
    if (!standing || standing.__arcs !== key) { paint.call(this, arcs, restyled); this.svg.__arcs = key; return; }
    const host = this.host;
    this.host = document.createDocumentFragment(); this.svg = null;
    try { paint.call(this, arcs, restyled); } finally {
      const fresh = this.svg; this.host = host; this.svg = standing;
      fresh?.querySelectorAll('circle').forEach((circle, k) => {
        for (const name of ['transform', 'stroke-dasharray']) if (circle.hasAttribute(name)) standing.children[k]?.setAttribute(name, circle.getAttribute(name));
      });
    }
  };
})();
