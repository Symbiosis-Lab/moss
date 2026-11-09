import { afterEach, describe, expect, test, vi } from 'vitest';

const handlers = new Map<string, (payload: unknown) => void>();

import { bindPreviewInstruments, pointOfCssPosition, type PreviewInstruments } from '../preview-instruments';
import { host } from "../../host";

// The bus the instruments talk over, held so a test can play the editor's side.
host.bus = {
  emit: vi.fn(),
  on: <T,>(name: string, handler: (payload: T) => void) => {
    handlers.set(name, handler as (payload: unknown) => void);
    return () => { handlers.delete(name); };
  },
};
import { HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS, HERO_WORDS } from '../../editor/session/instrument-events';

/** A previewed page with one picture whose inline crop is `style`; `setStyle` restyles it the way the bridge does. */
function page(style: string) {
  const picture = { style };
  const restyles: string[] = [];
  const callBridge = vi.fn(async (method: string, _selector: string, prop?: string, value?: string) => {
    if (method === 'document.querySelector') return { attributes: { style: picture.style } };
    if (method === 'element.setStyle' && prop === 'object-position') {
      restyles.push(value ?? '');
      picture.style = value ? `object-position:${value}` : '';
      return true;
    }
    return null;
  });
  return { picture, restyles, nav: { callBridge } };
}

const settle = () => new Promise((r) => setTimeout(r, 0));
let bound: PreviewInstruments | null = null;
afterEach(() => { bound?.destroy(); bound = null; handlers.clear(); });

describe('a crop the editor wrote', () => {
  const focus = async (payload: unknown) => { handlers.get(HERO_IMAGE_FOCUS)!(payload); await settle(); await settle(); };
  /** A rebuilt page lands: the morph puts the server's inline style back, then the shell is told. */
  const morph = async (p: ReturnType<typeof page>, serverStyle: string) => { p.picture.style = serverStyle; bound!.refresh(); await settle(); await settle(); };

  test('is put back over a rebuilt page that still shows the old crop, and let go once a page shows it', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => true);
    await focus({ path: 'p.md', line: 1, position: '0% 0%', held: true });
    expect(p.restyles).toEqual(['0% 0%']);

    await morph(p, 'object-position:50% 50%'); // an earlier build
    expect(p.restyles).toEqual(['0% 0%', '0% 0%']);

    await morph(p, 'object-position:top left'); // the build of the write: moss writes a named corner as keywords
    await morph(p, 'object-position:top left');
    expect(p.restyles).toEqual(['0% 0%', '0% 0%']);
  });

  test('a live move is not held: a rebuilt page replaces it', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => true);
    await focus({ path: 'p.md', line: 1, position: '20% 30%' });
    await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['20% 30%']);
  });

  test('putting it back restores the crop the page had, not no crop at all', async () => {
    const p = page('object-position:10% 90%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => true);
    await focus({ path: 'p.md', line: 1, position: '20% 30%' });
    await focus({ path: 'p.md', line: 1, position: '25% 35%' });
    await focus({ path: 'p.md', line: 1, position: null });
    expect(p.restyles).toEqual(['20% 30%', '25% 35%', '10% 90%']);
  });

  test('a null after a held write (an undo) lets the hold go: the next rebuilt page is not overridden', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => true);
    await focus({ path: 'p.md', line: 1, position: '0% 0%', held: true });
    await focus({ path: 'p.md', line: 1, position: null });
    expect(p.restyles).toEqual(['0% 0%', '50% 50%']);
    await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['0% 0%', '50% 50%']);
  });

  test('is let go after three rebuilt pages that never show it, so a failed write cannot pin the preview', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => true);
    await focus({ path: 'p.md', line: 1, position: '0% 0%', held: true });
    for (let i = 0; i < 5; i++) await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['0% 0%', '0% 0%', '0% 0%']);
  });
});

