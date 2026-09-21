// The painter registry: watercolor-morph design doc section 4. One function
// per element kind, each returning pixels or declaring that it cannot --
// never a silent hole, never a thrown capture. Lives beside landing.js (a
// plain JS asset under site/, like landing-i18n.js and closing.js -- none of
// the three become a page, only .html/.md under site/ do) rather than under
// packages/, because it runs as a classic script in the same browser context
// landing.js already loads into, with no bundler in this project to resolve
// an ES module import across a package boundary. Standalone today (dissolve
// unit 2, step A): loaded by the fixture harness only. Landing's own capture
// threads its img/canvas2d/webgl/video branches through this registry as of
// step B; the whole-document foreignObject path (raster/rasterDoc in
// landing.js) is untouched, so svg/iframe/box below serve the fixture's own
// I-fidelity coverage and future callers, not landing.js today.
(function (global) {
  'use strict';

  function offscreen(w, h) {
    const c = document.createElement('canvas');
    c.width = Math.max(1, Math.round(w));
    c.height = Math.max(1, Math.round(h));
    return c;
  }
  function clipRadius(g, w, h, radius) {
    if (!radius) return;
    g.beginPath(); g.roundRect(0, 0, w, h, radius); g.clip();
  }
  function loadImage(src) {
    return new Promise((ok, no) => {
      const img = new Image();
      img.onload = () => ok(img);
      img.onerror = () => no(new Error('image failed to decode'));
      img.src = src;
    });
  }

  // img -- decode() first (currentSrc, not src: the resource the browser
  // actually picked for a responsive srcset), then object-fit/object-position
  // and border-radius honoured the same way drawPlates/rasterDoc already do.
  async function paintImg(el, ctx) {
    try { if (!el.complete || !el.naturalWidth) await el.decode(); }
    catch (e) { return { ok: false, reason: 'img decode failed: ' + e.message }; }
    if (!el.naturalWidth) return { ok: false, reason: 'img has no natural size after decode' };
    const { w, h, dpr, borderRadius = 0 } = ctx;
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    const cs = ctx.computedStyle || getComputedStyle(el);
    const fit = cs.objectFit || 'fill';
    const scale = fit === 'contain' ? Math.min(cv.width / el.naturalWidth, cv.height / el.naturalHeight)
      : fit === 'fill' ? null : Math.max(cv.width / el.naturalWidth, cv.height / el.naturalHeight);
    const dw = fit === 'fill' ? cv.width : el.naturalWidth * scale;
    const dh = fit === 'fill' ? cv.height : el.naturalHeight * scale;
    const pos = (cs.objectPosition || '50% 50%').split(/\s+/).map((v) => v.endsWith('%') ? parseFloat(v) / 100 : 0.5);
    // drawImage on an <img> always paints whatever currentSrc resolved to
    // (the browser's own responsive-image pick); decode() above is what
    // makes that resource the on-screen one before this reads it.
    g.drawImage(el, (cv.width - dw) * (pos[0] ?? 0.5), (cv.height - dh) * (pos[1] ?? 0.5), dw, dh);
    return { ok: true, canvas: cv };
  }

  // 2D canvas -- single-buffered by spec, always readable; the only failure
  // is a tainted canvas (cross-origin content drawn into it).
  function paintCanvas2d(el, ctx) {
    const { w, h, dpr, borderRadius = 0 } = ctx;
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    try { g.drawImage(el, 0, 0, cv.width, cv.height); }
    catch (e) { return { ok: false, reason: 'canvas2d tainted or unreadable: ' + e.message }; }
    return { ok: true, canvas: cv };
  }

  // WebGL canvas -- never a blind drawImage/toDataURL: the drawing buffer is
  // allowed to already be cleared by the time anything outside the draw call
  // reads it. Prefer the element's own mossCaptureFrame() hook (redraws and
  // reads in the same task, the contract mandelbrot-renderer.js and
  // native-canvas.js already implement); otherwise require
  // preserveDrawingBuffer, checked via the live context's own attributes, not
  // guessed; otherwise declare failure honestly.
  async function paintWebgl(el, ctx) {
    const { w, h, dpr, borderRadius = 0 } = ctx;
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    if (typeof el.mossCaptureFrame === 'function') {
      let dataUrl;
      try { dataUrl = el.mossCaptureFrame(); }
      catch (e) { return { ok: false, reason: 'mossCaptureFrame threw: ' + e.message }; }
      try { const img = await loadImage(dataUrl); g.drawImage(img, 0, 0, cv.width, cv.height); return { ok: true, canvas: cv }; }
      catch (e) { return { ok: false, reason: 'mossCaptureFrame image failed: ' + e.message }; }
    }
    const gl = el.dataset.glContext === 'webgl' ? el.getContext('webgl') : el.getContext('webgl2');
    const attrs = gl && gl.getContextAttributes && gl.getContextAttributes();
    if (attrs && attrs.preserveDrawingBuffer) {
      try { g.drawImage(el, 0, 0, cv.width, cv.height); return { ok: true, canvas: cv }; }
      catch (e) { return { ok: false, reason: 'webgl canvas unreadable: ' + e.message }; }
    }
    return { ok: false, reason: 'no mossCaptureFrame hook and preserveDrawingBuffer is not set' };
  }

  // video -- the frame on screen, frozen: current frame via drawImage when a
  // frame has decoded, the poster when not, declared failure when neither.
  // Cross-origin taints a canvas the same way a tainted 2D canvas does.
  async function paintVideo(el, ctx) {
    const { w, h, dpr, borderRadius = 0 } = ctx;
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    if (el.readyState >= 2 && el.videoWidth) {
      try { g.drawImage(el, 0, 0, cv.width, cv.height); return { ok: true, canvas: cv }; }
      catch (e) { return { ok: false, reason: 'video frame unreadable (cross-origin taint?): ' + e.message }; }
    }
    const posterSrc = el.poster || el.dataset.poster;
    if (posterSrc) {
      try { const img = await loadImage(posterSrc); g.drawImage(img, 0, 0, cv.width, cv.height); return { ok: true, canvas: cv }; }
      catch (e) { /* falls through to the declared failure below */ }
    }
    return { ok: false, reason: 'video has no decoded frame and no usable poster' };
  }

  // inline SVG -- serialized as its own standalone document (not wrapped in a
  // foreignObject: it is already SVG), with no external references, so its
  // markup alone is a complete image source.
  async function paintSvg(el, ctx) {
    const { w, h, dpr, borderRadius = 0 } = ctx;
    const clone = el.cloneNode(true);
    clone.setAttribute('width', String(w)); clone.setAttribute('height', String(h));
    if (!clone.getAttribute('xmlns')) clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    const markup = new XMLSerializer().serializeToString(clone);
    let img;
    try { img = await loadImage('data:image/svg+xml;charset=utf-8,' + encodeURIComponent(markup)); }
    catch (e) { return { ok: false, reason: 'svg serialize/decode failed: ' + e.message }; }
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    g.drawImage(img, 0, 0, cv.width, cv.height);
    return { ok: true, canvas: cv };
  }

  // text and CSS boxes -- the foreignObject path landing.js's raster() already
  // uses for the whole stage: fontsReady first, then the element's own
  // subtree plus the page's <style> sheets serialized into one SVG image, at
  // the caller's box size (radius, clip, transform and z-order are the
  // caller's job -- see the composite() note below).
  async function paintBox(el, ctx) {
    const { w, h, dpr } = ctx;
    if (document.fonts) { try { await document.fonts.ready; } catch (e) { /* a face that never arrives is not worth a hung capture */ } }
    const host = document.createElement('div');
    host.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml');
    host.setAttribute('style', `position:relative;width:${w}px;height:${h}px;overflow:hidden;`);
    const clone = el.cloneNode(true);
    // Rotation is the compositor's job (compositeOnto), the same split
    // drawPlates already uses for an <img> plate: a painter returns its own
    // box unrotated, in its own local coordinates, and the caller turns it
    // about the box's centre when placing it. This member's own stylesheet
    // rule still applies to the clone (styleNodes below), so without this
    // the foreignObject would render it pre-rotated and compositeOnto would
    // then rotate that already-rotated image a second time.
    clone.style.transform = 'none';
    // The live element's own size can come from an ancestor-scoped selector
    // (an id, a `parent > child` combinator) that only matches inside its
    // real tree -- cloned in isolation here, such a rule stops matching and
    // the clone collapses to its content's intrinsic size. Any absolutely
    // positioned children (as this fixture's own box has) contribute
    // nothing to that intrinsic size, so the box silently shrinks to 0x0
    // and `overflow` hides everything inside it -- a blank print with no
    // error anywhere. Pinning the clone's own box to the size this painter
    // was asked for removes the dependency on which selector granted it.
    clone.style.width = w + 'px'; clone.style.height = h + 'px';
    host.appendChild(clone);
    const styleNodes = [...document.querySelectorAll('style')].map((s) => s.cloneNode(true));
    const svgMarkup = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><foreignObject width="100%" height="100%">${new XMLSerializer().serializeToString(wrap(styleNodes, host))}</foreignObject></svg>`;
    let img;
    try { img = await loadImage('data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svgMarkup)); }
    catch (e) { return { ok: false, reason: 'box raster failed: ' + e.message }; }
    const cv = offscreen(w * dpr, h * dpr);
    cv.getContext('2d').drawImage(img, 0, 0, cv.width, cv.height);
    return { ok: true, canvas: cv };
  }
  function wrap(styleNodes, host) {
    const box = document.createElement('div'); box.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml');
    styleNodes.forEach((s) => box.appendChild(s));
    box.appendChild(host);
    return box;
  }

  // same-origin iframe document -- the document's own root, scripts and
  // frames stripped, through the same foreignObject path. Cross-origin
  // access throws or reads back null; either way, a declared failure.
  async function paintIframe(el, ctx) {
    const { w, h, dpr, borderRadius = 0 } = ctx;
    let doc;
    try { doc = el.contentDocument; } catch (e) { return { ok: false, reason: 'cross-origin iframe: ' + e.message }; }
    if (!doc || !doc.documentElement) return { ok: false, reason: 'iframe has no same-origin document yet' };
    if (doc.fonts) { try { await doc.fonts.ready; } catch (e) {} }
    const root = doc.documentElement.cloneNode(true);
    root.querySelectorAll('script,noscript').forEach((n) => n.remove());
    const host = document.createElement('div');
    host.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml');
    host.setAttribute('style', `position:relative;width:${w}px;height:${h}px;overflow:hidden;`);
    host.appendChild(root);
    const svgMarkup = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><foreignObject width="100%" height="100%">${new XMLSerializer().serializeToString(host)}</foreignObject></svg>`;
    let img;
    try { img = await loadImage('data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svgMarkup)); }
    catch (e) { return { ok: false, reason: 'iframe raster failed: ' + e.message }; }
    const cv = offscreen(w * dpr, h * dpr);
    const g = cv.getContext('2d');
    clipRadius(g, cv.width, cv.height, borderRadius * dpr);
    g.drawImage(img, 0, 0, cv.width, cv.height);
    return { ok: true, canvas: cv };
  }

  const painters = { img: paintImg, canvas2d: paintCanvas2d, webgl: paintWebgl, video: paintVideo, svg: paintSvg, iframe: paintIframe, box: paintBox };

  // Kind is read from data-capture-kind when a caller sets it (the fixture
  // does, so a CANVAS that already holds a live context is never re-probed
  // with getContext() of a different type just to guess -- that call can
  // itself create a context, which is not a read). Falls back to tag name.
  function classify(el) {
    const declared = el.dataset && el.dataset.captureKind;
    if (declared && painters[declared]) return declared;
    const tag = el.tagName;
    if (tag === 'IMG') return 'img';
    if (tag === 'VIDEO') return 'video';
    if (tag === 'IFRAME') return 'iframe';
    if (tag === 'svg' || tag === 'SVG') return 'svg';
    if (tag === 'CANVAS') return 'canvas2d';
    return 'box';
  }

  // One member, painted by its own kind's painter, with no silent middle: a
  // painter's own declared failure and a painter that throws both become the
  // same { ok:false, reason } shape a caller can report on and fall back
  // from, and neither one propagates to abort every other member's capture.
  async function paintMember(member) {
    const painter = painters[member.kind];
    if (!painter) return { ok: false, reason: 'no painter registered for kind ' + member.kind };
    try {
      const result = await painter(member.el, member);
      if (!result || !result.ok) return { ok: false, reason: (result && result.reason) || 'painter declined' };
      return result;
    } catch (e) { return { ok: false, reason: 'painter threw: ' + (e && e.message || e) }; }
  }

  // Captures every member independently and reports on each one; never
  // throws for an individual member's failure (only a truly unexpected
  // rejection from paintMember itself, which is already caught above, so in
  // practice this resolves for every input). `members`: [{ id, el, kind?,
  // w, h, x, y, rotation?, borderRadius?, zIndex? }], sizes and positions in
  // CSS px, the grid's own coordinate space.
  async function capture(members, opts = {}) {
    const dpr = opts.dpr || 1;
    const results = [], painted = [];
    for (const m of members) {
      const kind = m.kind || classify(m.el);
      const r = await paintMember({ ...m, kind, dpr });
      results.push({ id: m.id, kind, ok: r.ok, reason: r.reason });
      if (r.ok) painted.push({ member: m, canvas: r.canvas });
    }
    return { results, painted, fallbackCount: results.filter((r) => !r.ok).length };
  }

  // Composites painted members onto a destination canvas at device pixels,
  // in z-order, applying each member's own rotation about its own box centre
  // -- the one piece of "accumulated transform" the painters themselves do
  // not apply (drawPlates already keeps rotation at the compositor for the
  // same reason: a painter's pixels are its own box, unrotated).
  function compositeOnto(destCanvas, captureResult, dpr) {
    const g = destCanvas.getContext('2d');
    const ordered = [...captureResult.painted].sort((a, b) => (a.member.zIndex || 0) - (b.member.zIndex || 0));
    for (const { member, canvas } of ordered) {
      g.save();
      const cx = (member.x + member.w / 2) * dpr, cy = (member.y + member.h / 2) * dpr;
      g.translate(cx, cy);
      if (member.rotation) g.rotate(member.rotation * Math.PI / 180);
      g.drawImage(canvas, -member.w * dpr / 2, -member.h * dpr / 2, member.w * dpr, member.h * dpr);
      g.restore();
    }
  }

  global.WatercolorCapture = { painters, classify, capture, compositeOnto };
})(window);
