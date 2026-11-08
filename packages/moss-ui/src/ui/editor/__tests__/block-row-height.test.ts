import { describe, test, expect, beforeEach, vi } from 'vitest';

// block-row-height.ts holds module-level caches (column width, chrome heights,
// video aspect ratios) that are deliberately session-lived. Re-import it fresh
// for every test so no test can depend on another having run first — an earlier
// draft of this file needed a "keep this describe block first" comment, which
// `vitest --sequence.shuffle` promptly falsified.
type Mod = typeof import('../block-row-height');
let m: Mod;
beforeEach(async () => {
  vi.resetModules();
  m = await import('../block-row-height');
});

/** jsdom lays nothing out, so `offsetHeight` is 0 for everything. Stub it. */
function withHeight<T extends HTMLElement>(el: T, height: number): T {
  Object.defineProperty(el, 'offsetHeight', { value: height, configurable: true });
  return el;
}

function el(tag: string, className: string, height: number): HTMLElement {
  const node = document.createElement(tag);
  node.className = className;
  return withHeight(node, height);
}

/** A `.cm-content` stand-in holding one rendered widget of each shape. */
function contentWith(opts: { audio?: number; card?: number } = {}) {
  const content = document.createElement('div');
  if (opts.audio !== undefined) {
    const player = el('div', 'cm-media-embed-player', opts.audio);
    player.appendChild(withHeight(document.createElement('audio'), opts.audio));
    content.appendChild(player);
  }
  if (opts.card !== undefined) content.appendChild(el('div', 'cm-media-embed-card', opts.card));
  return content;
}

// ── The padding invariant ────────────────────────────────────────────────────
//
// Every export returns the height of the widget ROOT — the box CM6 measures —
// with that root's own vertical padding folded in. Callers must never add it;
// that forget-to-add split is exactly what this module consolidates, and it had
// already happened once (the margin fix moved spacing into padding and two of
// three estimates did not follow).
//
// These assert EXACT values on purpose. An earlier draft used
// `toBeGreaterThan(ROW_PADDING)`, which 54, 150 and 48 all satisfy with the
// padding stripped — the test was named for the invariant and could not see it
// break.
describe('every root height includes that root’s own padding', () => {
  test('media rows are ROW_PADDING + the chrome height, per shape', () => {
    expect(m.mediaRowHeight('audio')).toBe(m.ROW_PADDING + 54);
    expect(m.mediaRowHeight('video')).toBe(m.ROW_PADDING + 150);
    expect(m.mediaRowHeight('card')).toBe(m.ROW_PADDING + 44);
  });

  test('an image row is ROW_PADDING + the scaled image box', () => {
    expect(m.imageRowHeight(400, 300, undefined, 900)).toBe(m.ROW_PADDING + 300);
  });
});

// One estimate for every media shape is wrong by a lot: an audio embed renders
// 62px, so a page of audio embeds estimated at a 180px player carries ~126px of
// phantom height per embed until each scrolls into view.
describe('mediaRowHeight distinguishes the shapes it estimates', () => {
  test('audio is a bare control bar, nowhere near the video box', () => {
    expect(m.mediaRowHeight('audio')).toBe(62);
    expect(m.mediaRowHeight('audio')).toBeLessThan(m.mediaRowHeight('video'));
  });

  test('an unseen video is the default replaced box (no intrinsic size yet)', () => {
    // `preload="metadata"` means no intrinsic size at estimate time, so CSS
    // gives the element its 300x150 default (the default sizing rules in CSS Images 3).
    expect(m.mediaRowHeight('video', { src: 'moss-source://x/v.mp4' })).toBe(m.ROW_PADDING + 150);
  });

  test('a placeholder card is a single label row, shorter than either player', () => {
    expect(m.mediaRowHeight('card')).toBe(52);
  });
});

