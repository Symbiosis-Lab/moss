// Mock data for editor Playwright and Vitest tests.
// Mirrors the prototype's Chinese personal site data structure.

import type { ContentSchema, JsonValue, WikilinkCompletion } from '@symbiosis-lab/moss-ui';
import type { ParsedFrontmatter } from '@symbiosis-lab/moss-ui';
import type { PageFacts, TreeNode } from '@symbiosis-lab/moss-ui';

// ── Tree ────────────────────────────────────────────────────────────────

// NOTE: every node carries `is_home: false` — an output the real backend
// would not produce here (each folder has documents, so the election would
// flag one child per folder; the root would stay unflagged only because its
// candidates are fallback-tier). Deliberate: these fixtures exercise tree
// LAYOUT/interaction, not home election; election realism lives in the Rust
// is_home_* tests and file-tree-home-parity.test.ts.
export const MOCK_TREE: TreeNode = {
  name: '刘果', path: '/mock/刘果', is_dir: true, modified: 1700000900, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null,
  children: [
    { name: '文字', path: '/mock/刘果/文字', is_dir: true, modified: 1700000800, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: [
      { name: '纽约诸法门', path: '/mock/刘果/文字/纽约诸法门', is_dir: true, modified: 1700000700, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: [
        { name: '冬至的饺子.md', path: '/mock/刘果/文字/纽约诸法门/冬至的饺子.md', is_dir: false, modified: 1700000700, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
        { name: '藏传佛教寺庙里的瑜伽.md', path: '/mock/刘果/文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md', is_dir: false, modified: 1700000600, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
        { name: '纽约的禅修中心.md', path: '/mock/刘果/文字/纽约诸法门/纽约的禅修中心.md', is_dir: false, modified: 1700000500, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
        { name: '中央公园的太极.md', path: '/mock/刘果/文字/纽约诸法门/中央公园的太极.md', is_dir: false, modified: 1700000400, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
      ]},
      { name: '旅行随笔', path: '/mock/刘果/文字/旅行随笔', is_dir: true, modified: 1700000800, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: [
        { name: '京都之秋.md', path: '/mock/刘果/文字/旅行随笔/京都之秋.md', is_dir: false, modified: 1700000800, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
        { name: '伊斯坦布尔.md', path: '/mock/刘果/文字/旅行随笔/伊斯坦布尔.md', is_dir: false, modified: 1700000300, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
      ]},
      { name: '读书笔记', path: '/mock/刘果/文字/读书笔记', is_dir: true, modified: 1700000200, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: [
        { name: '金刚经.md', path: '/mock/刘果/文字/读书笔记/金刚经.md', is_dir: false, modified: 1700000200, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
      ]},
    ]},
    { name: '项目', path: '/mock/刘果/项目', is_dir: true, modified: 1700000900, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: [
      { name: 'moss.md', path: '/mock/刘果/项目/moss.md', is_dir: false, modified: 1700000900, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
      { name: 'indie-reader.md', path: '/mock/刘果/项目/indie-reader.md', is_dir: false, modified: 1700000100, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
    ]},
    { name: '关于.md', path: '/mock/刘果/关于.md', is_dir: false, modified: 1700000050, publish_date: null, date_source: "none", is_home: false, hidden: 0, file_id: null, children: null },
  ],
};

// ── File Contents ───────────────────────────────────────────────────────

export const MOCK_FILES: Record<string, { frontmatter: Record<string, unknown>; body: string }> = {
  '/mock/刘果/文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md': {
    frontmatter: {
      title: '藏传佛教寺庙里的瑜伽',
      date: '2024-11-07',
      tags: ['历史', '瑜伽', '藏传佛教'],
      cover: '[[4ed2502b...809.png]]',
      draft: false,
    },
    body: '## 一\n\n西藏之家位于繁华的纽约第五大道边。\n\n**瑜伽**和*冥想*课程。\n\n> [!tip] 小贴士\n> 西藏之家周三和周四有约翰的课。\n\n是[[詹米]]。\n',
  },
  '/mock/刘果/文字/纽约诸法门/冬至的饺子.md': {
    frontmatter: {
      title: '冬至的饺子',
      date: '2024-12-21',
      tags: ['纽约', '食物'],
    },
    // The grid feeds media-density.spec.ts, which measures that a fence body's
    // image is one line tall and STAYS one line when the caret enters it.
    body: '## 冬至\n\n在纽约包饺子。\n\n:::grid\n![](dumplings.jpg)\n+++\n![](wonton.jpg)\n:::\n\n![](dumplings.jpg)\n',
  },
  '/mock/刘果/关于.md': {
    frontmatter: { title: '关于' },
    body: '# 刘果\n\n写字的人。\n',
  },
};

// ── Schema ──────────────────────────────────────────────────────────────

export const MOCK_SCHEMA: ContentSchema = {
  generator: 'moss',
  version: '1.0',
  frontmatter: {
    fields: {
      title: { type: 'string', widget: 'text-input', required: true, description: 'Page title' },
      date: { type: 'string', widget: 'date-picker', format: 'date', description: 'Publication date (YYYY-MM-DD)' },
      weight: { type: 'integer', widget: 'number-input', description: 'Sort weight for ordering' },
      url: { type: 'string', widget: 'text-input', description: 'Custom URL path override' },
      cover: { type: 'string', widget: 'file-picker', description: 'Cover image path' },
      nav: { type: 'string', widget: 'text-input', description: 'Navigation label override' },
      draft: { type: 'boolean', widget: 'checkbox', description: 'Not published. Hidden from your site; visible in preview.' },
      description: { type: 'string', widget: 'text-area', description: 'Page excerpt for SEO meta, og:description, and list previews' },
      tags: { type: 'array', widget: 'tag-input', items: { type: 'string' }, description: 'Content tags for organization' },
      children: { type: 'boolean', widget: 'checkbox', default: true, description: 'Whether to render child pages below content' },
      children_style: { type: 'string', widget: 'select', enum_values: ['list', 'summary'], default: 'list', description: 'How child pages are rendered' },
      children_group: { type: 'string', widget: 'select', enum_values: ['year', 'none'], description: 'How children are grouped' },
      children_depth: { type: 'string', widget: 'select', enum_values: ['direct', 'all'], default: 'direct', description: 'Depth of child listing' },
      lang: { type: 'string', widget: 'text-input', description: 'Language code (e.g. en, zh)' },
      series: { type: 'array', widget: 'tag-input', items: { type: 'string' }, description: 'Declares children as sequential series' },
      breadcrumb: { type: 'boolean', widget: 'checkbox', description: 'Override site-wide breadcrumb setting' },
      footer: { type: 'boolean', widget: 'checkbox', description: 'Show as a link in the site footer' },
      sidebar: { type: 'string', widget: 'text-input', description: 'Wikilink to folder for sidebar' },
      also_in: { type: 'array', widget: 'tag-input', items: { type: 'string' }, description: 'Cross-list this page in other sections' },
      translation_key: { type: 'string', widget: 'text-input', description: 'Key to link translations' },
    },
  },
  shortcodes: {
    delimiters: [':::', ':::'],
    definitions: {},
  },
};

// ── Mock Commands ───────────────────────────────────────────────────────

export let lastSaved: { path: string; fm: JsonValue; body: string } | null = null;
export function resetSaved(): void { lastSaved = null; }

// ── Versions (publish history) ─────────────────────────────────────────
// Fixture for the Versions mode redesign: paths relative to
// MOCK_TREE's root, `toRelative`'s own convention, so a folder-scope open on
// e.g. '文字/纽约诸法门' filters the same way the real vault-relative paths
// would. Real command shapes: the engine's `listVersions` / `saveVersion` /
// `restoreVersion` / `readVersion` / `revealHistoryStore`.

const VERSION_FILES = [
  '关于.md',
  '文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md',
  '文字/纽约诸法门/冬至的饺子.md',
  '文字/纽约诸法门/纽约的禅修中心.md',
  '文字/纽约诸法门/中央公园的太极.md',
  '文字/旅行随笔/京都之秋.md',
  '文字/旅行随笔/伊斯坦布尔.md',
  '文字/读书笔记/金刚经.md',
  '项目/moss.md',
  '项目/indie-reader.md',
];

interface MockVersionRecord {
  id: string;
  publishedAt: string;
  trigger: 'publish' | 'manual' | 'restore';
  label: string | null;
  live: boolean;
  /** Files this record's content differs on from the previous record's —
   *  drives both the folder-scope timeline filter and the drill-down's
   *  changed_since/unchanged split. Real values come from
   *  `change_set::diff_hashes`; this is a plausible stand-in per record. */
  changed: string[];
}

const BASE_VERSION_RECORDS: MockVersionRecord[] = [
  { id: 'r1', publishedAt: '2026-09-16T11:20:00Z', trigger: 'publish', label: null, live: true, changed: ['关于.md', '文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md'] },
  { id: 'r2', publishedAt: '2026-09-16T09:05:00Z', trigger: 'manual', label: '重写前', live: false, changed: ['关于.md'] },
  { id: 'r3', publishedAt: '2026-09-15T16:40:00Z', trigger: 'publish', label: null, live: false, changed: ['文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md', '项目/moss.md'] },
  { id: 'r4', publishedAt: '2026-09-14T10:00:00Z', trigger: 'publish', label: null, live: false, changed: ['关于.md', '文字/旅行随笔/京都之秋.md'] },
  { id: 'r5', publishedAt: '2026-09-10T08:12:00Z', trigger: 'restore', label: '恢复前的样子', live: false, changed: ['关于.md'] },
  { id: 'r6', publishedAt: '2026-09-08T11:20:00Z', trigger: 'publish', label: null, live: false, changed: ['关于.md'] },
  { id: 'r7', publishedAt: '2026-09-03T14:02:00Z', trigger: 'publish', label: null, live: false, changed: ['文字/读书笔记/金刚经.md'] },
  { id: 'r8', publishedAt: '2026-08-29T09:47:00Z', trigger: 'publish', label: null, live: false, changed: ['关于.md', '项目/indie-reader.md'] },
];

const MOCK_PROJECT_ROOT = '/mock/刘果';

// Manual saves made during a harness session, newest first — same idea as
// `lastSaved` above, a tiny bit of in-memory state the mock is allowed to
// carry across calls within one page load.
let extraVersionRecords: MockVersionRecord[] = [];
// What a restore actually WROTE, keyed by the absolute path `parseFrontmatter`
// reads by (MOCK_FILES' own keys) — without this, `restoreVersion` returning
// "ok" changed nothing a reopened file could show, which reads as "restore
// doesn't work" even though the call itself succeeded.
let fileOverrides: Record<string, { frontmatter: Record<string, unknown>; body: string }> = {};
export function resetVersions(): void { extraVersionRecords = []; fileOverrides = {}; }

/** The fake body `readVersion` shows and `restoreVersion` writes for `path`
 *  at `rec` — the same text both ways, so what you read is what you get. */
function versionText(rec: MockVersionRecord, path: string): string {
  return `# ${path.split('/').pop()}\n\n(mock content for "${path}" as of ${rec.publishedAt})\n`;
}

function allVersionRecords(): MockVersionRecord[] {
  return [...extraVersionRecords, ...BASE_VERSION_RECORDS];
}

function underFolder(path: string, folder: string): boolean {
  return path === folder || path.startsWith(`${folder}/`);
}

/** `folder === null` counts the whole site (site scope); otherwise only the
 *  files under `folder` (folder scope) — the summary line moss-versions
 *  renders needs the SCOPED count, not the record's site-wide one. */
function toVersionRow(rec: MockVersionRecord, folder: string | null) {
  const relevant = folder === null ? rec.changed : rec.changed.filter((p) => underFolder(p, folder));
  return {
    id: rec.id,
    published_at: rec.publishedAt,
    target: 'mock-target-1',
    trigger: rec.trigger,
    label: rec.label,
    changed: relevant.length,
    added: 0,
    removed: 0,
  };
}

export const mockCommands = {
  getSchema: async () => ({ status: 'ok' as const, data: MOCK_SCHEMA }),
  getProjectPath: async () => ({ status: 'ok' as const, data: '/mock/刘果' }),
  pageFacts: async (_filePath: string) => ({
    status: 'ok' as const,
    data: { position: 'page', auto: {} } satisfies PageFacts,
  }),
  // Batched cold-start bootstrap (editor-main.ts:456) — replaced the serial
  // getSchema → getProjectPath → parseFrontmatter chain. Mirror those three
  // mock values; `initial_file: null` makes the frontend fall back to
  // parseFrontmatter (below), reproducing the pre-bootstrap harness path.
  editorBootstrap: async (_sourcePath: string | null) => ({
    status: 'ok' as const,
    data: {
      schema: MOCK_SCHEMA,
      project_path: '/mock/刘果',
      initial_file: null,
      // Without this, editor-main.ts's `rootName` stays null and every
      // site-scope surface (Versions' root title, the create-flow's root
      // basename) silently renders blank in this harness — a real vault
      // always has one (`VaultRoot::name`).
      root_name: '刘果',
    },
  }),
  listTree: async (_showInternal: boolean) => ({ status: 'ok' as const, data: MOCK_TREE }),
  parseFrontmatter: async (filePath: string) => {
    const file = fileOverrides[filePath] || MOCK_FILES[filePath] || MOCK_FILES[Object.keys(MOCK_FILES)[0]];
    return {
      status: 'ok' as const,
      data: {
        hash: '',
        frontmatter: file.frontmatter as unknown as JsonValue,
        body: file.body,
        has_frontmatter: true,
      } satisfies ParsedFrontmatter,
    };
  },
  saveEditorContent: async (filePath: string, frontmatter: JsonValue, body: string, _expectedHash: string) => {
    lastSaved = { path: filePath, fm: frontmatter, body };
    return { status: 'ok' as const, data: { status: 'saved' as const, hash: '' } };
  },
  resolveUrlForFile: async (filePath: string) => ({
    status: 'ok' as const,
    data: filePath.replace('/mock/刘果', '').replace('.md', '') || '/',
  }),
  getFileInfo: async (_filePath: string) => ({
    status: 'ok' as const,
    data: { size: '1024', modified: '2026-01-01T00:00:00Z', width: 800, height: 600 },
  }),
  // The engine's destination for a landed file, written from the page. This
  // stand-in covers the harness's shapes: a file in the page's folder or below.
  assetRefForPath: async (absPath: string, fromPage: string) => {
    const target = absPath.replace('/mock/刘果/', '');
    const dir = fromPage.slice(0, fromPage.lastIndexOf('/') + 1);
    const insert = target.startsWith(dir) ? target.slice(dir.length) : target;
    return {
      status: 'ok' as const,
      data: { insert, label: insert, detail: null, kind: 'page' } satisfies WikilinkCompletion,
    };
  },
  // Reference resolution — used by the image-repaint visual spec. An embed
  // target with an image extension resolves to an Image envelope (so the
  // live-preview renders an <img>); an audio extension resolves to an Audio
  // envelope (so the live-preview renders the native <audio> player widget —
  // exercised by the widget-height-drift gate); everything else is not-found.
  // Additive: other harness specs don't embed media so this is inert for them.
  editorResolveReferences: async (
    targets: { text: string; is_embed: boolean }[],
    _fromFile: string,
  ) => {
    const IMG = /\.(jpe?g|png|gif|webp|avif)$/i;
    const AUDIO = /\.(mp3|m4a|wav|ogg|flac)$/i;
    const VIDEO = /\.(mp4|mov|webm|m4v)$/i;
    // Card kinds: the placeholder-card branch of MediaEmbedBlockWidget. The
    // backend never reports dimensions for these (asset_resolver only probes
    // image/*), so `resolved` carries null width/height exactly as in prod.
    // Mime strings mirror `mime_for_path` (crates/moss-build/src/editor/resolve/asset_resolver.rs)
    // exactly, including its lack of an `html` arm — .html falls through to
    // application/octet-stream there, so it must here too. A mock that reports a
    // mime the backend never sends makes every gate built on it worthless.
    const CARD: [RegExp, string, string][] = [
      [/\.pdf$/i, 'pdf', 'application/pdf'],
      [/\.ipynb$/i, 'notebook', 'application/x-ipynb+json'],
      [/\.html?$/i, 'iframe', 'application/octet-stream'],
    ];
    const data = targets.map((t) => {
      const isImg = t.is_embed && IMG.test(t.text);
      const isAudio = t.is_embed && AUDIO.test(t.text);
      const isVideo = t.is_embed && VIDEO.test(t.text);
      const card = t.is_embed ? CARD.find(([re]) => re.test(t.text)) : undefined;
      // A trailing slash is a folder listing (the classifier's folder arm).
      const isFolder = t.is_embed && /\/$/.test(t.text);
      const requestUrl = '/assets/' + t.text.replace(/^.*[\\/]/, '');
      let kind: { kind: string } = { kind: 'not-found' };
      if (isImg) kind = { kind: 'image' };
      else if (isAudio) kind = { kind: 'audio' };
      else if (isVideo) kind = { kind: 'video' };
      else if (isFolder) kind = { kind: 'folder-listing' };
      else if (card) kind = { kind: card[1] };
      // `absolute_path` is what FOLLOWING an asset embed opens, so a mock that
      // omitted it would render a card advertising nothing and quietly make
      // every affordance gate built on this harness vacuous — the same trap the
      // mime-string note above is about.
      const absolutePath = '/project' + requestUrl;
      let resolved: Record<string, unknown> | null = null;
      if (isImg) resolved = { absolute_path: absolutePath, request_url: requestUrl, mime_type: 'image/jpeg', width: 800, height: 600 };
      else if (isAudio) resolved = { absolute_path: absolutePath, request_url: requestUrl, mime_type: 'audio/mpeg', width: null, height: null };
      else if (isVideo) resolved = { absolute_path: absolutePath, request_url: requestUrl, mime_type: 'video/mp4', width: null, height: null };
      else if (card) resolved = { absolute_path: absolutePath, request_url: requestUrl, mime_type: card[2], width: null, height: null };
      return {
        target: t.text,
        is_embed: t.is_embed,
        kind,
        resolved,
        url: null,
        anchor: null,
        // A folder embed follows to its INDEX SOURCE (never the directory) —
        // mirroring what FolderFacts::index_source_for returns in prod.
        //
        // …except for a folder whose index the build SYNTHESIZES (articles, no
        // index source of its own), which resolves to null and is therefore not
        // followable. `unindexed/` is the harness's name for that case, so a
        // gate can put a followable and an unfollowable card on one page — the
        // distinction is invisible in the DOM otherwise, and a mock with only
        // one of the two states cannot prove an affordance is conditional.
        resolved_path: isFolder && !/(^|\/)unindexed\/$/.test(t.text)
          ? '/project/' + t.text.replace(/^\/+|\/+$/g, '') + '/index.md'
          : null,
        message: null,
        candidates: [],
        folder: isFolder
          ? { title: 'Notes', direct_child_count: 12, descendant_count: 30 }
          : null,
      };
    });
    return { status: 'ok' as const, data };
  },
  // Chip-bar asset thumbnails — no preview server in the harness, so report
  // the asset as unresolved (null data). The synchronous call must exist on
  // `commands` or chip-bar's renderChips throws and aborts loadFile before the
  // editor mounts.
  editorResolveAsset: async (_pathStr: string, _filePath: string) => ({
    status: 'ok' as const,
    data: null,
  }),
  // Editor heading state — applyHeadingState() calls this after CM6 mounts.
  // Without it the call rejects with a "not a function" page error (the editor
  // still mounts, but the smoke suite's zero-JS-errors assertion fails).
  // Mirrors moss-core's resolution (crates/moss-core/src/heading/state.rs):
  // a missing `title:` is filename-mode (visible, text from the filename),
  // not a hidden heading — only an explicitly empty `title: ""` hides it.
  // Every MOCK_FILES fixture carries a title, so the filename-mode branch
  // only fires for a caller-supplied `fileOverrides` entry that omits one.
  computeHeadingState: async (
    absPath: string,
    frontmatter: JsonValue,
    _body: string,
  ) => {
    const fm = (frontmatter && typeof frontmatter === 'object' && !Array.isArray(frontmatter))
      ? frontmatter as Record<string, unknown>
      : {};
    if (typeof fm.title === 'string') {
      return {
        status: 'ok' as const,
        data: { visible: fm.title.trim() !== '', text: fm.title, source: 'Title' as const },
      };
    }
    const base = absPath.split('/').pop() ?? absPath;
    const text = base.replace(/\.[^./]+$/, '').replace(/[-_]/g, ' ');
    return {
      status: 'ok' as const,
      data: { visible: true, text, source: 'Filename' as const },
    };
  },
  // Versions mode. `path` does double duty for
  // 'folder'/'site' scope exactly like the real command: the top-level list
  // call passes the folder to filter by ('' for site), and the drill-down
  // (screen three's "Changed since / Unchanged" split) passes a record id
  // instead — distinguished here by checking whether `path` names a known
  // record, standing in for the real backend's argument-shape split
  // documented in versions-surface.ts's file header ("Two backend gaps").
  listVersions: async (scope: string, path: string | null) => {
    const records = allVersionRecords();
    const liveId = records.find((r) => r.live)?.id ?? null;
    const base = { live_id: liveId, has_multiple_targets: false, store_dir: '/mock/history/刘果' };

    if (scope === 'page') {
      const rows = records
        .filter((r) => r.trigger !== 'publish' || r.changed.includes(path ?? ''))
        .map((r) => toVersionRow(r, null));
      return { status: 'ok' as const, data: { ...base, rows, changed_since: [], unchanged: [] } };
    }

    const drillDown = records.find((r) => r.id === path);
    if (drillDown) {
      const changedSet = new Set(drillDown.changed);
      const changed_since = VERSION_FILES.filter((f) => changedSet.has(f)).map((f) => ({ source_path: f, verb: 'edited' as const }));
      const unchanged = VERSION_FILES.filter((f) => !changedSet.has(f));
      return { status: 'ok' as const, data: { ...base, rows: [], changed_since, unchanged } };
    }

    const folder = path || null; // '' (site) becomes null: "no folder filter"
    const rows = records
      .filter((r) => r.trigger !== 'publish' || folder === null || r.changed.some((p) => underFolder(p, folder)))
      .map((r) => toVersionRow(r, folder));
    return { status: 'ok' as const, data: { ...base, rows, changed_since: [], unchanged: [] } };
  },
  readVersion: async (id: string, path: string) => {
    const rec = allVersionRecords().find((r) => r.id === id);
    if (!rec) return { status: 'ok' as const, data: { kept: false, text: null } };
    return { status: 'ok' as const, data: { kept: true, text: versionText(rec, path) } };
  },
  restoreVersion: async (id: string, path: string | null, mode: string) => {
    const rec = allVersionRecords().find((r) => r.id === id);
    const restored = path === null ? VERSION_FILES.slice() : VERSION_FILES.filter((f) => underFolder(f, path));
    // 'copy' writes beside the original, never over it — a real reopen of
    // the SAME file must keep showing the current content, only 'in_place'
    // overwrites what parseFrontmatter returns for it.
    if (rec && mode === 'in_place') {
      for (const f of restored) {
        fileOverrides[`${MOCK_PROJECT_ROOT}/${f}`] = { frontmatter: { title: f.split('/').pop() }, body: versionText(rec, f) };
      }
    }
    return { status: 'ok' as const, data: { restored, failed: [], trashed: [], undo_id: `mock-undo-${id}` } };
  },
  saveVersion: async (label: string | null) => {
    const rec: MockVersionRecord = {
      id: `manual-${Date.now()}`,
      publishedAt: new Date().toISOString(),
      trigger: 'manual',
      label,
      live: false,
      changed: VERSION_FILES.slice(),
    };
    extraVersionRecords = [rec, ...extraVersionRecords];
    return { status: 'ok' as const, data: toVersionRow(rec, null) };
  },
  revealHistoryStore: async (_projectPath: string) => ({ status: 'ok' as const, data: null }),
};

// ── Mock Events ─────────────────────────────────────────────────────────

type EventHandler = (event: { payload: unknown }) => void;
const listeners = new Map<string, EventHandler[]>();

export const mockListen = async (event: string, handler: EventHandler) => {
  if (!listeners.has(event)) listeners.set(event, []);
  listeners.get(event)!.push(handler);
  return () => {
    const fns = listeners.get(event);
    if (fns) {
      const idx = fns.indexOf(handler);
      if (idx >= 0) fns.splice(idx, 1);
    }
  };
};

export const mockEmit = async (event: string, payload: unknown) => {
  for (const fn of listeners.get(event) || []) fn({ payload });
};

export function clearListeners(): void { listeners.clear(); }
