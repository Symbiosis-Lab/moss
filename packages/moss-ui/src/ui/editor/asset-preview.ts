// asset-preview.ts — the ONE asset preview card, and its presentations.
//
// The card (thumbnail + name + path + dimensions + size) is rendered by
// `renderAssetPreview`. Two surfaces show it:
//
//   1. the reference card's asset provider (`assetCardProvider`,
//      cm-reference-card.ts) — data already in hand from the unified
//      reference cache, so it renders synchronously via `buildHoverDom`;
//   2. the CM6 completion info panel beside the highlighted asset row
//      (`assetCompletionInfo` → `createAssetPreviewPanel` — metadata is fetched
//      lazily for the highlighted row only);
//
// Hover and chord arming for references goes through the reference card, reusing
// `buildHoverDom` rather than a second hover surface.
//
// `AssetPreviewData` lives in `@symbiosis-lab/moss-syntax` (completion-core.ts),
// the shared pure DTO layer, beside `ExtKind`/`WikilinkCompletionItem`, and the
// envelope subset is declared below. The generated engine types are
// structurally assignable to both, so callers keep passing `ResolvedAsset` /
// `EditorReferenceResolution` unchanged.

// popup-dismiss: CM hover content, closes on pointer leave; no click-opened popup.
import { sourceAssetUrl } from './source-asset-url';
import type { AssetPreviewData } from '@symbiosis-lab/moss-syntax';

export type { AssetPreviewData };

/** Structural subset of `EditorReferenceResolution` the hover reads. */
export interface AssetReferenceEnvelope {
  kind: { kind: string };
  resolved?: AssetPreviewData | null;
}

/**
 * A mounted card plus its teardown. Matches CM6's `CompletionInfo` object form
 * (`{ dom, destroy? }`) exactly, so a handle can be returned verbatim from a
 * `Completion.info` callback.
 */
export interface AssetPreviewHandle {
  dom: HTMLElement;
  destroy(): void;
}

export interface RenderAssetPreviewOptions {
  /**
   * true  → live media controls for non-image kinds (`<video controls>`,
   *         `<audio controls>`, `<embed>` pdf). The hover popover only: it is a
   *         deliberate, stationary gesture, so playback there is reachable.
   * false → a still, non-interactive format badge for every non-image kind.
   *         Default. A transient panel driven by arrow keys must not sprout
   *         controls the user cannot reach, and WebKit gives no guaranteed
   *         poster frame from preload="metadata" — a black rectangle is a worse
   *         deterministic outcome than a legible badge.
   */
  interactive?: boolean;
  /**
   * Project-root-relative path shown under the name. This is the point of the
   * card for hash-named assets — the folder is the only human-readable identity
   * they have. Null/undefined omits the line.
   */
  relPath?: string | null;
}

/**
 * Delay before a lazily-loaded panel fetches its metadata. Arrow-key auto-repeat
 * is ~30ms and CM6 destroys the previous info pane BEFORE building the next
 * (`updateSel` in the @codemirror/autocomplete implementation), so at 120ms
 * every skipped row's timer is cleared before it fires: no RPC and no <img> is
 * ever created for a row the user scrolled past. Correctness does not depend on
 * the constant — the `disposed` flag makes a late resolve inert regardless.
 */
export const PREVIEW_DELAY_MS = 120;

/**
 * Build the hover popover content from the unified-cache envelope.
 *
 * - `kind: not-found` → empty: the card's problem sentence (cm-problems.ts)
 *   says what is missing and what follows from it.
 * - Missing media metadata → the target alone; absence of metadata does not
 *   establish absence of a file.
 * - otherwise → the shared preview card from `env.resolved`, in `interactive`
 *   mode (video/audio/pdf get real controls; the hover is stationary enough to
 *   use them).
 *
 * Pure over (target, envelope) — exported for tests.
 */