// Once a video has played anywhere in the session we know its real ratio, and
// the row it renders is columnWidth / ratio — measured at a 300px player:
// 16:9 → 176.75, 4:3 → 233, 1:1 → 308, 9:16 → 541.33. A flat 150 is off by
// -383px on that last one, three times the audio bug and in the growing
// direction. The first-sight case is one the DOM cannot answer.
describe('video rows scale with the aspect ratio once it is known', () => {
  const SRC = 'moss-source://localhost/clip.mp4';

  test('a known 16:9 ratio estimates the height it will render at this column', () => {
    m.recordVideoAspect(SRC, 640, 360);
    expect(m.mediaRowHeight('video', { src: SRC, columnWidth: 300 }))
      .toBe(m.ROW_PADDING + Math.round(300 / (16 / 9)));
  });

  test('a portrait clip is far taller than the 150px default — the worst case', () => {
    m.recordVideoAspect(SRC, 360, 640);
    const est = m.mediaRowHeight('video', { src: SRC, columnWidth: 300 });
    expect(est).toBe(m.ROW_PADDING + Math.round(300 / (360 / 640)));
    // The old flat estimate was 158; this is the ~383px error, now closed.
    expect(est - 158).toBeGreaterThan(370);
  });

  test('video UPSCALES past its intrinsic width, unlike an image', () => {
    // `.cm-media-embed-video { width: 100% }` — a 360px-wide clip fills an
    // 800px column. `imageRowHeight` caps at intrinsic; video must not.
    m.recordVideoAspect(SRC, 360, 640);
    expect(m.mediaRowHeight('video', { src: SRC, columnWidth: 1200 }))
      .toBeGreaterThan(m.mediaRowHeight('video', { src: SRC, columnWidth: 800 }));
    // Same dimensions through the image path: both columns exceed the intrinsic
    // 360, so both cap there and the height does not move.
    expect(m.imageRowHeight(360, 640, undefined, 1200)).toBe(m.imageRowHeight(360, 640, undefined, 800));
  });

  test('a |NN% token scales the player, so it scales the row', () => {
    m.recordVideoAspect(SRC, 640, 360);
    expect(m.mediaRowHeight('video', { src: SRC, widthToken: '50%', columnWidth: 800 }))
      .toBe(m.mediaRowHeight('video', { src: SRC, columnWidth: 400 }));
  });

  test('the ratio is keyed per source — one video does not answer for another', () => {
    m.recordVideoAspect(SRC, 360, 640);
    expect(m.mediaRowHeight('video', { src: 'moss-source://localhost/other.mp4', columnWidth: 300 }))
      .toBe(m.ROW_PADDING + 150);
  });

  test('a degenerate ratio is refused rather than dividing by zero', () => {
    m.recordVideoAspect(SRC, 0, 0);
    m.recordVideoAspect(SRC, 640, 0);
    m.recordVideoAspect(undefined, 640, 360);
    expect(m.mediaRowHeight('video', { src: SRC, columnWidth: 300 })).toBe(m.ROW_PADDING + 150);
  });
});

// Guards the no-drag CM6 "layout shifted" churn:
// ImageBlockWidget.estimatedHeight hardcoded a 544px column, but toDOM renders
// each block at min(realColumnWidth, intrinsicWidth). On a wide editor the
// off-viewport ESTIMATE (544) and the in-viewport MEASURE (real column) diverge,
// so total doc height swings ~2000px as images enter/leave the viewport →
// unprompted content jumps. The estimate must mirror toDOM's width policy using
// the real column width so estimate ≈ measured (no swing).
describe('imageRowHeight (mirrors toDOM width policy → estimate ≈ measured)', () => {
  test('a wide image scales to the ACTUAL column width, not a fixed 544', () => {
    // 2000x800 in a 900px column renders 900 wide → 360 tall. The bug estimated
    // it at col=544 → 218, diverging from the ~360 measured once it scrolls in.
    expect(m.imageRowHeight(2000, 800, undefined, 900)).toBe(m.ROW_PADDING + 360);
    expect(m.imageRowHeight(2000, 800, undefined, 544)).toBe(m.ROW_PADDING + 218);
  });

  test('a narrow image never upscales past its intrinsic width', () => {
    // 400x300 caps at 400 wide in any wider column → 300 tall, regardless of col.
    expect(m.imageRowHeight(400, 300, undefined, 900)).toBe(m.ROW_PADDING + 300);
    expect(m.imageRowHeight(400, 300, undefined, 544)).toBe(m.ROW_PADDING + 300);
  });

  test('an explicit |NN% token estimates against that fraction of the column', () => {
    // 50% of a 900 column → 450 wide → 225 tall.
    expect(m.imageRowHeight(2000, 1000, '50%', 900)).toBe(m.ROW_PADDING + 225);
    // An unparseable token falls back to the full column, as toDOM does.
    expect(m.imageRowHeight(2000, 800, 'wide', 900)).toBe(m.ROW_PADDING + 360);
  });

  // ── The height cap ──────────────────────────────────────────────
  //
  // Deliberately breaks size parity with the published page: while editing, a
  // picture is a landmark and the text around it is the work. The estimate has
  // to mirror the cap or every capped image is a scroll jump.

  test('a tall image is capped, whatever the column does', () => {
    // 400x1200 portrait: 1200 tall uncapped. Capped by NARROWING (400 tall at
    // 1:3 is 133 wide), so the picture keeps its shape and stays whole.
    for (const column of [544, 900, 1600]) {
      expect(m.imageRowHeight(400, 1200, undefined, column))
        .toBe(m.ROW_PADDING + m.IMAGE_HEIGHT_CAP);
    }
    expect(m.imageDisplayWidth(400, 1200, undefined, 900)).toBeCloseTo(133.3, 1);
  });

  test('the cap clamps a |NN% token too — asking for the column does not opt out', () => {
    expect(m.imageRowHeight(400, 1200, '100%', 900)).toBe(m.ROW_PADDING + m.IMAGE_HEIGHT_CAP);
  });

  test('an image that already fits is untouched by the cap', () => {
    // It is for pictures that take the page over, not a resize of every image.
    expect(m.imageRowHeight(400, 300, undefined, 900)).toBe(m.ROW_PADDING + 300);
    expect(m.imageDisplayWidth(400, 300, undefined, 900)).toBe(400);
  });

  test('unknown intrinsic dimensions fall back to a fixed estimate', () => {
    expect(m.imageRowHeight(undefined, undefined, undefined, 900)).toBe(m.ROW_PADDING + 200);
    expect(m.imageRowHeight(400, undefined, undefined, 900)).toBe(m.ROW_PADDING + 200);
  });

  test('a non-positive column width falls back so a pre-layout estimate is never zero', () => {
    // min(FALLBACK, 400)=400 → 300; guards against a 0/NaN column collapsing the
    // estimate to 0 (which would itself cause a swing on first measure).
    expect(m.imageRowHeight(400, 300, undefined, 0)).toBe(m.ROW_PADDING + 300);
    expect(m.imageRowHeight(2000, 1000, undefined, 0)).toBe(
      m.ROW_PADDING + Math.round((1000 / 2000) * Math.min(m.FALLBACK_COLUMN_WIDTH, 2000)),
    );
  });
});

