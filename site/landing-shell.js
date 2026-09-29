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

  // Publish stays clickable, even with nothing on the ring: the app disables
  // it then, and the reader should still be able to try it.
  button.disabled = false;
  new MutationObserver(() => { if (button.disabled) button.disabled = false; }).observe(button, { attributes: true, attributeFilter: ['disabled'] });
  // set() is where the ring's state is read back: it is called on every frame
  // of a growth, so `onRing` is always what the reader sees.
  const set = ring?.set;
  let onRing = {};
  if (fits) ring.set = function (state) { onRing = state; return set.call(this, state); };

  // Publish opens the receipt the app shows after a notable publish, built as
  // the app's publish receipt modal builds it (the shell carries that
  // component's own stylesheet rules): "Checking your site is live…" over rows for what was
  // uploaded, the pages added and removed, and the live check, then the same
  // in-place morph the app runs when its verification lands. The rows are the
  // changes on the ring when it is clicked, named from the example site's real
  // pages (?pages=, which the landing passes). Strings are the app's, in the
  // frame's language.
  const host = params.get('target') || location.hostname;
  const url = params.get('live') || (/^https?:\/\//.test(host) ? host : `https://${host}`);
  let pages = [];
  try { pages = JSON.parse(params.get('pages') || '[]'); } catch (e) { pages = []; }
  const STRINGS = {
    en: { title_checking: 'Checking your site is live…', title_live: 'Your site is live', detail_checking: 'Checking…', detail_live: 'Home page served the new version',
      uploaded: 'Uploaded', added: 'Added', removed: 'Removed', live: 'Live', sum: '{n} files uploaded', sum_removed: '{n} files uploaded, {m} removed', sum_zero: 'Nothing new — nothing else changed',
      notfound: 'old link now shows not found', more: 'and {n} more', done: 'Done', view: 'View site', view_page: 'View page', copy: 'Copy', copied: 'Copied' },
    'zh-Hans': { title_checking: '正在确认网站已上线……', title_live: '你的站点已上线', detail_checking: '正在确认…', detail_live: '首页已返回新版本',
      uploaded: '已上传', added: '新增', removed: '已移除', live: '已上线', sum: '已上传 {n} 个文件', sum_removed: '已上传 {n} 个文件，移除 {m} 个', sum_zero: '没有新内容 — 其他内容未变',
      notfound: '旧链接现在显示“未找到”', more: '还有 {n} 页', done: '完成', view: '查看网站', view_page: '查看页面', copy: '复制', copied: '已复制' },
    'zh-Hant': { title_checking: '正在確認網站已上線……', title_live: '你的網站已上線', detail_checking: '正在確認…', detail_live: '首頁已返回新版本',
      uploaded: '已上傳', added: '新增', removed: '已移除', live: '已上線', sum: '已上傳 {n} 個檔案', sum_removed: '已上傳 {n} 個檔案，移除 {m} 個', sum_zero: '沒有新內容 — 其他內容未變',
      notfound: '舊連結現在顯示「找不到」', more: '還有 {n} 頁', done: '完成', view: '查看網站', view_page: '查看頁面', copy: '複製', copied: '已複製' },
  };
  const t = (key, values) => {
    const text = (STRINGS[document.documentElement.lang] || STRINGS.en)[key];
    return values ? text.replace(/\{(\w)\}/g, (_, name) => String(values[name])) : text;
  };
  const SVG = 'http://www.w3.org/2000/svg', VERIFY_MS = 1600, MAX_PAGE_ROWS = 3;
  let backdrop = null, modal = null, settle = 0;
  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (tag === 'button') node.type = 'button';   // before the class, the attribute order the app's builders produce
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  };
  const svg = (tag, attrs) => { const node = document.createElementNS(SVG, tag); for (const k in attrs) node.setAttribute(k, attrs[k]); return node; };
  const open = (href) => window.open(href, '_blank', 'noopener');
  const pageUrl = (path) => `https://${host.replace(/\/+$/, '')}/${path.replace(/^\/+/, '')}`;
  const displayUrl = (raw) => raw.replace(/^https?:\/\//, '').replace(/\/+$/, '');
  const mark = (state) => {
    const node = svg('svg', { class: 'mark', viewBox: '0 0 14 14', 'aria-hidden': 'true' });
    node.dataset.state = state;
    node.append(svg('circle', { class: 'mark-ring', cx: '7', cy: '7', r: '5.5' }), svg('polyline', { class: 'mark-check', points: '4.2,7.3 6.1,9.1 9.8,4.9' }), svg('line', { class: 'mark-bar', x1: '7', y1: '4.6', x2: '7', y2: '7.6' }));
    return node;
  };
  const icon = (className) => svg('svg', { width: '14', height: '14', viewBox: '0 0 14 14', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true', class: className });
  const copyButton = (value) => {
    const node = el('button', 'moss-copy-btn'), copy = icon('moss-copy-btn__icon--copy'), check = icon('moss-copy-btn__icon--check'), label = el('span', 'moss-copy-btn__label', t('copy'));
    node.setAttribute('aria-label', t('copy'));
    copy.append(svg('rect', { x: '0.5', y: '0.5', width: '9', height: '9', rx: '2' }), svg('rect', { x: '4.5', y: '4.5', width: '9', height: '9', rx: '2' }));
    check.append(svg('path', { d: 'M2 7l4 4 6-6' }));
    node.append(copy, check, label);
    let revert = 0;
    node.addEventListener('click', () => navigator.clipboard.writeText(value).then(() => {
      node.classList.add('moss-copy-btn--copied'); node.setAttribute('aria-label', t('copied')); label.textContent = t('copied');
      clearTimeout(revert);
      revert = setTimeout(() => { node.classList.remove('moss-copy-btn--copied'); node.setAttribute('aria-label', t('copy')); label.textContent = t('copy'); }, 2000);
    }).catch(() => {}));
    return node;
  };
  const row = (state, label, detail, key) => {
    const li = el('li', 'receipt-row');
    if (key) li.dataset.row = key;
    li.append(mark(state), el('span', 'receipt-label', label), typeof detail === 'string' ? el('span', 'receipt-detail', detail) : detail);
    return li;
  };
  const pageRow = (page) => {
    const full = page.kind === 'added' ? pageUrl(page.path) : null, detail = el('div', 'receipt-detail'), line1 = el('div', 'receipt-detail-line1');
    if (full) {
      const link = el('button', 'receipt-host', page.title || page.path);
      link.addEventListener('click', () => open(full));
      line1.append(link, copyButton(full));
    } else line1.textContent = page.title || page.path;
    detail.append(line1, el('div', 'receipt-detail-line2', full ? displayUrl(full) : t('notfound')));
    return row('pending', t(page.kind), detail, `page:${page.kind}:${page.path}`);
  };
  const onKey = (event) => { if (event.key === 'Escape') close(); };
  function close() {
    if (!modal) return;
    clearTimeout(settle); document.removeEventListener('keydown', onKey);
    const b = backdrop, m = modal; backdrop = modal = null;
    b.classList.remove('visible'); m.classList.remove('visible');
    setTimeout(() => { b.remove(); m.remove(); }, 200);
  }
  function show() {
    if (modal) return;
    const count = (name) => Math.round(onRing[name] || 0);
    const added = pages.filter((p) => p.kind === 'added').slice(0, count('added')), removed = pages.filter((p) => p.kind === 'removed').slice(0, count('deleted'));
    const listed = [...added, ...removed], uploaded = count('edited') + count('added') + count('restyled'), gone = count('deleted');
    backdrop = el('div', 'moss-modal-backdrop'); backdrop.addEventListener('click', close);
    modal = el('div', 'moss-modal moss-modal--receipt');
    modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true'); modal.setAttribute('aria-labelledby', 'moss-receipt-title-0');
    modal.addEventListener('click', (event) => event.stopPropagation());
    const stack = el('div', 'moss-stack'), title = el('h2', 'moss-modal-title', t('title_checking'));
    title.id = 'moss-receipt-title-0';
    const address = el('div', 'receipt-address'), hostLink = el('button', 'receipt-host', host);
    hostLink.addEventListener('click', () => open(url));
    address.append(hostLink, copyButton(host));
    const list = el('ul', 'receipt');
    list.append(row('done', t('uploaded'), gone > 0 ? t('sum_removed', { n: uploaded, m: gone }) : uploaded === 0 ? t('sum_zero') : t('sum', { n: uploaded })));
    listed.slice(0, MAX_PAGE_ROWS).forEach((page) => list.append(pageRow(page)));
    if (listed.length > MAX_PAGE_ROWS) {
      const more = el('li', 'receipt-row');
      more.append(el('span'), el('span'), el('span', 'receipt-detail-line2', t('more', { n: listed.length - MAX_PAGE_ROWS })));
      list.append(more);
    }
    list.append(row('pending', t('live'), t('detail_checking'), 'live'));
    // the app's primary is "View page" when exactly one added page shows
    const one = added.length === 1 && listed.indexOf(added[0]) < MAX_PAGE_ROWS;
    const actions = el('div', 'moss-modal-actions'), done = el('button', 'moss-btn moss-btn-secondary', t('done')), view = el('button', 'moss-btn moss-btn-primary');
    done.setAttribute('data-action', 'done'); done.addEventListener('click', close);
    view.setAttribute('data-action', one ? 'view-page' : 'view'); view.textContent = t(one ? 'view_page' : 'view');
    view.addEventListener('click', () => open(one ? pageUrl(added[0].path) : url));
    actions.append(done, view);
    stack.append(title, address, list, actions); modal.append(stack);
    document.body.append(backdrop, modal);
    backdrop.classList.add('visible'); modal.classList.add('visible');
    document.addEventListener('keydown', onKey);
    view.focus({ preventScroll: true });
    shell.publishFlash();
    // the verdict: every mark settles, and the title fades through to the new one
    settle = setTimeout(() => {
      modal.querySelectorAll('svg.mark').forEach((node) => { node.dataset.state = 'done'; });
      modal.querySelector('[data-row="live"] .receipt-detail').textContent = t('detail_live');
      title.classList.add('fading');
      setTimeout(() => { title.textContent = t('title_live'); title.classList.remove('fading'); }, 100);
    }, VERIFY_MS);
  }
  button.addEventListener('click', show);
})();
