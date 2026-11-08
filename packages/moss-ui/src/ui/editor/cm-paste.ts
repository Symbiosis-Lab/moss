// Paste verbs for the markdown editor:
//
//   - PASTE-URL-AS-LINK: a bare URL pasted over a non-empty selection turns
//     the selection into `[selection](url)` (iA Writer / Obsidian convention).
//   - PASTE-IMAGE: clipboard image files (the screenshot workflow) are saved
//     into the project via the injected `saveImage` callback and inserted as
//     `![](path)` at the cursor, the path written from the page (same as drop),
//     or into the empty `()` of an image the caret is in.
//
// Everything else falls through to CM6's default paste. The logic lives in
// the exported pure `handlePaste(view, clipboard, opts)` so it is testable
// without constructing real ClipboardEvents (jsdom can't).

import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { isEmbedNode, type TreeNode } from '@symbiosis-lab/moss-syntax/cm6';
import { embedFor, type DestinationFor } from './cm-drop';

/** The slice of DataTransfer the handler needs — injectable for tests. */
export interface ClipboardLike {
  files: ArrayLike<File>;
  getData(format: string): string;
}

export interface PasteOptions {
  /**
   * Persist pasted image bytes into the project (where the attachment-folder
   * setting says). Receives a suggested filename and the base64-encoded
   * bytes; returns the stored file's project-relative path, or null on
   * failure (nothing is inserted).
   */
  saveImage?: (suggestedName: string, dataBase64: string) => Promise<string | null>;
  /** The stored file's destination from the page being edited (cm-drop.ts). */
  destinationFor?: DestinationFor;
}

/** A bare absolute URL and nothing else — the linkify trigger. */
const BARE_URL = /^https?:\/\/\S+$/;

const MIME_EXT: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/svg+xml': 'svg',
  'image/avif': 'avif',
  'image/heic': 'heic',
  'image/bmp': 'bmp',
  'image/tiff': 'tiff',
};

function timestampName(ext: string): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `pasted-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.${ext}`;
}

/** Base64-encode without blowing the call stack on large images. */
function toBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let binary = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** Blob.arrayBuffer with a FileReader fallback (jsdom lacks the former). */
function readBytes(file: File): Promise<ArrayBuffer> {
  if (typeof file.arrayBuffer === 'function') return file.arrayBuffer();
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result as ArrayBuffer);
    r.onerror = () => reject(r.error);
    r.readAsArrayBuffer(file);
  });
}

/**
 * The embed or wikilink straddling the caret — `![](…)`, `[[…]]` or
 * `![[…]]` — or null, with what a paste may do to it. Walking up from the
 * syntax node at the caret mirrors `cm-link-nav.ts`'s established pattern;
 * any node `resolveInner` returns for a position strictly inside `Image` /
 * `WikilinkEmbed` / `Wikilink` has that node somewhere in its ancestor chain,
 * including the zero-width target of a freshly-opened, still-empty pair.
 *
 * `emptyDest` is the `()` of an image with no destination yet — what the
 * `/img` slash command leaves (`cm-shortcode-catalog.ts`) — which a paste
 * fills, keeping any alt text. `empty` is a wikilink with no target, as typing
 * `![[` auto-closes to (`cm-wikilink-complete.ts`), which a paste replaces.
 * Anything else holds the author's text, so a paste goes after it.
 */
function embedAtCaret(view: EditorView): {
  from: number;
  to: number;
  empty: boolean;
  emptyDest: { from: number; to: number } | null;
} | null {
  const pos = view.state.selection.main.head;
  let node: TreeNode | null = syntaxTree(view.state).resolveInner(pos, 1);
  while (node && node.name !== 'Wikilink' && node.name !== 'Image' && !isEmbedNode(node.name)) node = node.parent;
  // Strictly inside (matching `cm-link-nav.ts`'s own `inside()` convention):
  // `resolveInner(pos, 1)` at a position exactly BEFORE an existing
  // `![[x]]` also descends into that node (side 1 favors what follows), so
  // the node check alone would treat "about to type right before an embed"
  // as "inside" it. Only a span that actually straddles `pos` counts.
  if (!node || pos <= node.from || pos >= node.to) return null;
  const text = view.state.doc.sliceString(node.from, node.to);
  if (node.name === 'Image') {
    const parens = node.getChild('URL') ? null : /\(\s*\)$/.exec(text);
    const emptyDest = parens ? { from: node.from + parens.index, to: node.to } : null;
    return { from: node.from, to: node.to, empty: false, emptyDest };
  }
  return { from: node.from, to: node.to, empty: /^!?\[\[\s*\]\]$/.test(text), emptyDest: null };
}

async function pasteImages(
  view: EditorView,
  images: File[],
  saveImage: NonNullable<PasteOptions['saveImage']>,
  destinationFor: DestinationFor = async () => null,
): Promise<void> {
  const stored: string[] = [];
  for (const file of images) {
    const ext = MIME_EXT[file.type] ?? 'png';
    const base64 = toBase64(await readBytes(file));
    const path = await saveImage(timestampName(ext), base64);
    if (path) stored.push(path);
  }
  if (stored.length === 0) return;

  // Never nest an embed inside the one at the caret (`![[![[name]]\n]]` used
  // to reach the build as a corrupted reference), and never overwrite one
  // that holds the author's text: only an empty one is filled.
  const at = embedAtCaret(view);
  if (at?.emptyDest) {
    const dest = await destinationFor(stored[0]);
    if (dest) {
      const rest = await Promise.all(stored.slice(1).map((t) => embedFor(t, destinationFor)));
      const insert = `(${dest})` + rest.map((e) => `\n${e}`).join('');
      view.dispatch({ changes: { ...at.emptyDest, insert }, userEvent: 'input.paste' });
      return;
    }
  }
  const embeds = (await Promise.all(stored.map((t) => embedFor(t, destinationFor)))).join('\n');
  view.dispatch({
    changes: at?.empty
      ? { from: at.from, to: at.to, insert: embeds }
      : at
        ? { from: at.to, insert: `\n${embeds}` }
        : { from: view.state.selection.main.head, insert: embeds + '\n' },
    userEvent: 'input.paste',
  });
}

/**
 * Returns true when the paste was handled here (caller must preventDefault);
 * false to fall through to CM6's default paste.
 */
export function handlePaste(
  view: EditorView,
  clipboard: ClipboardLike,
  opts: PasteOptions,
): boolean {
  // Screenshot / image-file paste.
  const images = Array.from(clipboard.files ?? []).filter((f) => f.type.startsWith('image/'));
  if (images.length > 0) {
    if (!opts.saveImage) return false;
    void pasteImages(view, images, opts.saveImage, opts.destinationFor);
    return true;
  }

  // Bare URL over a selection → linkify.
  const sel = view.state.selection.main;
  if (sel.from === sel.to) return false;
  const text = clipboard.getData('text/plain').trim();
  if (!BARE_URL.test(text)) return false;

  const selected = view.state.doc.sliceString(sel.from, sel.to);
  const insert = `[${selected}](${text})`;
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert },
    selection: { anchor: sel.from + insert.length },
    userEvent: 'input.paste',
  });
  return true;
}

export function pasteExtension(opts: PasteOptions = {}): Extension {
  return EditorView.domEventHandlers({
    paste(event, view) {
      if (!event.clipboardData) return false;
      const handled = handlePaste(view, event.clipboardData, opts);
      if (handled) event.preventDefault();
      return handled;
    },
  });
}