export function buildHoverDom(
  target: string,
  env: AssetReferenceEnvelope | undefined,
): HTMLElement {
  const dom = document.createElement('div');
  dom.className = 'cm-asset-hover';
  const resolved = env?.resolved ?? undefined;
  if (env?.kind.kind === 'not-found') return dom;
  if (!resolved || typeof resolved.mime_type !== 'string') {
    dom.textContent = target;
    return dom;
  }
  dom.appendChild(renderAssetPreview(resolved, { interactive: true }));
  return dom;
}

/**
 * THE preview card. Layout lives in `.asset-preview*` CSS (editor.css), never in
 * inline styles — each container (`.cm-asset-hover`, `.cm-completionInfo`) owns
 * its own chrome and nothing else.
 */
export function renderAssetPreview(
  asset: AssetPreviewData,
  opts: RenderAssetPreviewOptions = {},
): HTMLElement {
  const wrap = document.createElement('div');
  wrap.className = 'asset-preview';
  // Defense-in-depth: callers already reject entries lacking mime_type, but a
  // `{ resolved: false }` placeholder once reached here and crashed the hover on
  // `absolute_path.split()`. An empty wrapper is the safe degradation.
  if (typeof asset?.mime_type !== 'string') return wrap;
  wrap.appendChild(renderThumb(asset, opts.interactive ?? false));
  wrap.appendChild(renderMeta(asset, opts.relPath ?? null));
  return wrap;
}

/** A still tile carrying a short format label (`PNG`, `MP4`, `PDF`). */
function badgeTile(label: string): HTMLElement {
  const el = document.createElement('div');
  el.className = 'asset-preview-thumb asset-preview-thumb--badge';
  el.textContent = label;
  return el;
}

/** `image/png` → `PNG`, `image/svg+xml` → `SVG`, `.mp4` → `MP4`. */
export function formatBadgeLabel(mimeOrExt: string | null | undefined): string {
  if (!mimeOrExt) return '?';
  const sub = mimeOrExt.includes('/') ? mimeOrExt.split('/')[1] : mimeOrExt.replace(/^\./, '');
  return (sub ?? '').split('+')[0].toUpperCase() || '?';
}

function renderThumb(asset: AssetPreviewData, interactive: boolean): HTMLElement {
  const url = sourceAssetUrl(asset.request_url);
  if (asset.mime_type.startsWith('image/')) {
    const img = document.createElement('img');
    img.className = 'asset-preview-thumb';
    img.src = url;
    // A file deleted between the search walk and the load degrades to a legible
    // format badge rather than the browser's broken-image glyph.
    img.addEventListener('error', () => {
      img.replaceWith(badgeTile(formatBadgeLabel(asset.mime_type)));
    });
    return img;
  }
  if (!interactive) return badgeTile(formatBadgeLabel(asset.mime_type));
  if (asset.mime_type.startsWith('video/')) {
    const v = document.createElement('video');
    v.className = 'asset-preview-media';
    v.controls = true;
    v.preload = 'metadata';
    v.src = url;
    return v;
  }
  if (asset.mime_type.startsWith('audio/')) {
    const a = document.createElement('audio');
    a.className = 'asset-preview-media';
    a.controls = true;
    a.preload = 'metadata';
    a.src = url;
    return a;
  }
  if (asset.mime_type === 'application/pdf') {
    const e = document.createElement('embed');
    e.className = 'asset-preview-media';
    e.setAttribute('type', 'application/pdf');
    e.setAttribute('src', url + '#page=1&toolbar=0');
    return e;
  }
  return badgeTile(formatBadgeLabel(asset.mime_type));
}

function renderMeta(asset: AssetPreviewData, relPath: string | null): HTMLElement {
  const wrap = document.createElement('div');
  wrap.className = 'asset-preview-meta';
  wrap.appendChild(
    metaLine('asset-preview-name', asset.absolute_path.split('/').pop() ?? asset.absolute_path),
  );
  if (relPath) wrap.appendChild(metaLine('asset-preview-path', relPath));
  if (asset.width != null && asset.height != null) {
    wrap.appendChild(metaLine('asset-preview-dims', `${asset.width} × ${asset.height}`));
  }
  wrap.appendChild(metaLine('asset-preview-size', formatSize(Number(asset.size_bytes))));
  return wrap;
}

