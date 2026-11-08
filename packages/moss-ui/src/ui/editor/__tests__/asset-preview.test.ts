import { describe, test, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { EditorReferenceResolution, ReferenceKind } from '@symbiosis-lab/moss-ui';
import {
  buildHoverDom,
  renderAssetPreview,
  createAssetPreviewPanel,
  assetCompletionInfo,
  type AssetPreviewData,
} from '../asset-preview';
import type { ResolvedAsset } from '@symbiosis-lab/moss-ui';

/** Build a unified-cache envelope (embed context) with the given kind/asset. */
function envelope(
  kind: ReferenceKind,
  resolved: ResolvedAsset | null,
): EditorReferenceResolution {
  return {
    target: 'x',
    is_embed: true,
    kind,
    resolved,
    url: null,
    anchor: null,
    resolved_path: null,
    message: null,
    candidates: [],
    folder: null,
  };
}

const IMAGE_ASSET: ResolvedAsset = {
  absolute_path: '/site/images/photo.png',
  request_url: '/images/photo.png',
  mime_type: 'image/png',
  width: 800,
  height: 600,
  size_bytes: 204800,
  provenance: 'literal' as any,
  rung_widths: [],
  offers_original: false,
};

// The card renderer is tested HERE, once, at the level it lives. The
// `buildHoverDom` describe below covers only its own branching (not-found vs
// card); the four render-level assertions it used to duplicate were deleted
// with the extraction.
describe('renderAssetPreview (the one card, three containers)', () => {
  test('image → thumbnail via moss-source://, name, dimensions, size', () => {
    const el = renderAssetPreview(IMAGE_ASSET);
    const img = el.querySelector('img') as HTMLImageElement;
    expect(img).not.toBeNull();
    expect(img.src).toBe('moss-source://localhost/images/photo.png');
    expect(el.querySelector('.asset-preview-name')!.textContent).toBe('photo.png');
    expect(el.querySelector('.asset-preview-dims')!.textContent).toBe('800 × 600');
    expect(el.querySelector('.asset-preview-size')!.textContent).toBe('200.0 KB');
  });

  test('relPath renders the disambiguating folder line; absent by default', () => {
    const withPath = renderAssetPreview(IMAGE_ASSET, { relPath: '關於/photo.png' });
    expect(withPath.querySelector('.asset-preview-path')!.textContent).toBe('關於/photo.png');
    expect(renderAssetPreview(IMAGE_ASSET).querySelector('.asset-preview-path')).toBeNull();
  });

  test('non-interactive video → format badge, never a <video> with unreachable controls', () => {
    const el = renderAssetPreview({ ...IMAGE_ASSET, mime_type: 'video/mp4' });
    expect(el.querySelector('video')).toBeNull();
    expect(el.querySelector('.asset-preview-thumb--badge')!.textContent).toBe('MP4');
  });

  test('interactive (hover) keeps live media controls for video/audio/pdf', () => {
    const v = renderAssetPreview({ ...IMAGE_ASSET, mime_type: 'video/mp4' }, { interactive: true });
    expect((v.querySelector('video') as HTMLVideoElement).controls).toBe(true);
    const a = renderAssetPreview({ ...IMAGE_ASSET, mime_type: 'audio/mpeg' }, { interactive: true });
    expect(a.querySelector('audio')).not.toBeNull();
    const p = renderAssetPreview(
      { ...IMAGE_ASSET, mime_type: 'application/pdf' },
      { interactive: true },
    );
    expect(p.querySelector('embed')).not.toBeNull();
  });

  test('svg+xml badge drops the +suffix', () => {
    const el = renderAssetPreview({ ...IMAGE_ASSET, mime_type: 'application/svg+xml' });
    expect(el.querySelector('.asset-preview-thumb--badge')!.textContent).toBe('SVG');
  });

  // Crash guards: `{ resolved: false }` is the shape that once crashed the
  // hover on renderMeta's `absolute_path.split('/')`.
  test('mimeless / placeholder inputs degrade to an empty card instead of throwing', () => {
    expect(() => renderAssetPreview({ resolved: false } as any)).not.toThrow();
    expect(() => renderAssetPreview({} as any)).not.toThrow();
  });
});

describe('buildHoverDom (unified-cache data source)', () => {
  test('not-found kind → an empty body: the card title names the file and its problem sentence says what follows', () => {
    const dom = buildHoverDom('missing.png', envelope({ kind: 'not-found' }, null));
    expect(dom.textContent).toBe('');
    expect(dom.querySelector('img')).toBeNull();
    expect(dom.querySelector('.asset-preview-meta')).toBeNull();
  });

  test('resolved image kind → the card, from env.resolved', () => {
    const dom = buildHoverDom('photo.png', envelope({ kind: 'image' }, IMAGE_ASSET));
    expect(dom.querySelector('img')).not.toBeNull();
    expect(dom.querySelector('.asset-preview-name')!.textContent).toBe('photo.png');
  });

  test('not-yet-resolved metadata does not claim a missing file', () => {
    const dom = buildHoverDom('pending.png', undefined);
    expect(dom.textContent).toBe('pending.png');
  });
});

describe('createAssetPreviewPanel (lazy, one image in flight)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const data = IMAGE_ASSET as AssetPreviewData;

  it('returns synchronously with the name + format badge, and loads nothing yet', () => {
    const load = vi.fn().mockResolvedValue(data);
    const h = createAssetPreviewPanel({ name: 'photo.png', ext: '.png', load });
    expect(h.dom.querySelector('.asset-preview-name')!.textContent).toBe('photo.png');
    expect(h.dom.querySelector('.asset-preview-thumb--badge')!.textContent).toBe('PNG');
    expect(h.dom.querySelector('img')).toBeNull();
    expect(load).not.toHaveBeenCalled();
    h.destroy();
  });

  it('loads exactly once after the delay, then fills in thumbnail + resolved path', async () => {
    const load = vi.fn().mockResolvedValue(data);
    const h = createAssetPreviewPanel({ name: 'photo.png', ext: '.png', load, delayMs: 120 });
    await vi.advanceTimersByTimeAsync(119);
    expect(load).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(50);
    expect(load).toHaveBeenCalledTimes(1);
    expect(h.dom.querySelector('img')).not.toBeNull();
    // request_url is '/' + the root-relative path, so the folder comes free.
    expect(h.dom.querySelector('.asset-preview-path')!.textContent).toBe('images/photo.png');
    expect(h.dom.querySelector('.asset-preview-size')!.textContent).toBe('200.0 KB');
    h.destroy();
  });

  it('destroy() before the delay → never loads (this is what makes fast arrow-scroll free)', async () => {
    const load = vi.fn().mockResolvedValue(data);
    const h = createAssetPreviewPanel({ name: 'photo.png', load });
    h.destroy();
    await vi.advanceTimersByTimeAsync(1000);
    expect(load).not.toHaveBeenCalled();
  });

  it('destroy() while the load is in flight → the late resolve writes nothing', async () => {
    let settle: (v: AssetPreviewData) => void = () => {};
    const load = vi.fn(() => new Promise<AssetPreviewData>((r) => { settle = r; }));
    const h = createAssetPreviewPanel({ name: 'photo.png', load });
    await vi.advanceTimersByTimeAsync(200);
    expect(load).toHaveBeenCalledTimes(1);
    h.destroy();
    settle(data);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.dom.querySelector('img')).toBeNull();
  });

  it('null metadata and a rejected load both keep the name + badge and never throw', async () => {
    const nullPanel = createAssetPreviewPanel({ name: 'a.png', ext: '.png', load: async () => null });
    const rejectPanel = createAssetPreviewPanel({
      name: 'b.png',
      ext: '.png',
      load: () => Promise.reject(new Error('resolver declined')),
    });
    await vi.advanceTimersByTimeAsync(200);
    for (const p of [nullPanel, rejectPanel]) {
      expect(p.dom.querySelector('.asset-preview-name')).not.toBeNull();
      expect(p.dom.querySelector('.asset-preview-thumb--badge')!.textContent).toBe('PNG');
      expect(p.dom.textContent).not.toContain('not found');
      p.destroy();
    }
  });

  it('destroy() is idempotent', () => {
    const h = createAssetPreviewPanel({ name: 'a.png', load: async () => null });
    h.destroy();
    expect(() => h.destroy()).not.toThrow();
  });
});

describe('assetCompletionInfo (the Completion.info seam)', () => {
  it('is undefined for a page candidate, and for an asset when no resolver is wired', () => {
    const resolve = vi.fn(async () => IMAGE_ASSET as AssetPreviewData);
    expect(assetCompletionInfo({ insert: 'Notes', kind: 'page' }, resolve)).toBeUndefined();
    expect(assetCompletionInfo({ insert: 'a.png', kind: 'asset' }, undefined)).toBeUndefined();
  });

  it('resolves an asset by its INSERT value — the text apply() writes — and badges its extension', async () => {
    vi.useFakeTimers();
    const resolve = vi.fn(async () => IMAGE_ASSET as AssetPreviewData);
    const info = assetCompletionInfo({ insert: '/images/photo.png', kind: 'asset' }, resolve)!;
    const handle = info();
    expect(handle.dom.querySelector('.asset-preview-name')!.textContent).toBe('/images/photo.png');
    expect(handle.dom.textContent).toContain('PNG');
    expect(resolve).not.toHaveBeenCalled(); // lazy, not eager
    await vi.advanceTimersByTimeAsync(200);
    expect(resolve).toHaveBeenCalledWith('/images/photo.png');
    handle.destroy();
    vi.useRealTimers();
  });
});
