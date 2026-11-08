// Shared file type detection for the editor.
// Used by cm-editor.ts (language extension selection) and editor-main.ts (feature gating).
//
// ── Embeddable asset truth ────────────────────────────────────────────────────
// Which extensions are embeddable (Image/Video/Audio/Pdf/Notebook/…) is the
// SOLE responsibility of `asset-registry.generated.ts` (generated from
// `crates/moss-core/src/resolve/asset_registry.rs`).  `detectFileCategory`
// routes image/video/audio detection through `lookupAsset` so the two sources
// never diverge.
//
// The sets below (TEXT_EXTS, FONT_EXTS, TEXT_NAMES) are file-viewer concerns
// only — they control whether the CM6 text editor or a native viewer opens the
// file.  They are intentionally NOT in the asset registry (no embed template,
// no MIME needed there).

import { lookupAsset } from './asset-registry.generated';

export type EditorFileType = 'markdown' | 'html' | 'javascript' | 'css' | 'json' | 'toml' | 'plain';

export function detectFileType(filePath: string): EditorFileType {
  const ext = filePath.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'md': case 'mdx': return 'markdown';
    case 'html': case 'htm': return 'html';
    case 'js': case 'mjs': case 'jsx': case 'ts': case 'tsx': return 'javascript';
    case 'css': return 'css';
    case 'json': return 'json';
    case 'toml': return 'toml';
    default: return 'plain';
  }
}

export function isMarkdownFile(filePath: string): boolean {
  return detectFileType(filePath) === 'markdown';
}

// ── File Category (routing: text editor vs. native viewer) ────────────

export type FileCategory = 'text' | 'image' | 'video' | 'audio' | 'pdf' | 'font' | 'binary';

// Font and text sets are viewer concerns only — they are NOT in the asset registry
// (no embed template / MIME needed).  See the file header comment above.
const FONT_EXTS = new Set(['ttf', 'otf', 'woff', 'woff2']);
const TEXT_EXTS = new Set([
  'md', 'mdx', 'html', 'htm', 'js', 'mjs', 'jsx', 'ts', 'tsx', 'css', 'json',
  'txt', 'xml', 'yaml', 'yml', 'toml', 'csv', 'log', 'sh', 'py', 'rb', 'rs',
  'go', 'swift', 'kt', 'java', 'c', 'cpp', 'h', 'hpp', 'sql', 'graphql',
  'env', 'gitignore', 'dockerignore', 'dockerfile', 'makefile', 'lock',
]);

// Well-known extensionless filenames that should open as text
const TEXT_NAMES = new Set([
  'readme', 'license', 'licence', 'changelog', 'authors', 'contributors',
  'copying', 'notice', 'todo', 'makefile', 'dockerfile', 'gemfile',
  'rakefile', 'procfile', 'brewfile', 'vagrantfile', 'caddyfile',
]);

export function detectFileCategory(filePath: string): FileCategory {
  const filename = filePath.split('/').pop()?.toLowerCase() ?? '';
  const ext = filename.includes('.') ? filename.split('.').pop()! : '';

  if (ext) {
    if (TEXT_EXTS.has(ext)) return 'text';
    if (FONT_EXTS.has(ext)) return 'font';

    // Embeddable-asset categories derive from the registry (single source of truth).
    // This ensures image/video/audio detection never diverges from the Rust registry.
    const registryEntry = lookupAsset(ext);
    if (registryEntry) {
      switch (registryEntry.kind) {
        case 'Image': return 'image';
        case 'Video': return 'video';
        case 'Audio': return 'audio';
        case 'Pdf':   return 'pdf';
        // Other registry kinds (Notebook, Iframe, Model, Table, Transclusion) are
        // handled as 'binary' here — the file viewer doesn't have dedicated views
        // for them; they open via embed in the editor.
      }
    }

    return 'binary';
  }

  // No extension — check by filename
  if (TEXT_NAMES.has(filename)) return 'text';
  return 'binary';
}
