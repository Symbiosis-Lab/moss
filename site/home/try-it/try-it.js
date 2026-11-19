// Try moss on the page: the visitor's notes and pictures become moss pages,
// drawn by moss's own renderer compiled to WebAssembly. Nothing is uploaded.
// The renderer loads on the first press or drop, never before.
import { zipFolder, writeFolder, withPicture } from './folder.js';

const HERE = new URL('./', import.meta.url);
const locale = document.documentElement.lang || 'en';
const fine = matchMedia('(pointer: fine)').matches;
const PAGE = /\.(md|markdown)$/i;
const HEIC = /\.(heic|heif)$/i;

let engine = null;
function loadEngine() {
  engine ??= (async () => {
    const wasm = await import(new URL('wasm/moss_bindings.js', HERE));
    await wasm.default({ module_or_path: new URL('wasm/moss_bindings_bg.wasm', HERE) });
    return wasm;
  })();
  engine.catch(() => { engine = null; });
  return engine;
}

// Runs inside the preview: draws the visitor's pictures from the files the
// page posts, and hands links between pages back to it.
const FRAME_SCRIPT = `
addEventListener('message', (e) => {
  if (e.source !== parent || !e.data.pictures) return;
  for (const img of document.querySelectorAll('img[data-file]')) {
    const blob = e.data.pictures[img.dataset.file];
    if (blob) img.src = URL.createObjectURL(blob);
  }
});
document.addEventListener('click', (e) => {
  const link = e.target.closest('a[href]');
  if (!link) return;
  const url = new URL(link.getAttribute('href'), 'https://site.invalid/');
  if (url.origin !== 'https://site.invalid') { link.target = '_blank'; link.rel = 'noopener'; return; }
  e.preventDefault();
  parent.postMessage({ tryItPage: url.pathname }, '*');
});
parent.postMessage({ tryIt: 'ready' }, '*');
`;

const el = (tag, props = {}, ...kids) => {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...kids);
  return node;
};

// A button whose press may wait on the renderer: moss's loading button.
function slotButton(label, role, onPress) {
  const button = el('button', { type: 'button', className: 'moss-btn' },
    el('span', { className: 'moss-btn__label', textContent: label }),
    el('span', { className: 'moss-btn__spinner', ariaHidden: 'true' }));
  button.dataset.role = role;
  // moss's stylesheet reads the state from an element around the slot.
  const holder = el('span', { className: 'try-it-slot' }, el('span', { className: 'moss-btn-slot' }, button));
  holder.dataset.state = 'idle';
  button.addEventListener('click', async () => {
    if (holder.dataset.state === 'loading') return;
    holder.dataset.state = 'loading';
    button.setAttribute('aria-disabled', 'true');
    try { await onPress(); } finally {
      holder.dataset.state = 'idle';
      button.removeAttribute('aria-disabled');
    }
  });
  return holder;
}