describe('editorColumnWidth (live column cache)', () => {
  test('falls back until something measures a real column', () => {
    expect(m.editorColumnWidth()).toBe(m.FALLBACK_COLUMN_WIDTH);
  });

  test('a bogus column is ignored — a stale/absurd width freezes the editor', () => {
    // jsdom lays nothing out, so offsetWidth is 0 → the >120 guard rejects it.
    // Without the guard, estimatedHeight would collapse and CM6 would re-anchor
    // the scroll every frame (the 6-10s freeze).
    m.refreshEditorColumnWidth(document.createElement('div'));
    m.refreshEditorColumnWidth(null);
    expect(m.editorColumnWidth()).toBe(m.FALLBACK_COLUMN_WIDTH);
  });

  test('adopts a real column minus BOTH line insets, so it matches .cm-line', () => {
    const dom = document.createElement('div');
    Object.defineProperty(dom, 'offsetWidth', { value: 900 });
    m.refreshEditorColumnWidth(dom);
    expect(m.editorColumnWidth()).toBe(900 - 2 * 16);
  });
});

describe('sampleBlockChromeHeights', () => {
  test('is a no-op without a contentDOM (off-viewport estimate, no view)', () => {
    m.sampleBlockChromeHeights(null);
    m.sampleBlockChromeHeights(undefined);
    expect(m.mediaRowHeight('audio')).toBe(62);
  });

  test('is a no-op when nothing of that shape is on screen', () => {
    m.sampleBlockChromeHeights(document.createElement('div'));
    expect(m.mediaRowHeight('audio')).toBe(62);
    expect(m.mediaRowHeight('card')).toBe(52);
  });

  test('refuses an implausibly short measurement rather than collapsing the estimate', () => {
    // A detached, display:none or mid-teardown element measures ~0. Latching it
    // would make every off-viewport row estimate collapse to its padding for the
    // rest of the session — there is no later event that would undo it.
    m.sampleBlockChromeHeights(contentWith({ audio: 0, card: 3 }));
    expect(m.mediaRowHeight('audio')).toBe(62);
    expect(m.mediaRowHeight('card')).toBe(52);
  });

  test('adopts real measurements, so another engine or font size self-corrects', () => {
    // WKWebView and WebKitGTK draw a different control bar; the card is
    // font-metric derived and moves with the user's text size.
    m.sampleBlockChromeHeights(contentWith({ audio: 71, card: 60 }));
    expect(m.mediaRowHeight('audio')).toBe(m.ROW_PADDING + 71);
    expect(m.mediaRowHeight('card')).toBe(m.ROW_PADDING + 60);
  });
});