function metaLine(className: string, text: string): HTMLElement {
  const el = document.createElement('div');
  el.className = className;
  el.textContent = text;
  return el;
}

function formatSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

// ── Lazy panel (completion info pane, chip-bar picker) ────────────────

export interface AssetPreviewPanelOptions {
  /** Card title: the candidate's insert value (the text acceptance writes). */
  name: string;
  /** The candidate's extension (`'.png'`), used for the pre-load format badge. */
  ext?: string | null;
  /**
   * Fetch metadata for this candidate. Called at most ONCE, and only after
   * `delayMs` elapses without a destroy(). Resolving null (or rejecting) means
   * "no metadata" — the card keeps the name and the format badge.
   */
  load: () => Promise<AssetPreviewData | null>;
  delayMs?: number;
  interactive?: boolean;
}

/**
 * A card that appears instantly with what is already known (name + format) and
 * fills in the thumbnail, resolved path, dimensions and size once the metadata
 * lands.
 *
 * SYNCHRONOUS return, patched in place — never a `Promise<CompletionInfo>`. CM6's
 * promise branch skips `aria-describedby` (@codemirror/autocomplete 6.20.3
 * implementation) and gives no cancellation handle.
 */
export function createAssetPreviewPanel(o: AssetPreviewPanelOptions): AssetPreviewHandle {
  const dom = document.createElement('div');
  dom.className = 'asset-preview';
  dom.appendChild(badgeTile(formatBadgeLabel(o.ext)));
  const meta = document.createElement('div');
  meta.className = 'asset-preview-meta';
  meta.appendChild(metaLine('asset-preview-name', o.name));
  dom.appendChild(meta);

  let disposed = false;
  const timer = setTimeout(() => {
    void o.load().then(
      (data) => {
        if (disposed || !data) return;
        // `request_url` is `/` + the project-root-relative path
        // (asset_resolver.rs's absolute_to_request_url), so the disambiguating
        // folder arrives free with the metadata.
        const relPath = data.request_url.replace(/^\//, '');
        dom.replaceChildren(
          ...renderAssetPreview(data, { interactive: o.interactive, relPath }).childNodes,
        );
      },
      // A failed resolve leaves the name + format badge standing. There is
      // nothing truthful to say beyond that: the candidate came from the file
      // walk, so "not found" would be wrong.
      () => {},
    );
  }, o.delayMs ?? PREVIEW_DELAY_MS);

  return {
    dom,
    destroy() {
      disposed = true;
      clearTimeout(timer);
    },
  };
}

/**
 * The `Completion.info` builder for one dropdown row: a preview card for an
 * asset, nothing for any other kind. The card resolves the row's `insert` —
 * exactly the text acceptance writes, from the active document — so the panel
 * previews the INSERTION, not "a file": two candidates that insert the same
 * text are the same completion, and showing them the same preview is the
 * truth. Absent resolver ⇒ no panel.
 */
export function assetCompletionInfo(
  item: { insert: string; kind: string },
  resolve: ((target: string) => Promise<AssetPreviewData | null>) | undefined,
): (() => AssetPreviewHandle) | undefined {
  if (item.kind !== 'asset' || !resolve) return undefined;
  const target = item.insert;
  const dot = target.lastIndexOf('.');
  const ext = dot > 0 ? target.slice(dot) : null;
  return () => {
    try {
      return createAssetPreviewPanel({
        name: target,
        ext,
        load: () => resolve(target),
      });
    } catch {
      const dom = document.createElement('div');
      dom.className = 'asset-preview';
      dom.appendChild(metaLine('asset-preview-name', target));
      return { dom, destroy() {} };
    }
  };
}