export function mount(root) {
  // The host page's words, one per [data-word], so each language's page carries its own.
  const T = Object.fromEntries([...root.querySelector('template[data-try-it-words]')?.content.querySelectorAll('[data-word]') ?? []]
    .map((word) => [word.dataset.word, word.textContent]));
  const folderName = T.folder;
  const files = new Map([['index.md', { text: '' }]]);
  let open = 'index.md';
  let site = null;
  let ready = null;

  const status = el('p', { className: 'try-it-status', role: 'status' });
  const say = (words) => { status.textContent = words; };

  const list = el('ul', { className: 'try-it-files' });
  list.setAttribute('aria-label', T.files);
  const frame = el('iframe', { title: T.preview, src: new URL(`sample-${locale}.html`, HERE).href });
  // An opaque origin: whatever the visitor's own files run cannot reach this page.
  frame.setAttribute('sandbox', 'allow-scripts allow-popups allow-popups-to-escape-sandbox');
  const text = el('textarea', { className: 'try-it-text', hidden: true, spellcheck: true });
  text.setAttribute('aria-label', T.text);
  const path = el('span', { className: 'try-it-path', textContent: `${folderName}/index.md` });

  const picker = el('input', { type: 'file', hidden: true, multiple: true });
  // A phone offers its camera only while the picker asks for pictures alone.
  picker.accept = fine ? 'image/*,.md,.markdown,text/markdown' : 'image/*';

  const toggle = el('button', { type: 'button', className: 'try-it-toggle', textContent: T.write });
  toggle.setAttribute('aria-pressed', 'false');

  // The sample folder as moss would hold it, read when the visitor first acts.
  function start() {
    ready ??= (async () => {
      const [wasm, sample] = await Promise.all([
        loadEngine(),
        fetch(new URL(`samples/${locale}.txt`, HERE)).then((r) => r.text()),
      ]);
      files.get('index.md').text = sample;
      root.dataset.state = 'live';
      return wasm;
    })();
    ready.catch(() => { ready = null; say(T.failed); });
    return ready;
  }

  const paths = () => [...files.keys()];
  function rebuild(wasm) {
    site?.free();
    const texts = Object.fromEntries([...files].filter(([, f]) => 'text' in f).map(([p, f]) => [p, f.text]));
    const { bundle } = JSON.parse(wasm.siteBundle(JSON.stringify({ files: paths(), texts, detected_lang: locale })));
    site = new wasm.MossSite(JSON.stringify(bundle));
    site.setNotes(JSON.stringify(texts));
    site.setTexts(JSON.stringify(texts), folderName);
  }

  async function render() {
    const wasm = await start();
    rebuild(wasm);
    const page = JSON.parse(site.preview(open, files.get(open).text, '/'));
    const doc = new DOMParser().parseFromString(page.html, 'text/html');
    for (const img of doc.querySelectorAll('img[src]')) {
      const name = decodeURIComponent(new URL(img.getAttribute('src'), 'https://site.invalid/').pathname.slice(1));
      if (!files.get(name)?.blob) continue;
      img.dataset.mossSrc = img.getAttribute('src');
      img.dataset.file = name;
      img.removeAttribute('src');
    }
    doc.head.append(Object.assign(doc.createElement('script'), { textContent: FRAME_SCRIPT }));
    frame.srcdoc = `<!doctype html>${doc.documentElement.outerHTML}`;
    path.textContent = `${folderName}/${open}`;
    drawList();
  }

  // The page a link in the preview names. moss writes a page's link as its
  // path without `.md` (`Second note/`), and the home's as `./`.
  function pageAt(pathname) {
    const target = decodeURIComponent(pathname).replace(/^\/+|\/+$/g, '');
    if (!target) return 'index.md';
    return paths().find((p) => PAGE.test(p) && (p === target || p.replace(PAGE, '') === target));
  }

  addEventListener('message', (event) => {
    if (event.source !== frame.contentWindow) return;
    if (event.data?.tryIt === 'ready') {
      const pictures = Object.fromEntries([...files].filter(([, f]) => f.blob).map(([p, f]) => [p, f.blob]));
      frame.contentWindow.postMessage({ pictures }, '*');
    } else if (typeof event.data?.tryItPage === 'string') {
      const target = pageAt(event.data.tryItPage);
      if (target) show(target);
    }
  });

  function drawList() {
    list.replaceChildren(...paths().map((p) => {
      const isPage = PAGE.test(p);
      const item = el(isPage ? 'button' : 'span', { className: 'try-it-file', textContent: p, title: p });
      if (isPage) {
        item.type = 'button';
        if (p === open) item.setAttribute('aria-current', 'page');
        item.addEventListener('click', () => show(p));
      }
      return el('li', {}, item);
    }));
  }

  function setMode(writing) {
    toggle.setAttribute('aria-pressed', String(writing));
    toggle.textContent = writing ? T.preview : T.write;
    text.hidden = !writing;
    frame.hidden = writing;
    if (writing) { text.value = files.get(open).text; text.focus(); }
  }

  async function show(p) {
    open = p;
    await render();
    if (!text.hidden) setMode(true);
  }

  async function asJpeg(file) {
    if (typeof createImageBitmap !== 'function') return null;
    try {
      const bitmap = await createImageBitmap(file);
      const canvas = el('canvas', { width: bitmap.width, height: bitmap.height });
      canvas.getContext('2d').drawImage(bitmap, 0, 0);
      const blob = await new Promise((done) => canvas.toBlob(done, 'image/jpeg', 0.9));
      return blob && new File([blob], file.name.replace(HEIC, '.jpg'), { type: 'image/jpeg' });
    } catch { return null; }
  }

  async function add(chosen) {
    stage.ariaBusy = 'true';
    try { await addFiles(chosen); } finally { stage.removeAttribute('aria-busy'); }
  }

  async function addFiles(chosen) {
    const wasm = await start();
    const images = new Set(JSON.parse(wasm.imageExtensions()));
    let refused = false;
    for (let file of chosen) {
      if (HEIC.test(file.name) || /image\/hei[cf]/.test(file.type)) file = await asJpeg(file) ?? file;
      const ext = (file.name.match(/\.([^.]+)$/)?.[1] ?? '').toLowerCase();
      if (PAGE.test(file.name)) {
        files.set(file.name, { text: await file.text() });
        open = file.name;
      } else if (images.has(ext)) {
        const name = wasm.freeName('', file.name, JSON.stringify(paths()));
        files.set(name, { blob: file });
        const page = files.get(open);
        page.text = withPicture(page.text, wasm.relativeDestination(open, name));
      } else refused = true;
    }
    say(refused ? T.refused : '');
    setMode(false);
    await render();
  }

  picker.addEventListener('change', () => { if (picker.files.length) add([...picker.files]); picker.value = ''; });

  const pick = el('button', { type: 'button', className: 'moss-btn', textContent: fine ? T.add : T.pick });
  pick.dataset.role = 'primary';
  // The picker opens inside the press; the renderer starts loading beside it.
  pick.addEventListener('click', () => { picker.click(); start(); });

  const sample = slotButton(T.sample, 'secondary', async () => {
    const [blob] = await Promise.all([fetch(new URL('samples/fish.jpg', HERE)).then((r) => r.blob()), start()]);
    await add([new File([blob], T.sampleName, { type: 'image/jpeg' })]);
  });

  const fresh = slotButton(T.newPage, 'secondary', async () => {
    const wasm = await start();
    const name = wasm.freeName('', `${T.untitled}.md`, JSON.stringify(paths()));
    files.set(name, { text: '' });
    open = name;
    await render();
    setMode(true);
  });

  let typing = 0;
  text.addEventListener('input', () => {
    files.get(open).text = text.value;
    clearTimeout(typing);
    typing = setTimeout(render, 150);
  });
  toggle.addEventListener('click', async () => {
    const writing = toggle.getAttribute('aria-pressed') !== 'true';
    stage.ariaBusy = 'true';
    try { await start(); } finally { stage.removeAttribute('aria-busy'); }
    if (!writing) await render();
    setMode(writing);
  });

  const contents = () => paths().map((p) => ({ path: p, data: files.get(p).text ?? files.get(p).blob }));
  const zip = slotButton(T.zip, 'primary', async () => {
    await start();
    const link = el('a', { href: URL.createObjectURL(await zipFolder(folderName, contents())), download: `${folderName}.zip` });
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 60_000);
  });
  const take = el('div', { className: 'try-it-take' }, zip);
  if (typeof window.showDirectoryPicker === 'function') {
    take.append(slotButton(T.save, 'secondary', async () => {
      let dir;
      try { dir = await window.showDirectoryPicker({ mode: 'readwrite' }); } catch { return; }
      await start();
      await writeFolder(dir, folderName, contents());
      say(T.saved.replace('{folder}', `${dir.name}/${folderName}`));
    }));
  }
  const moss = el('a', { className: 'moss-btn', href: '#downloads', textContent: T.download });
  moss.dataset.role = 'secondary';
  take.append(moss);

  const stage = el('div', { className: 'try-it-stage' }, frame, text);
  if (fine) stage.append(el('p', { className: 'try-it-drop', textContent: T.drop, ariaHidden: 'true' }));
  root.replaceChildren(
    el('div', { className: 'try-it-folder' },
      el('p', { className: 'try-it-site', textContent: folderName }),
      list,
      el('div', { className: 'try-it-add' }, pick, sample, fresh, picker)),
    el('div', { className: 'try-it-view' }, el('div', { className: 'try-it-bar' }, path, toggle), stage),
    take,
    el('p', { className: 'try-it-local', textContent: T.local }),
    status);
  root.dataset.state = 'sample';
  drawList();

  if (fine) {
    // Over the preview, drag events go to its document, so it steps aside while a file is dragged.
    let depth = 0;
    const carriesFiles = (e) => [...(e.dataTransfer?.types ?? [])].includes('Files');
    root.addEventListener('dragenter', (e) => { if (!carriesFiles(e)) return; depth++; root.dataset.drag = 'over'; start(); });
    root.addEventListener('dragleave', () => { if (--depth <= 0) { depth = 0; delete root.dataset.drag; } });
    root.addEventListener('dragover', (e) => { if (carriesFiles(e)) e.preventDefault(); });
    root.addEventListener('drop', (e) => {
      if (!carriesFiles(e)) return;
      e.preventDefault();
      depth = 0;
      delete root.dataset.drag;
      add([...e.dataTransfer.files]);
    });
  }
}

for (const root of document.querySelectorAll('[data-try-it]')) mount(root);
