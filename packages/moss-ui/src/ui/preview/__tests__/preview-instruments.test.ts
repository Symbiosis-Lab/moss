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
import { HERO_IMAGE_FOCUS } from '../../editor/session/instrument-events';

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
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md');
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
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md');
    await focus({ path: 'p.md', line: 1, position: '20% 30%' });
    await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['20% 30%']);
  });

  test('putting it back restores the crop the page had, not no crop at all', async () => {
    const p = page('object-position:10% 90%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md');
    await focus({ path: 'p.md', line: 1, position: '20% 30%' });
    await focus({ path: 'p.md', line: 1, position: '25% 35%' });
    await focus({ path: 'p.md', line: 1, position: null });
    expect(p.restyles).toEqual(['20% 30%', '25% 35%', '10% 90%']);
  });

  test('a null after a held write (an undo) lets the hold go: the next rebuilt page is not overridden', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md');
    await focus({ path: 'p.md', line: 1, position: '0% 0%', held: true });
    await focus({ path: 'p.md', line: 1, position: null });
    expect(p.restyles).toEqual(['0% 0%', '50% 50%']);
    await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['0% 0%', '50% 50%']);
  });

  test('is let go after three rebuilt pages that never show it, so a failed write cannot pin the preview', async () => {
    const p = page('object-position:50% 50%');
    bound = bindPreviewInstruments(p.nav as never, null, () => 'p.md');
    await focus({ path: 'p.md', line: 1, position: '0% 0%', held: true });
    for (let i = 0; i < 5; i++) await morph(p, 'object-position:50% 50%');
    expect(p.restyles).toEqual(['0% 0%', '0% 0%', '0% 0%']);
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