describe('with preview features off, the preview is the published page', () => {
  const rect = { left: 0, top: 0, width: 400, height: 200 };
  /** A previewed page with a hero on line 3 and a picture; records every bridge call. */
  function previewed() {
    const calls: string[] = [];
    const callBridge = vi.fn(async (method: string) => {
      calls.push(method);
      if (method === 'document.querySelector') return { boundingRect: rect, attributes: { style: '' } };
      return true;
    });
    return { calls, nav: { callBridge }, iframe: document.createElement('iframe') };
  }
  const emit = async (name: string, payload: unknown) => { handlers.get(name)!(payload); await settle(); await settle(); };
  afterEach(() => { document.querySelectorAll('.moss-instrument-layer').forEach((n) => n.remove()); vi.mocked(host.bus.emit).mockClear(); });

  test.each([[true, 1], [false, 0]])('preview features %s: the words box is drawn %i times', async (on, layers) => {
    const p = previewed();
    bound = bindPreviewInstruments(p.nav as never, p.iframe, () => 'p.md', () => on);
    await emit(HERO_WORDS, { path: 'p.md', line: 3 });
    expect(document.querySelectorAll('.moss-instrument-layer')).toHaveLength(layers);
  });

  test.each([[true, 1], [false, 0]])('preview features %s: the editor\'s live crop restyles the page %i times', async (on, restyles) => {
    const p = previewed();
    bound = bindPreviewInstruments(p.nav as never, p.iframe, () => 'p.md', () => on);
    await emit(HERO_IMAGE_FOCUS, { path: 'p.md', line: 3, position: '20% 30%' });
    expect(p.calls.filter((m) => m === 'element.setStyle')).toHaveLength(restyles);
  });

  test('turning them off clears a drawn words box at the next refresh', async () => {
    const p = previewed();
    let on = true;
    bound = bindPreviewInstruments(p.nav as never, p.iframe, () => 'p.md', () => on);
    await emit(HERO_WORDS, { path: 'p.md', line: 3 });
    expect(document.querySelectorAll('.moss-instrument-layer')).toHaveLength(1);
    on = false;
    bound.refresh();
    await settle(); await settle();
    expect(document.querySelectorAll('.moss-instrument-layer')).toHaveLength(0);
  });

  test('turning them off mid-crop stops the live crop and puts back the crop the page had', async () => {
    const p = page('object-position:10% 90%');
    let on = true;
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => on);
    await emit(HERO_IMAGE_FOCUS, { path: 'p.md', line: 1, position: '20% 30%' });
    on = false;
    bound.refresh();
    await emit(HERO_IMAGE_FOCUS, { path: 'p.md', line: 1, position: '25% 35%' });
    expect(p.restyles).toEqual(['20% 30%', '10% 90%']);
  });

  test('turning them off just after a crop was written leaves it for the rebuilt page', async () => {
    const p = page('object-position:10% 90%');
    let on = true;
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md', () => on);
    await emit(HERO_IMAGE_FOCUS, { path: 'p.md', line: 1, position: '20% 30%', held: true });
    on = false;
    bound.refresh();
    await settle(); await settle();
    expect(p.restyles).toEqual(['20% 30%']);
  });

  test('a crop whose page read was still pending when they went off does not restyle', async () => {
    let answer: (v: unknown) => void = () => {};
    const restyles: string[] = [];
    let on = true;
    const callBridge = vi.fn((method: string, _s: string, _p?: string, value?: string) => {
      if (method === 'document.querySelector') return new Promise((r) => { answer = r; });
      if (method === 'element.setStyle') restyles.push(value ?? '');
      return Promise.resolve(true);
    });
    bound = bindPreviewInstruments({ callBridge } as never, null, () => 'p.md', () => on);
    handlers.get(HERO_IMAGE_FOCUS)!({ path: 'p.md', line: 1, position: '20% 30%' });
    await settle();
    on = false;
    answer({ attributes: { style: '' } });
    await settle(); await settle();
    expect(restyles).toEqual([]);
  });

  test('a picture is still measured for the editor\'s focus point, which draws nothing on the preview', async () => {
    const p = previewed();
    bound = bindPreviewInstruments(p.nav as never, p.iframe, () => 'p.md', () => false);
    await emit(HERO_FRAME_REQUEST, { path: 'p.md', line: 3 });
    expect(host.bus.emit).toHaveBeenCalledWith(HERO_FRAME, { line: 3, width: 400, height: 200 });
    expect(document.querySelectorAll('.moss-instrument-layer')).toHaveLength(0);
  });
});

describe('pointOfCssPosition', () => {
  test.each([
    ['20% 62%', { x: 20, y: 62 }],
    ['top left', { x: 0, y: 0 }],
    ['left top', { x: 0, y: 0 }],
    ['left', { x: 0, y: 50 }],
    ['top', { x: 50, y: 0 }],
    ['center right', { x: 100, y: 50 }],
    ['bottom', { x: 50, y: 100 }],
  ])('%s', (value, point) => { expect(pointOfCssPosition(value)).toEqual(point); });

  test('anything else is not a point', () => {
    expect(pointOfCssPosition('')).toBeNull();
    expect(pointOfCssPosition('calc(10px) 2em')).toBeNull();
    expect(pointOfCssPosition('constructor')).toBeNull();
  });
});
