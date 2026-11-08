import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ChipBar, assetObjectFit } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';
import { engine } from '../../engine';

// Mock the engine so editorResolveAsset returns deterministic values
vi.mock('../../engine', () => {
  const editorResolveAsset = vi.fn();
  return {
    engine: { editorResolveAsset },
  };
});

// ── ResizeObserver mock (jsdom doesn't have it) ──────────────────────────
class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(_cb: () => void) {}
}
vi.stubGlobal('ResizeObserver', MockResizeObserver);

// ── Minimal schema with a FilePicker cover field ─────────────────────────
//
// ContentSchema shape:
//   { generator, version, frontmatter: { fields: Record<string, FieldDefinition>, internal_fields?: string[] }, shortcodes }
//
// FieldDefinition widget uses the hyphenated form 'file-picker' (matches
// renderChipValue's `widget === 'file-picker'` branch).
function makeFilePickerSchema(): ContentSchema {
  return {
    generator: 'moss',
    version: '1.0',
    frontmatter: {
      fields: {
        cover: {
          type: 'string',
          widget: 'file-picker',
          description: 'Cover image path',
          priority: 60,
          group: 'Occasional',
        } as any,
      },
      internal_fields: [],
    },
    shortcodes: null,
  } as ContentSchema;
}

describe('ChipBar asset widget', () => {
  let container: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
    vi.mocked(engine.editorResolveAsset).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  it('shows thumbnail for resolved cover via moss-source://', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/hero.jpg',
        request_url: '/hero.jpg',
        mime_type: 'image/jpeg',
        size_bytes: '1000' as any,
        width: 800,
        height: 600,
        lqip_data_uri: 'data:image/jpeg;base64,xx',
      } as any,
    } as any);

    const schema = makeFilePickerSchema();
    const store = new FrontmatterStore({
      initial: { cover: './hero.jpg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);

    // Allow async resolve to complete
    await vi.advanceTimersByTimeAsync(50);

    const widget = container.querySelector('.chip-bar-asset');
    expect(widget).not.toBeNull();

    // Should NOT have is-unresolved (asset resolves)
    expect(widget!.classList.contains('is-unresolved')).toBe(false);

    // Should have an img.asset-thumb with moss-source:// src
    const img = widget!.querySelector('img.asset-thumb') as HTMLImageElement | null;
    expect(img).not.toBeNull();
    expect(img!.src).toBe('moss-source://localhost/hero.jpg');

    bar.destroy();
  });

  it('shows is-unresolved styling for missing cover', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: null,
    } as any);

    const schema = makeFilePickerSchema();
    const store = new FrontmatterStore({
      initial: { cover: './missing.jpg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);

    await vi.advanceTimersByTimeAsync(50);

    const widget = container.querySelector('.chip-bar-asset.is-unresolved');
    expect(widget).not.toBeNull();

    bar.destroy();
  });

  // No preview server exists in this harness, which is the point:
  // moss-source:// needs no server.
  it('always uses moss-source:// for the thumbnail URL', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/hero.jpg',
        request_url: '/hero.jpg',
        mime_type: 'image/jpeg',
        size_bytes: '1000' as any,
        width: 800,
        height: 600,
        lqip_data_uri: null,
      } as any,
    } as any);

    const schema = makeFilePickerSchema();
    const store = new FrontmatterStore({
      initial: { cover: './hero.jpg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);

    await vi.advanceTimersByTimeAsync(50);

    const img = container.querySelector('img.asset-thumb') as HTMLImageElement | null;
    expect(img).not.toBeNull();
    expect(img!.src).toBe('moss-source://localhost/hero.jpg');

    bar.destroy();
  });

  it('resolves an Obsidian wikilink cover by the unwrapped path (not the literal [[ ]])', async () => {
    // Regression: `cover: '[[DSCF4053.jpeg]]'` was passed verbatim to the
    // resolver, which then searched for a file literally named
    // `[[DSCF4053.jpeg]]` → NotFound → red chip, even though the build resolves
    // it. The chip must unwrap the wikilink and resolve the bare path.
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/图片/摄影/DSCF4053.jpeg',
        request_url: '/%E5%9B%BE%E7%89%87/DSCF4053.jpeg',
        mime_type: 'image/jpeg', size_bytes: '8000' as any,
        width: 4000, height: 3000, lqip_data_uri: null,
      } as any,
    } as any);

    const store = new FrontmatterStore({
      initial: { cover: '[[DSCF4053.jpeg]]' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(makeFilePickerSchema(), store, { getActivePath: () => '/x/post.md' });
    bar.render(container);
    await vi.advanceTimersByTimeAsync(50);

    // Resolver was called with the UNWRAPPED path, not the bracketed literal.
    expect(vi.mocked(engine.editorResolveAsset)).toHaveBeenCalledWith('DSCF4053.jpeg', '/x/post.md');
    // Not flagged broken.
    const widget = container.querySelector('.chip-bar-asset');
    expect(widget!.classList.contains('is-unresolved')).toBe(false);
    // Display shows the bare path, not the [[ ]] wrapper.
    const path = container.querySelector('.chip-bar-asset .asset-path') as HTMLElement;
    expect(path.textContent).toBe('DSCF4053.jpeg');

    bar.destroy();
  });

  it('does not render asset widget when field value is empty', () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({ status: 'ok', data: null } as any);

    const schema = makeFilePickerSchema();
    const store = new FrontmatterStore({
      initial: { cover: '' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);

    // Empty string: chip is rendered (value is '') but no .chip-bar-asset widget
    const widget = container.querySelector('.chip-bar-asset');
    expect(widget).toBeNull();

    bar.destroy();
  });

  it('SVG logo: object-fit contain, loads via moss-source:// (not opaque LQIP), path verbatim', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/assets/logo.svg', request_url: '/assets/logo.svg',
        mime_type: 'image/svg+xml', size_bytes: '500' as any,
        width: 64, height: 64, lqip_data_uri: 'data:image/jpeg;base64,xx',
      } as any,
    } as any);

    const store = new FrontmatterStore({
      initial: { cover: 'assets/logo.svg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(makeFilePickerSchema(), store,
      { getActivePath: () => '/x/post.md' });
    bar.render(container);
    await vi.advanceTimersByTimeAsync(50);

    const img = container.querySelector('img.asset-thumb') as HTMLImageElement;
    expect(img.style.objectFit).toBe('contain');
    // Real source via moss-source://, NOT the opaque LQIP — preserves transparency.
    expect(img.src).toBe('moss-source://localhost/assets/logo.svg');
    // Path shown exactly as stored — no basename trimming.
    const path = container.querySelector('.chip-bar-asset .asset-path') as HTMLElement;
    expect(path.textContent).toBe('assets/logo.svg');

    bar.destroy();
  });

  it('JPEG cover: object-fit cover, uses moss-source:// directly', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/hero.jpg', request_url: '/hero.jpg',
        mime_type: 'image/jpeg', size_bytes: '1000' as any,
        width: 800, height: 600, lqip_data_uri: 'data:image/jpeg;base64,yy',
      } as any,
    } as any);

    const store = new FrontmatterStore({
      initial: { cover: './hero.jpg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(makeFilePickerSchema(), store,
      { getActivePath: () => '/x/post.md' });
    bar.render(container);
    await vi.advanceTimersByTimeAsync(50);

    const img = container.querySelector('img.asset-thumb') as HTMLImageElement;
    expect(img.style.objectFit).toBe('cover');
    expect(img.src).toBe('moss-source://localhost/hero.jpg');

    bar.destroy();
  });

  it('does not re-RPC when re-render happens for the same value', async () => {
    vi.mocked(engine.editorResolveAsset).mockResolvedValue({
      status: 'ok',
      data: {
        absolute_path: '/x/h.jpg',
        request_url: '/h.jpg',
        mime_type: 'image/jpeg',
        size_bytes: '1' as any,
        width: null,
        height: null,
        lqip_data_uri: null,
      } as any,
    } as any);

    const schema = makeFilePickerSchema();
    const store = new FrontmatterStore({
      initial: { cover: './hero.jpg' },
      requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const bar = new ChipBar(schema, store, { getActivePath: () => '/x/post.md' });
    bar.render(container);

    await vi.advanceTimersByTimeAsync(50);
    expect(vi.mocked(engine.editorResolveAsset)).toHaveBeenCalledTimes(1);

    // Notify subscribers via an unrelated field change — triggers renderChips
    store.setField('description', 'whatever');
    await vi.advanceTimersByTimeAsync(50);

    // RPC should NOT fire again for the same cover value (cache hit)
    expect(vi.mocked(engine.editorResolveAsset)).toHaveBeenCalledTimes(1);

    bar.destroy();
    container.remove();
  });
});

describe('assetObjectFit', () => {
  it('returns cover for photographic formats', () => {
    expect(assetObjectFit('image/jpeg')).toBe('cover');
    expect(assetObjectFit('image/webp')).toBe('cover');
    expect(assetObjectFit('image/avif')).toBe('cover');
  });

  it('returns contain for vector / alpha-capable formats', () => {
    expect(assetObjectFit('image/svg+xml')).toBe('contain');
    expect(assetObjectFit('image/png')).toBe('contain');
    expect(assetObjectFit('image/gif')).toBe('contain');
  });

  it('returns contain for unknown / missing mime (never crop by default)', () => {
    expect(assetObjectFit('application/octet-stream')).toBe('contain');
    expect(assetObjectFit('')).toBe('contain');
    expect(assetObjectFit(null)).toBe('contain');
    expect(assetObjectFit(undefined)).toBe('contain');
  });
});
