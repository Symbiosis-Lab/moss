import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { ChipBar, serializeChipValue } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import type { ContentSchema } from '@symbiosis-lab/moss-ui';
import { setLocale } from "../../i18n/index";

function makeSchema(fields: Record<string, any>): ContentSchema {
  return {
    frontmatter: { fields },
  } as ContentSchema;
}

/**
 * Create a FrontmatterStore with a no-op requestSave mock and the given initial
 * values. The mock is returned as `write` so the existing assertions
 * (`expect(write).toHaveBeenCalled()`) keep reading as "a save was requested".
 */
function makeStore(initial: Record<string, unknown> = {}, writeMock?: ReturnType<typeof vi.fn>) {
  const write = writeMock ?? vi.fn().mockResolvedValue(undefined);
  const store = new FrontmatterStore({ initial, requestSave: write });
  return { store, write };
}

// ── Global ResizeObserver mock (jsdom doesn't have it) ─────────────────
let globalResizeObserverInstances: { observe: any; disconnect: any; cb: () => void }[] = [];

class MockResizeObserver {
  private cb: () => void;
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(cb: () => void) {
    this.cb = cb;
    globalResizeObserverInstances.push({ observe: this.observe, disconnect: this.disconnect, cb });
  }
}

vi.stubGlobal('ResizeObserver', MockResizeObserver);

describe('ChipBar', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    globalResizeObserverInstances = [];
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('only renders chips for schema fields that have values', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
      date: { type: 'string', widget: 'date-picker', priority: 30 },
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      tags: { type: 'array', widget: 'tag-input', priority: 50 },
      description: { type: 'string', widget: 'text-area', priority: 20 },
    });

    const values = { title: 'My Post', date: '2026-01-01' };

    const { store } = makeStore(values);
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    expect(chips.length).toBe(2); // only title and date

    const keys = Array.from(chips).map(
      (c) => (c as HTMLElement).dataset.key,
    );
    expect(keys).toEqual(['title', 'date']);

    renderer.destroy();
  });

  test('renders chips for unknown fields (not in schema) that have values', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });

    const values = { title: 'Hello', custom_field: 'surprise' };

    const { store } = makeStore(values);
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    expect(chips.length).toBe(2); // title + custom_field

    renderer.destroy();
  });

  test('does not render chips for schema fields with undefined values', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
    });

    const values = { title: 'Present', draft: undefined };

    const { store } = makeStore(values);
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    expect(chips.length).toBe(1);

    renderer.destroy();
  });

  test('boolean chip toggles value on click', () => {
    const schema = makeSchema({
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
    });

    const { store, write } = makeStore({ draft: false });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    expect(chip.classList.contains('bool-off')).toBe(true);

    chip.click();
    // Write fn is called (immediate write on toggle).
    expect(write).toHaveBeenCalled();
    expect(renderer.getValues().draft).toBe(true);

    renderer.destroy();
  });

  test('tag chip displays tag values with chip-accent spans', () => {
    const schema = makeSchema({
      tags: { type: 'array', widget: 'tag-input', items: { type: 'string' }, priority: 50 },
    });

    const { store } = makeStore({ tags: ['历史', '瑜伽'] });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    const accents = chip.querySelectorAll('.chip-accent');
    expect(accents.length).toBe(2);
    expect(accents[0].textContent).toBe('历史');
    expect(accents[1].textContent).toBe('瑜伽');

    renderer.destroy();
  });

  test('getValues returns current values', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });

    const { store } = makeStore({ title: 'Test' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const vals = renderer.getValues();
    expect(vals.title).toBe('Test');

    renderer.destroy();
  });

  test('destroy cleans up container', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });

    const { store } = makeStore({ title: 'Test' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);
    expect(container.children.length).toBeGreaterThan(0);

    renderer.destroy();
    expect(container.innerHTML).toBe('');
  });

  test('chip uses label when present', () => {
    const schema = makeSchema({
      children_depth: { type: 'string', widget: 'select', label: 'Depth', enum_values: ['direct', 'all'], priority: 100 },
    });

    const { store } = makeStore({ children_depth: 'direct' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    const keyEl = chip.querySelector('.chip-key') as HTMLElement;
    expect(keyEl.textContent).toBe('Depth');

    renderer.destroy();
  });

  test('chip falls back to key when label is absent', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });

    const { store } = makeStore({ title: 'Hello' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    const keyEl = chip.querySelector('.chip-key') as HTMLElement;
    // chipLabel('title') returns the i18n label 'Title' (not the raw key)
    expect(keyEl.textContent).toBe('Title');

    renderer.destroy();
  });

  test('origin chip resolves its i18n label like sibling external_url', () => {
    // Guards the ui-strings.ts entry: without it the chip shows the raw key.
    const schema = makeSchema({
      origin: { type: 'string', widget: 'text-input', priority: 70, label_key: 'chip.origin.label' },
    });

    const { store } = makeStore({ origin: 'https://example.com/original-post' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    const keyEl = chip.querySelector('.chip-key') as HTMLElement;
    expect(keyEl.textContent).toBe('Imported from');

    renderer.destroy();
  });

  test('chip falls back to field name for unknown schema fields', () => {
    // Fields not in the i18n registry fall back to the raw field name.
    // The deprecated def.label is no longer the display source.
    const schema = makeSchema({
      some_toggle: { type: 'boolean', widget: 'checkbox', label: 'My Toggle', priority: 80 },
    });

    const { store } = makeStore({ some_toggle: true });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip') as HTMLElement;
    const keyEl = chip.querySelector('.chip-key') as HTMLElement;
    // 'some_toggle' has no i18n key — chipLabel returns the field name
    expect(keyEl.textContent).toBe('some_toggle');

    renderer.destroy();
  });

  test('chips are sorted by priority (lower number = first)', () => {
    const schema = makeSchema({
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      title: { type: 'string', widget: 'text-input', priority: 10 },
      tags: { type: 'array', widget: 'tag-input', priority: 50 },
      description: { type: 'string', widget: 'text-area', priority: 20 },
    });

    const values = { draft: false, title: 'Hello', tags: ['a'], description: 'desc' };
    const { store } = makeStore(values);
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    const keys = Array.from(chips).map(c => (c as HTMLElement).dataset.key);
    // Expected order: title(10), description(20), tags(50), draft(60)
    expect(keys).toEqual(['title', 'description', 'tags', 'draft']);

    renderer.destroy();
  });

  test('fields without priority sort to the end', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
      custom: { type: 'string', widget: 'text-input' }, // no priority
    });

    const values = { title: 'Hello', custom: 'world' };
    const { store } = makeStore(values);
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    const keys = Array.from(chips).map(c => (c as HTMLElement).dataset.key);
    expect(keys).toEqual(['title', 'custom']);

    renderer.destroy();
  });

  test('unified button always rendered', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
    });
    const { store } = makeStore({ title: 'Test' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
    expect(btn).not.toBeNull();
    expect(btn.textContent).toBe('+');
    expect(btn.classList.contains('round')).toBe(true);

    renderer.destroy();
  });

  test('empty page shows only unified "+" button', () => {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10, group: 'This Page' },
      draft: { type: 'boolean', widget: 'checkbox', priority: 92, group: 'This Page' },
    });
    const { store } = makeStore({});
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chips = container.querySelectorAll('.chip');
    expect(chips.length).toBe(0);

    const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
    expect(btn).not.toBeNull();
    expect(btn.textContent).toBe('+');

    renderer.destroy();
  });

  // ── Overflow detection ───────────────────────────────────────────────

  describe('overflow detection', () => {
    let rafCallbacks: (() => void)[];

    beforeEach(() => {
      rafCallbacks = [];
      vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
        rafCallbacks.push(cb);
        return rafCallbacks.length;
      });
    });

    afterEach(() => {
      vi.unstubAllGlobals();
      vi.stubGlobal('ResizeObserver', MockResizeObserver);
    });

    /**
     * Set up mocked getBoundingClientRect for the bar, the .chip-track, and
     * the chips. Overflow is now measured against the TRACK's right edge (the
     * two-box layout — the button is a sibling box past the track), so the
     * track rect is what the threshold is read from. `trackRight` defaults to
     * 52px inside the bar to model the unified button's box.
     * chipRights: right-edge position of each chip, left-to-right.
     * barRight: the outer right edge of the bar element (border-box).
     */
    function setupRects(
      bar: HTMLElement,
      chipRights: number[],
      barRight: number,
      trackRight: number = barRight - 52,
    ): void {
      bar.getBoundingClientRect = () =>
        ({ right: barRight, left: 0, top: 0, bottom: 0, width: barRight, height: 30 } as DOMRect);

      const track = bar.querySelector('.chip-track') as HTMLElement | null;
      if (track) {
        track.getBoundingClientRect = () =>
          ({ right: trackRight, left: 0, top: 0, bottom: 0, width: trackRight, height: 32 } as DOMRect);
      }

      const chips = bar.querySelectorAll('.chip');
      chips.forEach((el, i) => {
        const right = chipRights[i] ?? 0;
        (el as HTMLElement).getBoundingClientRect = () =>
          ({ right, left: 0, top: 0, bottom: 0, width: right, height: 32 } as DOMRect);
      });
    }

    test('no overflow: all chips visible, unified button shows "+"', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        date: { type: 'string', widget: 'date-picker', priority: 30 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
      });
      const { store } = makeStore({ title: 'My Post', date: '2026-01-01', tags: ['a'] });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      setupRects(container, [80, 160, 200], 400);
      rafCallbacks[0]();

      const chips = container.querySelectorAll('.chip') as NodeListOf<HTMLElement>;
      const hiddenChips = Array.from(chips).filter(c => c.style.display === 'none');
      expect(hiddenChips).toHaveLength(0);

      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      expect(btn.textContent).toBe('+');
      expect(btn.classList.contains('round')).toBe(true);

      renderer.destroy();
    });

    test('priority-based overflow: lowest priority chip hidden first', () => {
      // 4 chips: title(10), description(20), tags(50), draft(60)
      // Bar is narrow — not all fit. Lowest priority (draft=60) should be hidden first.
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
        draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      });
      const { store } = makeStore({ title: 'My Post', description: 'desc', tags: ['a'], draft: false });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      // Bar: 300px, track right edge = 300 - 52 = 248 (button box past it).
      // 4 chips at 80, 160, 240, 290. Threshold = track right = 248.
      // chip[3] (draft, priority 60, right=290) > 248 → hide first (lowest pri)
      // chip[2] (tags, priority 50, right=240) ≤ 248 → keep once draft is gone
      setupRects(container, [80, 160, 240, 290], 300);
      rafCallbacks[0]();

      const chips = container.querySelectorAll('.chip') as NodeListOf<HTMLElement>;
      const chipArr = Array.from(chips);
      // title(10), description(20), tags(50) fit within the track → visible
      expect(chipArr[0].style.display).not.toBe('none'); // title
      expect(chipArr[1].style.display).not.toBe('none'); // description
      expect(chipArr[2].style.display).not.toBe('none'); // tags
      // draft(60), the lowest priority, is the only one over the track edge
      expect(chipArr[3].style.display).toBe('none'); // draft

      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      expect(btn.textContent).toBe('+1');
      expect(btn.classList.contains('round')).toBe(false);

      renderer.destroy();
    });

    test('ResizeObserver is set up and observes the container', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Test' });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      expect(globalResizeObserverInstances).toHaveLength(1);
      expect(globalResizeObserverInstances[0].observe).toHaveBeenCalledWith(container);

      renderer.destroy();
    });

    test('ResizeObserver disconnected on destroy', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Test' });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      const observer = globalResizeObserverInstances[0];
      renderer.destroy();
      expect(observer.disconnect).toHaveBeenCalled();
    });

    test('overflow recalculates: chips re-appear when container grows', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        date: { type: 'string', widget: 'date-picker', priority: 30 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
      });
      const { store } = makeStore({ title: 'My Post', date: '2026-01-01', tags: ['a'] });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      // Start narrow: some chips overflow
      setupRects(container, [80, 160, 200], 200);
      rafCallbacks[0]();

      const chips = container.querySelectorAll('.chip') as NodeListOf<HTMLElement>;
      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;

      // Some chips should be hidden
      const hiddenCount = Array.from(chips).filter(c => c.style.display === 'none').length;
      expect(hiddenCount).toBeGreaterThan(0);
      expect(btn.textContent).toContain('+');

      // Now container grows — all chips fit
      setupRects(container, [80, 160, 200], 400);
      globalResizeObserverInstances[0].cb();

      // All chips should be visible again
      expect(chips[0].style.display).not.toBe('none');
      expect(chips[1].style.display).not.toBe('none');
      expect(chips[2].style.display).not.toBe('none');
      expect(btn.textContent).toBe('+');
      expect(btn.classList.contains('round')).toBe(true);

      renderer.destroy();
    });

    // ── Button pre-sizing ──────────────
    // Bug: the track right edge was captured while the button was in its narrow
    // `.round` form (32px). After overflow was found, `updateUnifiedBtn()` grew
    // the button to "+N" form (wider due to padding), shrinking the track. Chips
    // that appeared to fit within the round-button track right edge then got
    // visually clipped by the narrower final track. The fix: pre-size the button
    // to its widest form before capturing `trackRight` so the inline-chip budget
    // never overcounts. This test asserts the invariant that EVERY visible chip's
    // right edge is <= the track right edge as measured with the button in its
    // ROUND form (the pre-expansion value that the measurement code now avoids).
    // We model the post-expansion track as narrower (smaller trackRight) and
    // verify no visible chip exceeds it — the fix must account for this.
    test('button is pre-sized to widest form before capturing trackRight (no clipped chips on overflow)', () => {
      // Setup: 4 chips where chip[2] fits within the round-button track
      // (268px) but overflows the wider +N-button track (240px).
      // With the bug: chip[2] (right=241) appears to fit (≤ 268 round-form),
      // but after the button grows the track shrinks to 240 and chip[2] gets clipped.
      // With the fix: trackRight is captured at 240 (wide-button track), so
      // chip[2] (right=241) is correctly detected as overflowing and hidden.
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
        draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      });
      const { store } = makeStore({ title: 'My Post', description: 'desc', tags: ['a'], draft: false });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      // Bar: 300px.
      // Round-button track right: 268 (button = 32px, gap = 0px placeholder).
      // Wide-button track right: 240 (button wider after overflow found).
      // Chips at 80, 160, 241, 270.
      //   chip[0]=80, chip[1]=160 fit within 240.
      //   chip[2]=241 fits within 268 (round) but NOT within 240 (wide) — the clipped chip.
      //   chip[3]=270 overflows either form.
      // The fix must hide chip[2] (and chip[3]) so no visible chip > 240.
      const ROUND_TRACK_RIGHT = 268;
      const WIDE_TRACK_RIGHT  = 240; // smaller — represents the track after button grows

      container.getBoundingClientRect = () =>
        ({ right: 300, left: 0, top: 0, bottom: 0, width: 300, height: 30 } as DOMRect);

      // Before the measurement starts, the button is in `.round` form.
      // `measureOverflow` should expand it before reading the track rect.
      // We return WIDE_TRACK_RIGHT always, since the fix ensures the button is
      // already in wide form when getBoundingClientRect is called on the track.
      const track = container.querySelector('.chip-track') as HTMLElement;
      track.getBoundingClientRect = () => {
        // The fix pre-expands the button before calling getBoundingClientRect on
        // the track. By this time `.round` should already be removed.
        const btnEl = container.querySelector('.chip-unified-btn') as HTMLElement;
        const isRound = btnEl.classList.contains('round');
        // We return different values depending on button form — this makes the
        // test self-documenting: with the bug, the button is still round here
        // (trackRight too large) and the last visible chip gets clipped.
        return {
          right: isRound ? ROUND_TRACK_RIGHT : WIDE_TRACK_RIGHT,
          left: 0, top: 0, bottom: 0,
          width: isRound ? ROUND_TRACK_RIGHT : WIDE_TRACK_RIGHT,
          height: 32,
        } as DOMRect;
      };

      const chipEls = Array.from(container.querySelectorAll('.chip')) as HTMLElement[];
      [80, 160, 241, 270].forEach((right, i) => {
        chipEls[i].getBoundingClientRect = () =>
          ({ right, left: right - 60, top: 0, bottom: 0, width: 60, height: 32 } as DOMRect);
      });

      rafCallbacks[0]();

      // With the fix: measureOverflow captures trackRight=240 (wide form — the
      // button is pre-expanded BEFORE the track rect is read).
      // chip[0]=80, chip[1]=160 fit; chip[2]=241 > 240 → hidden; chip[3]=270 → hidden.
      // Visible chips (80, 160) all sit within 240. ✓
      // With the bug: measureOverflow captures trackRight=268 (round form).
      // chip[3]=270 > 268 → hide chip[3]. Remaining chip[2]=241 ≤ 268 → loop stops.
      // But the button then grows → real track shrinks to 240 → chip[2]=241 is
      // clipped (241 > 240) yet still visible → the assertion below FAILS (the
      // regression guard: reverting the fix turns this test red).
      //
      // The key invariant: every visible chip's right edge ≤ the FINAL track width
      // (WIDE_TRACK_RIGHT), not the pre-expansion round-button width.
      chipEls.forEach((chip) => {
        if (chip.style.display === 'none') return;
        expect(chip.getBoundingClientRect().right).toBeLessThanOrEqual(WIDE_TRACK_RIGHT);
      });

      // Button should now be in "+N" form (not round) since there is overflow.
      const btnFinal = container.querySelector('.chip-unified-btn') as HTMLElement;
      expect(btnFinal.textContent).toMatch(/^\+\d+$/);
      expect(btnFinal.classList.contains('round')).toBe(false);

      renderer.destroy();
    });
  });

  // ── Unified dropdown ───────────────────────────────────────────────

  describe('unified dropdown', () => {
    test('clicking unified button opens dropdown with grouped unset fields', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10, group: 'This Page' },
        draft: { type: 'boolean', widget: 'checkbox', priority: 92, group: 'This Page' },
        children: { type: 'string', widget: 'checkbox', priority: 60, group: 'Child Pages' },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      btn.click();

      const menu = document.querySelector('.omenu') as HTMLElement;
      expect(menu).not.toBeNull();

      // Should have group headers rendered as their i18n display names.
      // GROUP_KEY_MAP translates 'This Page' -> t('group.this_page') = 'This Page' (en locale).
      const groups = menu.querySelectorAll('.omenu-group');
      const groupTexts = Array.from(groups).map(g => g.textContent);
      expect(groupTexts).toContain('This Page');
      expect(groupTexts).toContain('Child Pages');

      // Should have a search input
      const search = menu.querySelector('.omenu-search') as HTMLInputElement;
      expect(search).not.toBeNull();

      // Should NOT have "Set on this page" section (no overflow)
      expect(groupTexts).not.toContain('Set on this page');

      renderer.destroy();
    });

    // ── Transactional property add ─────────────────────────────────────
    // Clicking an add
    // row writes NOTHING — a ghosted provisional chip opens the editor; the
    // first commit is the first disk write; dismissing with nothing committed
    // leaves the store and the file untouched.
    describe('transactional property add', () => {
      function renderBar(fields: Record<string, any>, initial: Record<string, unknown> = { title: 'Hello' }) {
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10, group: 'Common' },
          ...fields,
        });
        const { store, write } = makeStore(initial);
        const renderer = new ChipBar(schema, store);
        renderer.render(container);
        return { store, write, renderer };
      }

      function clickAddRow(labelText: string): void {
        const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
        btn.click();
        const menu = document.querySelector('.omenu') as HTMLElement;
        const row = Array.from(menu.querySelectorAll('.orow-add')).find(
          r => r.textContent?.includes(labelText),
        ) as HTMLElement;
        expect(row).toBeTruthy();
        row.click();
      }

      test('add opens the popover WITHOUT writing: ghost chip, untouched store', () => {
        const { store, write, renderer } = renderBar({
          description: { type: 'string', widget: 'text-area', priority: 20, group: 'Common' },
        });
        clickAddRow('Description');

        // Ghost chip at the front, popover open for it.
        const ghost = container.querySelector('.chip[data-key="description"]') as HTMLElement;
        expect(ghost).toBeTruthy();
        expect(ghost.classList.contains('chip--provisional')).toBe(true);
        expect((container.querySelectorAll('.chip')[0] as HTMLElement).dataset.key).toBe('description');
        expect(document.body.querySelector('.pop .pop-title')?.textContent).toBe('Description');

        // The transactional core: nothing was written anywhere.
        expect(store.getValues()).not.toHaveProperty('description');
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });

      test('dismissing via × removes the ghost; the store and file never changed', () => {
        const { store, write, renderer } = renderBar({
          description: { type: 'string', widget: 'text-area', priority: 20, group: 'Common' },
        });
        clickAddRow('Description');

        (document.body.querySelector('.pop-x') as HTMLElement).click();

        expect(container.querySelector('.chip[data-key="description"]')).toBeNull();
        expect(store.getValues()).not.toHaveProperty('description');
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });

      test('Escape in the editor is the same dismissal', () => {
        const { store, write, renderer } = renderBar({
          subtitle: { type: 'string', widget: 'text-input', priority: 20, group: 'Common' },
        });
        clickAddRow('Subtitle');
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

        expect(container.querySelector('.chip[data-key="subtitle"]')).toBeNull();
        expect(store.getValues()).not.toHaveProperty('subtitle');
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });

      test('first commit materializes the field with the committed value (text)', () => {
        const { store, write, renderer } = renderBar({
          subtitle: { type: 'string', widget: 'text-input', priority: 20, group: 'Common' },
        });
        clickAddRow('Subtitle');
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        inp.value = 'a real value';
        inp.dispatchEvent(new Event('input', { bubbles: true }));

        expect(store.getValues().subtitle).toBe('a real value');
        // Ghost styling drops in place; text writes are debounced, so a save
        // is queued rather than already flushed.
        const chip = container.querySelector('.chip[data-key="subtitle"]') as HTMLElement;
        expect(chip.classList.contains('chip--provisional')).toBe(false);
        expect(store.hasPendingWrite()).toBe(true);

        // Closing afterwards keeps the (now real) chip.
        (document.body.querySelector('.pop-x') as HTMLElement).click();
        expect(container.querySelector('.chip[data-key="subtitle"]')).toBeTruthy();
        expect(store.getValues().subtitle).toBe('a real value');

        renderer.destroy();
        expect(write).not.toHaveBeenCalled(); // debounce cancelled by destroy — never a phantom write in tests
      });

      test('date add: today is prefilled as a SUGGESTION, not written; Enter accepts it', () => {
        const { store, write, renderer } = renderBar({
          date: { type: 'string', widget: 'date-picker', priority: 30, group: 'Common' },
        });
        clickAddRow('Date');

        const now = new Date();
        const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        expect(inp.value).toBe(today);
        // Suggestion only — nothing committed yet.
        expect(store.getValues()).not.toHaveProperty('date');
        expect(write).not.toHaveBeenCalled();

        // Enter accepts the suggestion: the first (and only) write.
        inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
        expect(store.getValues().date).toBe(today);
        expect(write).toHaveBeenCalledTimes(1);
        expect(document.body.querySelector('.pop')).toBeNull();
        renderer.destroy();
      });

      test('date add then dismiss: the suggestion was never written', () => {
        const { store, write, renderer } = renderBar({
          date: { type: 'string', widget: 'date-picker', priority: 30, group: 'Common' },
        });
        clickAddRow('Date');
        (document.body.querySelector('.pop-x') as HTMLElement).click();

        expect(store.getValues()).not.toHaveProperty('date');
        expect(container.querySelector('.chip[data-key="date"]')).toBeNull();
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });

      test('tags add: popover opens the tag editor; Enter-adding a tag is the first write', () => {
        const { store, write, renderer } = renderBar({
          tags: { type: 'array', widget: 'tag-input', priority: 50, group: 'Common' },
        });
        clickAddRow('Tags');

        // The old add path wrote `tags: []` immediately and fell into a text
        // editor for undefined values; now the real tag editor opens, unwritten.
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        expect(document.body.querySelector('.pop .pop-tags')).toBeTruthy();
        expect(store.getValues()).not.toHaveProperty('tags');
        expect(write).not.toHaveBeenCalled();

        inp.value = 'yoga';
        inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
        // `tags` is a plain array field, not a `oneof` string-or-list — a
        // one-name edit stays a list. Only author/editor/jury collapse.
        expect(store.getValues().tags).toEqual(['yoga']);
        expect(write).toHaveBeenCalledTimes(1);
        renderer.destroy();
      });

      test('union add follows popover rules: opens unwritten, choosing On is the first write', () => {
        const { store, write, renderer } = renderBar({
          children: {
            type: 'union', widget: 'union', priority: 60, group: 'Common',
            one_of: [{ type: 'boolean' }, { type: 'string', widget: 'wikilink-picker' }],
          },
        });
        clickAddRow('Children');

        // Popover open (a union chip's click has always opened its popover),
        // ghost chip present, nothing written.
        const on = document.body.querySelector('.pop .pop-union-seg') as HTMLElement;
        expect(on.textContent).toBe('On');
        expect(container.querySelector('.chip[data-key="children"] .chip-value')?.textContent).toBe('On');
        expect(container.querySelector('.chip[data-key="children"]')?.classList.contains('chip--provisional')).toBe(true);
        expect(store.getValues()).not.toHaveProperty('children');
        expect(write).not.toHaveBeenCalled();

        // Choosing On materializes with `true` — the first write.
        on.click();
        expect(store.getValues().children).toBe(true);
        expect(write).toHaveBeenCalledTimes(1);
        renderer.destroy();
      });

      describe('boolean add: the ghost IS the toggle', () => {
        test('no popover opens; the ghost previews the default value', () => {
          const { store, write, renderer } = renderBar({
            draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
          });
          clickAddRow('Draft');

          expect(document.body.querySelector('.pop')).toBeNull();
          const ghost = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
          expect(ghost.classList.contains('chip--provisional')).toBe(true);
          expect(ghost.classList.contains('bool-off')).toBe(true); // default false, previewed
          expect(store.getValues()).not.toHaveProperty('draft');
          expect(write).not.toHaveBeenCalled();
          renderer.destroy();
        });

        test('clicking the ghost commits the previewed value — the first write', () => {
          const { store, write, renderer } = renderBar({
            draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
          });
          clickAddRow('Draft');
          (container.querySelector('.chip[data-key="draft"]') as HTMLElement).click();

          expect(store.getValues().draft).toBe(false);
          expect(write).toHaveBeenCalledTimes(1);
          // Re-rendered as a real chip; a second click toggles normally.
          const chip = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
          expect(chip.classList.contains('chip--provisional')).toBe(false);
          chip.click();
          expect(store.getValues().draft).toBe(true);
          renderer.destroy();
        });

        test('click-away dismisses the ghost without a write', async () => {
          const { store, write, renderer } = renderBar({
            draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
          });
          clickAddRow('Draft');
          // Dismiss listeners register on the next tick (same deferral as
          // openOverlay, so the creating click can't self-dismiss).
          await new Promise(r => setTimeout(r, 0));

          const outside = document.createElement('div');
          document.body.appendChild(outside);
          outside.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));

          expect(container.querySelector('.chip[data-key="draft"]')).toBeNull();
          expect(store.getValues()).not.toHaveProperty('draft');
          expect(write).not.toHaveBeenCalled();
          renderer.destroy();
        });

        test('Escape dismisses the ghost without a write', async () => {
          const { store, write, renderer } = renderBar({
            draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
          });
          clickAddRow('Draft');
          await new Promise(r => setTimeout(r, 0));

          document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

          expect(container.querySelector('.chip[data-key="draft"]')).toBeNull();
          expect(store.getValues()).not.toHaveProperty('draft');
          expect(write).not.toHaveBeenCalled();
          renderer.destroy();
        });
      });
    });

    // ── Clearing removes the key (absence is the canonical empty) ──────
    describe('clearing a value removes the key', () => {
      test('clearing a select commits removeField — key: null is never written', () => {
        const schema = makeSchema({
          children_depth: { type: 'string', widget: 'select', enum_values: ['direct', 'all'], priority: 100 },
        });
        const { store, write } = makeStore({ children_depth: 'direct' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        (container.querySelector('.chip[data-key="children_depth"]') as HTMLElement).click();
        const sel = document.body.querySelector('.pop select.pop-input') as HTMLSelectElement;
        sel.value = '';
        sel.dispatchEvent(new Event('change', { bubbles: true }));

        // The key is GONE — not present as null.
        expect('children_depth' in store.getValues()).toBe(false);
        expect(write).toHaveBeenCalledTimes(1); // removeField writes immediately
        renderer.destroy();
      });

      test('clearing a date input removes the key', () => {
        const schema = makeSchema({
          date: { type: 'string', widget: 'date-picker', priority: 30 },
        });
        const { store } = makeStore({ date: '2026-01-01' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        (container.querySelector('.chip[data-key="date"]') as HTMLElement).click();
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        inp.value = '';
        inp.dispatchEvent(new Event('change', { bubbles: true }));

        expect('date' in store.getValues()).toBe(false);
        renderer.destroy();
      });

      test('clearing a number input removes the key', () => {
        const schema = makeSchema({
          weight: { type: 'number', widget: 'number-input', priority: 40 },
        });
        const { store } = makeStore({ weight: 5 });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        (container.querySelector('.chip[data-key="weight"]') as HTMLElement).click();
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        inp.value = '';
        inp.dispatchEvent(new Event('input', { bubbles: true }));

        expect('weight' in store.getValues()).toBe(false);
        renderer.destroy();
      });
    });

    // ── Legacy noise stays visible ─────────────────────────────────────
    describe('legacy key: null / key: "" render an explicit Empty chip', () => {
      test('a null value renders a chip with the Empty label instead of hiding', () => {
        const schema = makeSchema({
          subtitle: { type: 'string', widget: 'text-input', priority: 20 },
        });
        const { store } = makeStore({ subtitle: null });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        const chip = container.querySelector('.chip[data-key="subtitle"]') as HTMLElement;
        expect(chip).toBeTruthy();
        const empty = chip.querySelector('.chip-value-empty') as HTMLElement;
        expect(empty?.textContent).toBe('empty');
        renderer.destroy();
      });

      test('an empty-string value gets the same Empty treatment', () => {
        const schema = makeSchema({
          subtitle: { type: 'string', widget: 'text-input', priority: 20 },
        });
        const { store } = makeStore({ subtitle: '' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        const empty = container.querySelector('.chip[data-key="subtitle"] .chip-value-empty');
        expect(empty?.textContent).toBe('empty');
        renderer.destroy();
      });

      test('a null key is SET — it does not also appear in the add-property list', () => {
        const schema = makeSchema({
          subtitle: { type: 'string', widget: 'text-input', priority: 20, group: 'Common' },
          draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
        });
        const { store } = makeStore({ subtitle: null });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        (container.querySelector('.chip-unified-btn') as HTMLElement).click();
        const menu = document.querySelector('.omenu') as HTMLElement;
        const addLabels = Array.from(menu.querySelectorAll('.orow-add')).map(r => r.textContent);
        expect(addLabels.some(l => l?.includes('subtitle'))).toBe(false);
        expect(addLabels.some(l => l?.includes('Draft'))).toBe(true);
        renderer.destroy();
      });

      test('editing a legacy null field behaves normally: committing a value replaces it', () => {
        const schema = makeSchema({
          subtitle: { type: 'string', widget: 'text-input', priority: 20 },
        });
        const { store } = makeStore({ subtitle: null });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        (container.querySelector('.chip[data-key="subtitle"]') as HTMLElement).click();
        const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
        expect(inp.value).toBe(''); // null shown as empty input, not "null"
        inp.value = 'now real';
        inp.dispatchEvent(new Event('input', { bubbles: true }));

        expect(store.getValues().subtitle).toBe('now real');
        renderer.destroy();
      });
    });

    test('clicking a hidden field row promotes it to first chip and opens its editor', () => {
      // Reuse the rAF/setupRects machinery from the overflow describe block
      const rafCallbacks: (() => void)[] = [];
      vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
        rafCallbacks.push(cb);
        return rafCallbacks.length;
      });

      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
        draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      });
      const { store } = makeStore({ title: 'My Post', description: 'desc', tags: ['a'], draft: false });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      // Force overflow so draft (priority 60) and tags (priority 50) get hidden.
      container.getBoundingClientRect = () =>
        ({ right: 300, left: 0, top: 0, bottom: 0, width: 300, height: 30 } as DOMRect);
      const chips0 = container.querySelectorAll('.chip');
      [80, 160, 240, 290].forEach((right, i) => {
        (chips0[i] as HTMLElement).getBoundingClientRect = () =>
          ({ right, left: right - 60, top: 0, bottom: 0, width: 60, height: 32 } as DOMRect);
      });
      rafCallbacks[0]();

      // Open more menu, then click the "draft" row in "Set on this page".
      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      btn.click();
      const menu = document.querySelector('.omenu') as HTMLElement;
      const draftRow = Array.from(menu.querySelectorAll('.orow')).find(
        r => r.querySelector('.ok')?.textContent === 'Draft',
      ) as HTMLElement;
      expect(draftRow).toBeTruthy();
      draftRow.click();

      // Promoted field is now the first chip in the bar.
      const chipsAfter = container.querySelectorAll('.chip');
      expect((chipsAfter[0] as HTMLElement).dataset.key).toBe('draft');

      // The popover that's open is specifically for the promoted field — guards
      // against a stale popover from a previous interaction also satisfying
      // `.pop exists`.
      const pop = document.body.querySelector('.pop') as HTMLElement;
      expect(pop).toBeTruthy();
      expect(pop.querySelector('.pop-title')?.textContent).toBe('Draft');

      vi.unstubAllGlobals();
      vi.stubGlobal('ResizeObserver', MockResizeObserver);
      renderer.destroy();
    });

    // ── Two-box layout invariant ──────────────
    // Promoting a chip wider than the track must NEVER leave a partially-clipped
    // chip (invariant b — the JS demotion policy, fully checkable here) and the
    // "+" / "+N" button must stay structurally outside the chips' clipping box
    // (invariant a — the part jsdom can verify). The pixel-level "button stays
    // on screen" guarantee is a flexbox property checked in a real browser by
    // a browser test that measures the rendered chip geometry.
    test('promoting an over-wide chip keeps the + button outside the track and clips no chip', () => {
      const rafCallbacks: (() => void)[] = [];
      vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
        rafCallbacks.push(cb);
        return rafCallbacks.length;
      });

      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
      });
      // `description` holds a value far wider than the bar — once promoted it
      // is the lone unhideable chip that used to shove the button off-screen.
      const { store } = makeStore({
        title: 'My Post',
        description: 'A desktop app that turns folders into websites '.repeat(8),
        tags: ['a'],
      });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const BAR_RIGHT = 400;
      const TRACK_RIGHT = 360; // bar minus the button's own box

      // Stub the bar, the track box, and per-chip rects. `setupTwoBoxRects`
      // measures the track edge — there is no magic 52px reserve any more.
      const setupTwoBoxRects = (chipWidths: Record<string, number>) => {
        container.getBoundingClientRect = () =>
          ({ right: BAR_RIGHT, left: 0, top: 0, bottom: 0, width: BAR_RIGHT, height: 30 } as DOMRect);
        const track = container.querySelector('.chip-track') as HTMLElement | null;
        if (track) {
          track.getBoundingClientRect = () =>
            ({ right: TRACK_RIGHT, left: 0, top: 0, bottom: 0, width: TRACK_RIGHT, height: 32 } as DOMRect);
        }
        // Lay chips out left-to-right by their stubbed widths, in DOM order.
        let cursor = 0;
        container.querySelectorAll('.chip').forEach((el) => {
          const key = (el as HTMLElement).dataset.key ?? '';
          const w = chipWidths[key] ?? 60;
          const left = cursor;
          const right = cursor + w;
          cursor = right + 10; // gap
          (el as HTMLElement).getBoundingClientRect = () =>
            ({ right, left, top: 0, bottom: 0, width: w, height: 32 } as DOMRect);
        });
      };

      // Drain every queued rAF (renderChips queues measureOverflow; other
      // paths queue focus). Re-stub before each drain so measureOverflow sees
      // the current DOM. flushRafs runs whatever is pending and clears it.
      const flushRafs = () => {
        const pending = rafCallbacks.splice(0);
        for (const cb of pending) cb();
      };

      // Initial layout: everything narrow, description overflows so it lands
      // in the more menu.
      setupTwoBoxRects({ title: 60, description: 600, tags: 60 });
      flushRafs();

      // Promote the wide `description` from the more menu.
      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      btn.click();
      const menu = document.querySelector('.omenu') as HTMLElement;
      const descRow = Array.from(menu.querySelectorAll('.orow')).find(
        r => r.querySelector('.ok')?.textContent === 'Description',
      ) as HTMLElement;
      expect(descRow).toBeTruthy();
      descRow.click();

      // promoteAndEdit re-rendered (queuing measureOverflow) and opened the
      // editor (queuing a focus rAF). Re-stub for the new DOM (description now
      // first, still 600 wide) and drain both.
      setupTwoBoxRects({ description: 600, title: 60, tags: 60 });
      flushRafs();

      // Invariant (a) — STRUCTURAL: the button is a sibling of the track, not
      // inside it, and the overflow loop never hides the button. This is the
      // half jsdom CAN check: that the JS keeps the button out of the chips'
      // clipping box. The PIXEL guarantee (button stays in-bounds because
      // flex:0 0 auto claims its width first) is a CSS layout property jsdom
      // has no engine for — it is verified in a real browser by the e2e test
      // "over-wide promoted chip keeps the more-menu button reachable" in
      // browser geometry tests. Do NOT assert btn.getBoundingClientRect()
      // here: jsdom returns all-zeros for un-stubbed rects, so it would be a
      // `0 <= BAR_RIGHT` tautology that passes even with the fix removed.
      const btnAfter = container.querySelector('.chip-unified-btn') as HTMLElement;
      expect(btnAfter).toBeTruthy();
      expect(btnAfter.style.display).not.toBe('none');     // button never demoted
      const track = container.querySelector('.chip-track') as HTMLElement;
      expect(track).toBeTruthy();
      expect(btnAfter.parentElement).toBe(container);       // sibling of bar…
      expect(track.contains(btnAfter)).toBe(false);         // …NOT inside the clipping track
      expect(track.contains(container.querySelector('.chip'))).toBe(true); // chips ARE in the track

      // Invariant (b): whole-chip granularity — every chip is EITHER fully
      // shown (right edge within the track) OR display:none. No chip is left
      // straddling the track edge (partially clipped).
      container.querySelectorAll('.chip').forEach((el) => {
        const chip = el as HTMLElement;
        if (chip.style.display === 'none') return; // fully hidden — fine
        expect(chip.getBoundingClientRect().right).toBeLessThanOrEqual(TRACK_RIGHT);
      });

      vi.unstubAllGlobals();
      vi.stubGlobal('ResizeObserver', MockResizeObserver);
      renderer.destroy();
    });

    test('search filters add-property list', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10, group: 'This Page' },
        draft: { type: 'boolean', widget: 'checkbox', priority: 92, group: 'This Page' },
        breadcrumb: { type: 'boolean', widget: 'checkbox', priority: 86, group: 'This Page' },
        footer: { type: 'boolean', widget: 'checkbox', priority: 86, group: 'Whole Site' },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const btn = container.querySelector('.chip-unified-btn') as HTMLElement;
      btn.click();

      const menu = document.querySelector('.omenu') as HTMLElement;
      const search = menu.querySelector('.omenu-search') as HTMLInputElement;

      // Initially all 3 unset fields visible
      let addRows = menu.querySelectorAll('.orow-add');
      expect(addRows.length).toBe(3); // draft, breadcrumb, footer

      // Type "bread" in search
      search.value = 'bread';
      search.dispatchEvent(new Event('input'));

      addRows = menu.querySelectorAll('.orow-add');
      expect(addRows.length).toBe(1);

      renderer.destroy();
    });

    // The "+"
    // menu had no keyboard path or ARIA roles at all before this — a menu row
    // whose name doesn't already say what it does gets a second line.
    describe('ARIA roles, detail lines, and keyboard nav', () => {
      // The menu's keydown listener sits on `document` in the capture phase
      // (ctx-menu.ts's placement), so a test dispatches at the document too.
      const key = (k: string) =>
        document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));

      function renderBar() {
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10, group: 'Common' },
          nav: { type: 'boolean', widget: 'checkbox', priority: 20, group: 'Common' },
          breadcrumb: { type: 'boolean', widget: 'checkbox', priority: 30, group: 'Common' },
        });
        const { store } = makeStore({ title: 'Hello' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);
        (container.querySelector('.chip-unified-btn') as HTMLElement).click();
        const menu = document.querySelector('.omenu') as HTMLElement;
        return { renderer, menu };
      }

      test('the search box is a combobox owning the add list, whose rows are options', () => {
        // NOT role="menu": a menu's children must be menuitem/group/separator
        // and this surface's first child is a textbox. WAI-ARIA's
        // combobox-with-listbox-popup is the shape that admits one.
        const { renderer, menu } = renderBar();
        expect(menu.getAttribute('role')).toBeNull();
        const search = menu.querySelector('.omenu-search') as HTMLInputElement;
        expect(search.getAttribute('role')).toBe('combobox');
        const listbox = document.getElementById(search.getAttribute('aria-controls')!)!;
        expect(listbox.getAttribute('role')).toBe('listbox');
        const rows = Array.from(listbox.querySelectorAll('.orow-add'));
        expect(rows.length).toBeGreaterThan(0);
        expect(rows.every(r => r.getAttribute('role') === 'option')).toBe(true);
        renderer.destroy();
      });

      test('an add row shows a detail line only when chip.<key>.detail exists', () => {
        const { renderer, menu } = renderBar();
        // 'breadcrumb' has a detail key; 'nav' is skipped as self-explanatory
        // (see the skip list in ui-strings.ts) and gets none.
        const breadcrumbRow = Array.from(menu.querySelectorAll<HTMLElement>('.orow-add'))
          .find(r => r.textContent?.includes('Breadcrumb'))!;
        const navRow = Array.from(menu.querySelectorAll<HTMLElement>('.orow-add'))
          .find(r => r.textContent?.includes('Show in Nav'))!;
        expect(breadcrumbRow.querySelector('.orow-add-detail')).not.toBeNull();
        expect(breadcrumbRow.querySelector('.orow-add-detail')?.textContent)
          .toBe('Override the site-wide breadcrumb');
        expect(navRow.querySelector('.orow-add-detail')).toBeNull();
        renderer.destroy();
      });

      test('arrows move an active-descendant highlight while typing keeps working', () => {
        const { renderer, menu } = renderBar();
        const search = menu.querySelector('.omenu-search') as HTMLInputElement;
        search.focus();
        const rows = Array.from(menu.querySelectorAll<HTMLElement>('.orow-add'));
        // The first match is active on open, so Enter adds it without an arrow.
        expect(rows[0].classList.contains('orow-active')).toBe(true);
        expect(search.getAttribute('aria-activedescendant')).toBe(rows[0].id);

        key('ArrowDown');
        expect(rows[1].classList.contains('orow-active')).toBe(true);
        expect(rows[0].classList.contains('orow-active')).toBe(false);
        expect(search.getAttribute('aria-activedescendant')).toBe(rows[1].id);
        // Focus never left the text box — that is what keeps typing filtering.
        expect(document.activeElement).toBe(search);

        key('ArrowUp');
        expect(rows[0].classList.contains('orow-active')).toBe(true);
        renderer.destroy();
      });

      test('typing rebuilds the list and re-activates its first match', () => {
        const { renderer, menu } = renderBar();
        const search = menu.querySelector('.omenu-search') as HTMLInputElement;
        search.focus();
        key('ArrowDown');
        search.value = 'bread';
        search.dispatchEvent(new Event('input', { bubbles: true }));

        const rows = Array.from(menu.querySelectorAll<HTMLElement>('.orow-add'));
        expect(rows).toHaveLength(1);
        expect(rows[0].classList.contains('orow-active')).toBe(true);
        expect(search.getAttribute('aria-activedescendant')).toBe(rows[0].id);
        renderer.destroy();
      });

      test('Enter activates the highlighted row (begins a transactional add)', () => {
        const { renderer, menu } = renderBar();
        const search = menu.querySelector('.omenu-search') as HTMLInputElement;
        search.value = 'bread';
        search.dispatchEvent(new Event('input', { bubbles: true }));
        key('Enter');
        expect(container.querySelector('.chip--provisional[data-key="breadcrumb"]')).toBeTruthy();
        renderer.destroy();
      });

      test('Escape closes the menu and hands focus back to the "+" button', () => {
        const { renderer } = renderBar();
        const plus = container.querySelector('.chip-unified-btn') as HTMLElement;
        expect(document.querySelector('.omenu')).not.toBeNull();
        key('Escape');
        expect(document.querySelector('.omenu')).toBeNull();
        expect(document.activeElement).toBe(plus);
        renderer.destroy();
      });

      test('Tab closes the menu — neither a menu nor a combobox popup holds tab stops', () => {
        const { renderer } = renderBar();
        key('Tab');
        expect(document.querySelector('.omenu')).toBeNull();
        renderer.destroy();
      });

      test('the keyboard path survives a menu with no search box at all', () => {
        // Every schema field already set: no unset fields, so no search input
        // and nothing focusable inside the overlay. The old root-scoped
        // keydown listener could never fire here — Escape did nothing.
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10, group: 'Common' },
        });
        const { store } = makeStore({ title: 'Hello' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);
        (container.querySelector('.chip-unified-btn') as HTMLElement).click();
        const menu = document.querySelector('.omenu') as HTMLElement;
        expect(menu.querySelector('.omenu-search')).toBeNull();

        key('Escape');
        expect(document.querySelector('.omenu')).toBeNull();
        renderer.destroy();
      });

      test('Escape closing the menu does not discard a provisional chip', async () => {
        // The document-level Escape listener armProvisionalDismiss arms for a
        // checkbox ghost is the same key this menu consumes. Without
        // stopPropagation the menu's Escape reached it too, so closing the
        // menu also threw away the field the user was midway through adding.
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10, group: 'Common' },
          draft: { type: 'boolean', widget: 'checkbox', priority: 60, group: 'Common' },
          breadcrumb: { type: 'boolean', widget: 'checkbox', priority: 30, group: 'Common' },
        });
        const { store } = makeStore({ title: 'Hello' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        const openMenu = () => (container.querySelector('.chip-unified-btn') as HTMLElement).click();
        openMenu();
        (Array.from(document.querySelectorAll<HTMLElement>('.omenu .orow-add'))
          .find(r => r.textContent?.includes('Draft')) as HTMLElement).click();
        // The ghost's own document-level dismiss listeners arm on the next
        // tick (so the creating click can't self-dismiss) — without this wait
        // the assertion below would hold for the wrong reason.
        await new Promise(r => setTimeout(r, 0));
        expect(container.querySelector('.chip--provisional[data-key="draft"]')).toBeTruthy();

        openMenu();
        key('Escape');

        expect(document.querySelector('.omenu')).toBeNull();
        expect(container.querySelector('.chip--provisional[data-key="draft"]')).toBeTruthy();
        renderer.destroy();
      });
    });
  });

  // ── Overlay lifecycle (added with the openOverlay extraction) ──────
  // These tests guard the most-touched code paths in the chip-bar refactor:
  // the discriminated activeOverlay state and the openOverlay primitive's
  // outside-click + close machinery.

  describe('right-click context menu', () => {
    test('opens at cursor position with Remove field entry', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      chip.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: 80, clientY: 40,
      }));

      const menu = document.body.querySelector('.ctx-menu') as HTMLElement;
      expect(menu).not.toBeNull();
      expect(menu.style.position).toBe('fixed');
      expect(menu.style.left).toBe('80px');
      expect(menu.style.top).toBe('40px');

      const remove = menu.querySelector('.ctx-item.danger') as HTMLElement;
      expect(remove?.textContent).toBe('Remove field');

      // "Copy value" sits above the danger item; "Reset to
      // default" is hidden because this schema declares no default for title.
      const labels = Array.from(menu.querySelectorAll('.ctx-item'))
        .map(el => el.querySelector('.ctx-label')?.textContent ?? el.textContent);
      expect(labels).toEqual(['Copy value', 'Remove field']);

      renderer.destroy();
    });

    test('"Copy value" copies a string scalar and the tags flow form', async () => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.assign(navigator, { clipboard: { writeText } });
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
      });
      const { store } = makeStore({ title: 'Hello', tags: ['a', 'b'] });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const copyVia = (key: string) => {
        const chip = container.querySelector(`.chip[data-key="${key}"]`) as HTMLElement;
        chip.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
        const item = Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu .ctx-item'))
          .find(el => el.textContent === 'Copy value')!;
        item.click();
      };

      copyVia('title');
      expect(writeText).toHaveBeenLastCalledWith('Hello');
      copyVia('tags');
      expect(writeText).toHaveBeenLastCalledWith('a, b');

      renderer.destroy();
    });

    test('"Reset to default" shows only for fields with a schema default and commits it', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        lang: { type: 'string', widget: 'text-input', priority: 20, default: 'en' },
      });
      const { store } = makeStore({ title: 'Hello', lang: 'fr' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      // No default declared → no reset item.
      const titleChip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      titleChip.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
      expect(Array.from(document.querySelectorAll('.ctx-menu .ctx-item'))
        .some(el => el.textContent === 'Reset to default')).toBe(false);

      // Declared default → item shown; clicking commits the schema default.
      const langChip = container.querySelector('.chip[data-key="lang"]') as HTMLElement;
      langChip.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
      const reset = Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu .ctx-item'))
        .find(el => el.textContent === 'Reset to default');
      expect(reset).toBeTruthy();
      reset!.click();
      expect(renderer.getValues().lang).toBe('en');

      renderer.destroy();
    });

    test('a provisional ghost gets only "Remove field", which dismisses without a write', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        author: { type: 'string', widget: 'text-input', priority: 30 },
      });
      const { store, write } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      // Begin a transactional add: ghost chip + edit popover, nothing written.
      (container.querySelector('.chip-unified-btn') as HTMLElement).click();
      const addRow = Array.from(document.querySelectorAll<HTMLElement>('.omenu .orow-add'))
        .find(el => el.textContent?.includes('Author') || el.textContent?.includes('author'))!;
      addRow.click();
      const ghost = container.querySelector('.chip--provisional[data-key="author"]') as HTMLElement;
      expect(ghost).toBeTruthy();

      // Right-click the ghost: closing its popover to show the menu must NOT
      // dismiss it (retain-on-close), and the menu is dismissal-only.
      ghost.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
      expect(container.querySelector('.chip--provisional[data-key="author"]')).toBeTruthy();
      const items = Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu .ctx-item'));
      expect(items.map(el => el.querySelector('.ctx-label')?.textContent)).toEqual(['Remove field']);

      // "Remove field" on a ghost = dismiss: the ghost goes, no store entry,
      // and the file was never written.
      items[0].click();
      expect(container.querySelector('.chip--provisional')).toBeNull();
      expect(container.querySelector('.chip[data-key="author"]')).toBeNull();
      expect(renderer.getValues().author).toBeUndefined();
      expect(write).not.toHaveBeenCalled();

      renderer.destroy();
    });

    test('Remove field click deletes the field and re-renders chips', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
      });
      const { store, write } = makeStore({ title: 'Hello', draft: true });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      expect(container.querySelectorAll('.chip').length).toBe(2);

      const draftChip = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
      draftChip.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: 0, clientY: 0,
      }));
      const remove = document.body.querySelector('.ctx-menu .ctx-item.danger') as HTMLElement;
      remove.click();

      // removeField triggers a debounced write.
      // Check the field is removed from the store (getValues).
      expect(renderer.getValues().draft).toBeUndefined();
      expect(container.querySelector('.chip[data-key="draft"]')).toBeNull();
      expect(document.body.querySelector('.ctx-menu')).toBeNull();

      renderer.destroy();
    });

    test('outside mousedown closes the menu', async () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hi' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      chip.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: 10, clientY: 10,
      }));
      expect(document.body.querySelector('.ctx-menu')).not.toBeNull();

      // openOverlay defers outside-click registration via setTimeout(0) so the
      // opening click doesn't immediately self-close — wait one tick.
      await new Promise(r => setTimeout(r, 0));

      // Mousedown on the body outside both the chip and the menu.
      const outside = document.createElement('div');
      document.body.appendChild(outside);
      outside.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));

      expect(document.body.querySelector('.ctx-menu')).toBeNull();

      renderer.destroy();
    });
  });

  describe('overlay toggle and switch', () => {
    test('clicking the same chip twice toggles the popover closed', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hi' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      chip.click();
      expect(document.body.querySelector('.pop')).not.toBeNull();

      chip.click();
      expect(document.body.querySelector('.pop')).toBeNull();
      // The chip's .on highlight is also gone.
      expect(chip.classList.contains('on')).toBe(false);

      renderer.destroy();
    });

    test('clicking chip B while chip A popover is open switches the popover', () => {
      // Exercises the guarded toggle:
      //   if (activeOverlay.kind === 'edit' && activeOverlay.key === key) toggle;
      //   else close-then-open.
      // A regression here would leave both popovers stacked, or fail to repaint
      // .on on the new chip.
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
      });
      const { store } = makeStore({ title: 'Hi', description: 'desc' });
      const renderer = new ChipBar(
        schema,
        store,
      );
      renderer.render(container);

      const chipA = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      const chipB = container.querySelector('.chip[data-key="description"]') as HTMLElement;

      chipA.click();
      expect(document.body.querySelectorAll('.pop').length).toBe(1);
      expect(chipA.classList.contains('on')).toBe(true);

      chipB.click();
      // Exactly one popover, anchored to the new chip.
      expect(document.body.querySelectorAll('.pop').length).toBe(1);
      expect(document.body.querySelector('.pop .pop-title')?.textContent).toBe('Description');
      expect(chipA.classList.contains('on')).toBe(false);
      expect(chipB.classList.contains('on')).toBe(true);

      renderer.destroy();
    });
  });
});

// Minimal schema with only `title` as a user-facing field. internal_fields
// mirrors the skip_schema entries in BUILTIN_FIELDS — the chip bar filters
// these out of the chip render list (but getValues() still returns them).
const MIN_SCHEMA: ContentSchema = {
  generator: 'moss',
  version: '1.0',
  frontmatter: {
    fields: {
      title: { type: 'string', widget: 'text-input', description: 'Page title' },
    },
    internal_fields: ['uid', 'layout', 'cover_type', 'analytics', 'children_source'],
  },
  shortcodes: { delimiters: [':::', ':::'], definitions: {} },
} as ContentSchema;

describe('ChipBar internal-field denylist', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('uid is not rendered even when present in frontmatter', () => {
    const values = { title: 'My Page', uid: 'abc123ef' };
    const { store } = makeStore(values);
    const fr = new ChipBar(MIN_SCHEMA, store);
    fr.render(container);
    const chipKeys = Array.from(container.querySelectorAll('.chip')).map(
      (el) => (el as HTMLElement).dataset.key,
    );
    expect(chipKeys).toContain('title');
    expect(chipKeys).not.toContain('uid');
    fr.destroy();
  });

  test('all skip_schema fields are hidden: layout, cover_type, analytics, children_source', () => {
    const values = {
      title: 'Hi',
      layout: 'article',
      cover_type: 'image',
      analytics: { gid: 'x' },
      children_source: '[[News]]',
    };
    const { store } = makeStore(values);
    const fr = new ChipBar(MIN_SCHEMA, store);
    fr.render(container);
    const chipKeys = Array.from(container.querySelectorAll('.chip')).map(
      (el) => (el as HTMLElement).dataset.key,
    );
    for (const k of ['layout', 'cover_type', 'analytics', 'children_source']) {
      expect(chipKeys).not.toContain(k);
    }
    expect(chipKeys).toContain('title');
    fr.destroy();
  });

  test('values are preserved on getValues (not deleted, only hidden from UI)', () => {
    const values = {
      title: 'Hi',
      uid: 'abc123ef',
      layout: 'article',
      cover_type: 'image',
      analytics: { gid: 'x' },
      children_source: '[[News]]',
    };
    const { store } = makeStore(values);
    const fr = new ChipBar(MIN_SCHEMA, store);
    fr.render(container);
    const out = fr.getValues();
    expect(out.uid).toBe('abc123ef');
    expect(out.layout).toBe('article');
    expect(out.cover_type).toBe('image');
    expect((out.analytics as any).gid).toBe('x');
    expect(out.children_source).toBe('[[News]]');
    fr.destroy();
  });

  describe('setValues() — external-update semantics', () => {
    const SCHEMA = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
    });

    test('updates values and re-renders chips with new values', () => {
      const { store } = makeStore({ title: 'Old' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);
      expect(container.querySelector('.chip[data-key="title"] .chip-key')?.textContent).toBeTruthy();

      fr.setValues({ title: 'New' });

      expect(fr.getValues()).toEqual({ title: 'New' });
      // Still only one chip for title (now showing "New")
      const chips = container.querySelectorAll('.chip[data-key="title"]');
      expect(chips.length).toBe(1);
      fr.destroy();
    });

    test('removes chips for fields no longer present in values', () => {
      const { store } = makeStore({ title: 'Old', draft: true });
      const fr = new ChipBar(
        SCHEMA,
        store,
      );
      fr.render(container);
      expect(container.querySelectorAll('.chip').length).toBe(2);

      fr.setValues({ title: 'Kept' });

      expect(container.querySelectorAll('.chip[data-key="draft"]').length).toBe(0);
      expect(container.querySelectorAll('.chip[data-key="title"]').length).toBe(1);
      fr.destroy();
    });

    test('adds chips for newly-present fields', () => {
      const { store } = makeStore({ title: 'Hi' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);
      expect(container.querySelectorAll('.chip').length).toBe(1);

      fr.setValues({ title: 'Hi', draft: true });

      expect(container.querySelectorAll('.chip[data-key="draft"]').length).toBe(1);
      fr.destroy();
    });

    test('does not invoke write fn — external updates are silent (no new write scheduled)', () => {
      // setValues delegates to store.onFileChanged which reconciles values but
      // does NOT schedule a disk write (the values came FROM disk). The write fn
      // must not be called on an external update.
      const { store, write } = makeStore({ title: 'Old' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);
      write.mockClear();

      fr.setValues({ title: 'New' });

      expect(write).not.toHaveBeenCalled();
      fr.destroy();
    });

    test('after the popover closes, chips reflect the external-update values', async () => {
      // Complement to the popover-preservation test: once the user closes the
      // popover, the stale values shown in the chip bar must be replaced with
      // the external update that arrived while the popover was open. Otherwise
      // the user sees pre-update values until they make a local edit.
      const { store } = makeStore({ title: 'Old' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);

      // Open popover (outside-click listener is registered via setTimeout(0))
      const titleChip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      titleChip.click();
      expect(document.body.querySelector('.pop')).toBeTruthy();
      await new Promise(r => setTimeout(r, 0));

      // External update arrives while popover is open
      fr.setValues({ title: 'Remote change' });

      // Close popover via the × button in the popover header
      const closeX = document.body.querySelector('.pop-x') as HTMLElement;
      closeX.click();

      // Popover is gone and chip bar now shows the remote value, not stale 'Old'
      expect(document.body.querySelector('.pop')).toBeNull();
      const chipValue = container.querySelector('.chip[data-key="title"] span:last-child')?.textContent;
      expect(chipValue).toBe('Remote change');
      fr.destroy();
    });

    test('equality short-circuit: setValues with identical values is a no-op', () => {
      // A self-induced echo (chip edit → save → file-changed → setValues)
      // must not fight the user. setValues must be a no-op then — otherwise the
      // chip behind an open popover gets re-rendered (closing the popover) on
      // every auto-save echo.
      const { store } = makeStore({ title: 'Hello', draft: false });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);

      // Open popover so re-rendering would be observable (closePop fires).
      const titleChip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      titleChip.click();
      expect(document.body.querySelector('.pop')).toBeTruthy();

      // Identical values — must not re-render or close the popover.
      fr.setValues({ title: 'Hello', draft: false });

      expect(document.body.querySelector('.pop')).toBeTruthy();
      fr.destroy();
    });

    test('equality short-circuit: tag arrays compared element-wise', () => {
      const SCHEMA_TAGS = makeSchema({
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
      });
      const { store } = makeStore({ tags: ['a', 'b'] });
      const fr = new ChipBar(SCHEMA_TAGS, store);
      fr.render(container);

      const tagsChip = container.querySelector('.chip[data-key="tags"]') as HTMLElement;
      tagsChip.click();
      expect(document.body.querySelector('.pop')).toBeTruthy();

      // Different array identity, same contents — must short-circuit.
      fr.setValues({ tags: ['a', 'b'] });
      expect(document.body.querySelector('.pop')).toBeTruthy();

      // Genuinely different — must NOT short-circuit (popover deferral
      // protects the user's input but underlying values still update).
      fr.setValues({ tags: ['a', 'c'] });
      expect(fr.getValues().tags).toEqual(['a', 'c']);
      fr.destroy();
    });

    test('text-input edits live-paint the chip behind the popover (mirror of CM6 same-buffer behavior)', () => {
      // Bug: when the popover was open the chip behind it stayed at the
      // pre-edit value until the user closed the popover. Each keystroke now
      // repaints that chip in place, the same way CM6 edits are immediately
      // visible because the editor surface and the doc are the same buffer.
      const { store } = makeStore({ title: 'Old' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);

      const titleChip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      titleChip.click();
      const inp = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
      expect(inp).toBeTruthy();

      inp.value = 'Updated mid-edit';
      inp.dispatchEvent(new Event('input', { bubbles: true }));

      // Chip behind the popover must reflect the typed value WITHOUT closing
      // the popover (which would destroy the user's input).
      expect(document.body.querySelector('.pop')).toBeTruthy();
      const chipValueSpan = container.querySelector('.chip[data-key="title"] span:last-child') as HTMLElement;
      expect(chipValueSpan.textContent).toBe('Updated mid-edit');
      fr.destroy();
    });

    test('textarea edits live-paint the chip (description field — the bug from the screenshot)', () => {
      const SCHEMA_DESC = makeSchema({
        description: { type: 'string', widget: 'text-area', priority: 20 },
      });
      const { store } = makeStore({ description: 'old' });
      const fr = new ChipBar(SCHEMA_DESC, store);
      fr.render(container);

      const chip = container.querySelector('.chip[data-key="description"]') as HTMLElement;
      chip.click();
      const ta = document.body.querySelector('.pop textarea.pop-input') as HTMLTextAreaElement;
      expect(ta).toBeTruthy();

      ta.value = '这周末的纪实戏剧演出';
      ta.dispatchEvent(new Event('input', { bubbles: true }));

      const chipValue = container.querySelector('.chip[data-key="description"] span:last-child')?.textContent;
      expect(chipValue).toBe('这周末的纪实戏剧演出');
      // Popover stays open so the user can keep typing.
      expect(document.body.querySelector('.pop')).toBeTruthy();
      fr.destroy();
    });

    test('boolean toggle does not call commitFieldEdit (renderChips handles repaint)', () => {
      // Boolean chips toggle inline via store.setField (immediate). The store
      // subscriber fires renderChips() which repaints the chip.
      const { store, write } = makeStore({ draft: false });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);

      const chip = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
      expect(chip.classList.contains('bool-off')).toBe(true);
      chip.click();
      // Write fn is called (immediate toggle write).
      expect(write).toHaveBeenCalled();
      expect(fr.getValues().draft).toBe(true);
      // After toggle, chip is fully re-rendered (new node) — query again.
      const after = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
      expect(after.classList.contains('bool-on')).toBe(true);
      fr.destroy();
    });

    test('preserves an open popover and the text in it', () => {
      // If the user has opened a popover on a chip and is typing, we must not
      // blow away the popover via re-render — the user's in-progress input
      // would vanish. External updates defer their DOM refresh until the
      // popover closes (the underlying values ARE updated immediately so
      // getValues() returns fresh data).
      const { store } = makeStore({ title: 'Old' });
      const fr = new ChipBar(SCHEMA, store);
      fr.render(container);

      // Simulate user clicking the title chip — opens popover with <input>.
      const titleChip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      expect(titleChip).toBeTruthy();
      titleChip.click();

      const popoverInput = document.body.querySelector('.pop .pop-input') as HTMLInputElement;
      expect(popoverInput).toBeTruthy();
      popoverInput.focus();
      popoverInput.value = 'User typed this';

      // Now an external update arrives (e.g., git pulled a change to frontmatter)
      fr.setValues({ title: 'Remote change' });

      // Internal state reflects the update
      expect(fr.getValues()).toEqual({ title: 'Remote change' });
      // Popover is still open and the user's input is preserved
      expect(document.body.querySelector('.pop')).toBeTruthy();
      expect((document.body.querySelector('.pop .pop-input') as HTMLInputElement).value).toBe('User typed this');
      fr.destroy();
    });
  });

  // ── Date chip display ────────────────────────────────────────────────
  // Helper: render a date chip and return its displayed text. Uses `container`
  // from the outer describe's beforeEach — afterEach clears body.innerHTML so
  // each test gets a clean DOM.
  function renderDateChipText(dateValue: string, locale: 'en' | 'zh-hans' | 'zh-hant'): string {
    setLocale(locale);
    const schema = makeSchema({
      date: { type: 'string', widget: 'date-picker', priority: 30 },
    });
    const { store } = makeStore({ date: dateValue });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);
    const chip = container.querySelector('[data-key="date"]') as HTMLElement;
    const valueEl = chip.querySelector('.chip-value') as HTMLElement;
    const text = valueEl.textContent ?? '';
    renderer.destroy();
    return text;
  }

  describe('date chip rendering', () => {
    afterEach(() => setLocale('en'));

    test('renders date-only in English locale (no time component)', () => {
      const text = renderDateChipText('2026-01-15', 'en');
      // dateStyle:'medium' in 'en' locale renders as "Jan 15, 2026"
      expect(text).toContain('2026');
      expect(text).toContain('15');
      // Must not contain a colon (time separator)
      expect(text).not.toMatch(/\d:\d/);
    });

    test('renders date-only in Simplified Chinese locale', () => {
      const text = renderDateChipText('2026-01-15', 'zh-hans');
      // zh-Hans dateStyle:'medium' renders something like "2026年1月15日"
      expect(text).toContain('2026');
      expect(text).toContain('15');
      expect(text).toContain('年');
      // Must not contain a colon (time separator)
      expect(text).not.toMatch(/\d:\d/);
    });

    test('renders date-only in Traditional Chinese locale', () => {
      const text = renderDateChipText('2026-01-15', 'zh-hant');
      expect(text).toContain('2026');
      expect(text).toContain('15');
      // Must not contain a colon (time separator)
      expect(text).not.toMatch(/\d:\d/);
    });

    test('UTC timezone prevents negative-offset off-by-one: a date stays Jan 1', () => {
      // Without timeZone:'UTC', a date-only ISO string shifts back a day in UTC-8.
      // With timeZone:'UTC' the chip must show January 1, not December 31.
      const text = renderDateChipText('2026-01-01', 'en');
      // dateStyle:'medium' in 'en' renders "Jan 1, 2026" — must contain year and not Dec
      expect(text).toContain('2026');
      // The day shown must not be 31 (Dec 31 from timezone rollback)
      expect(text).not.toMatch(/31/);
      expect(text).not.toContain('Dec');
    });

    test('shows the Empty label when the date value is an empty string', () => {
      const schema = makeSchema({
        date: { type: 'string', widget: 'date-picker', priority: 30 },
      });
      // Legacy `date: ""` in the file — renders the explicit Empty chip.
      const { store } = makeStore({ date: '' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);
      const valueEl = container.querySelector('[data-key="date"] .chip-value') as HTMLElement;
      expect(valueEl.querySelector('.chip-value-empty')).toBeTruthy();
      expect(valueEl.textContent).not.toMatch(/\d{4}/); // no year shown
      renderer.destroy();
    });

    test('falls back to raw string for malformed date values', () => {
      const text = renderDateChipText('not-a-date', 'en');
      // Invalid ISO date: isNaN check → show raw value unchanged
      expect(text).toBe('not-a-date');
    });
  });

  // ── Date chip default value ──────────────────────────────────────────
  // defaultValueForWidget('date-picker') returns today's LOCAL calendar day as
  // YYYY-MM-DD (NOT UTC — so it matches the user's timezone). We invoke the real
  // (private) method rather than re-implementing the expression.
  describe('date chip default value when adding a field', () => {
    function dateDefault(): string {
      const schema = makeSchema({ date: { type: 'string', widget: 'date-picker', priority: 30 } });
      const { store } = makeStore({});
      const renderer = new ChipBar(schema, store);
      const v = (renderer as unknown as { defaultValueForWidget(w: string, d: unknown): unknown })
        .defaultValueForWidget('date-picker', null);
      renderer.destroy();
      return String(v);
    }

    test('date-picker default is today as local YYYY-MM-DD (date-only, no time)', () => {
      const value = dateDefault();
      expect(value).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(value).not.toContain('T');
      expect(value.length).toBe(10);
      // Must equal today's LOCAL date (not UTC), matching the user's timezone.
      const now = new Date();
      const expected = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
      expect(value).toBe(expected);
    });

    test('date chip with today value renders current year (display path smoke test)', () => {
      const today = dateDefault();
      const currentYear = String(new Date().getFullYear());
      const text = renderDateChipText(today, 'en');
      expect(text).toContain(currentYear);
      expect(text).not.toMatch(/\d:\d/); // no time component in display
    });
  });

  // ── Date chip edit writes YYYY-MM-DD ────────────────────────────────
  describe('date chip edit writes date-only YYYY-MM-DD', () => {
    test('WIDGET_EDITORS date-picker parse returns raw YYYY-MM-DD string', () => {
      // The date-picker editor uses <input type="date"> whose .value is always
      // YYYY-MM-DD. The parse function returns that string as-is. Verify this
      // by checking the stored value after a simulated edit through setField.
      const schema = makeSchema({
        date: { type: 'string', widget: 'date-picker', priority: 30 },
      });
      const { store } = makeStore({ date: '2026-01-01' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      // Simulate what happens when the popover commits: setField with new date
      store.setField('date', '2026-06-15');
      const value = renderer.getValues().date as string;

      expect(value).toBe('2026-06-15');
      expect(value).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(value).not.toContain('T');

      renderer.destroy();
    });
  });

  // ── Preview click → reveal, never edit ─────────────────────────────
  //
  // The preview is a view, not a control surface. Clicking the byline there
  // asks "where does this live?" — it must point at the chip and stop. The
  // old behaviour synthesised a click on the chip, which opened its popover
  // and pulled focus into the editor webview; a drag-select over the byline
  // therefore collapsed on every selectionchange and preview text could not
  // be selected at all.
  describe('revealField', () => {
    beforeEach(() => {
      // jsdom implements neither of these; the real WebKit webview does.
      const g = globalThis as unknown as { CSS?: { escape(s: string): string } };
      if (typeof g.CSS === 'undefined') {
        g.CSS = { escape: (x: string) => x.replace(/["\\\]]/g, '\\$&') };
      }
      Element.prototype.scrollIntoView = vi.fn();
    });

    function renderOverflowing() {
      const rafCallbacks: (() => void)[] = [];
      vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
        rafCallbacks.push(cb);
        return rafCallbacks.length;
      });
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        description: { type: 'string', widget: 'text-area', priority: 20 },
        tags: { type: 'array', widget: 'tag-input', priority: 50 },
        byline: { type: 'string', widget: 'text-input', priority: 60 },
      });
      const { store } = makeStore({
        title: 'My Post', description: 'desc', tags: ['a'], byline: 'Ada',
      });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      // Narrow bar: byline (priority 60) is the first to fall into the "+N" menu.
      container.getBoundingClientRect = () =>
        ({ right: 300, left: 0, top: 0, bottom: 0, width: 300, height: 30 } as DOMRect);
      const chips0 = container.querySelectorAll('.chip');
      [80, 160, 240, 290].forEach((right, i) => {
        (chips0[i] as HTMLElement).getBoundingClientRect = () =>
          ({ right, left: right - 60, top: 0, bottom: 0, width: 60, height: 32 } as DOMRect);
      });
      rafCallbacks[0]();
      return { renderer, rafCallbacks };
    }

    test('promotes a chip out of the overflow menu so it can be seen', () => {
      const { renderer } = renderOverflowing();

      const before = container.querySelector('.chip[data-key="byline"]') as HTMLElement;
      expect(before.style.display).toBe('none');

      renderer.revealField('byline');

      // Pointing at something invisible is not pointing: the chip moves to the
      // front of the strip, using the same promotion the more-menu uses.
      const chips = container.querySelectorAll('.chip');
      expect((chips[0] as HTMLElement).dataset.key).toBe('byline');

      vi.unstubAllGlobals();
      vi.stubGlobal('ResizeObserver', MockResizeObserver);
      renderer.destroy();
    });

    test('opens no editor and moves no focus', () => {
      const { renderer } = renderOverflowing();

      renderer.revealField('byline');

      // A popover here would be an edit the user did not ask for, and its
      // autofocus is what collapsed the preview selection.
      expect(document.body.querySelector('.pop')).toBeNull();
      expect(document.activeElement).toBe(document.body);

      vi.unstubAllGlobals();
      vi.stubGlobal('ResizeObserver', MockResizeObserver);
      renderer.destroy();
    });

    test('washes the chip so the eye lands on it', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      renderer.revealField('title');

      const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      expect(chip.classList.contains('moss-revealed')).toBe(true);
      // Not the just-committed green: nothing was committed.
      expect(chip.classList.contains('moss-just-committed')).toBe(false);

      renderer.destroy();
    });

    // flashChip is the OTHER signal on the same surface, and the distinction
    // is the point: green means "moss wrote your value", the wash means "it
    // lives here". Asserted together so a future edit cannot quietly merge
    // them. (Subsumes the three prototype-call cases in the deleted
    // chip-bar-flash.test.ts.)
    test('flashChip marks the chip whose value just landed, and only that one', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
        date: { type: 'string', widget: 'date-picker', priority: 30 },
      });
      const { store } = makeStore({ title: 'Hello', date: '2026-01-01' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      renderer.flashChip('title');

      const title = container.querySelector('.chip[data-key="title"]') as HTMLElement;
      const date = container.querySelector('.chip[data-key="date"]') as HTMLElement;
      expect(title.classList.contains('moss-just-committed')).toBe(true);
      expect(title.classList.contains('moss-revealed')).toBe(false);
      expect(date.classList.contains('moss-just-committed')).toBe(false);

      expect(() => renderer.flashChip('colophon')).not.toThrow();

      renderer.destroy();
    });

    test('a field with no chip is a no-op, not a throw', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      expect(() => renderer.revealField('colophon')).not.toThrow();

      renderer.destroy();
    });
  });

  // ── Preview DOUBLE click → open the chip's editor ───────────────────
  //
  // The other strength of the same gesture.
  // A double click commits to the edit, so openField
  // runs the chip's own click interaction — same popover a direct chip click
  // opens. revealField above stays the single-click answer.
  describe('openField', () => {
    beforeEach(() => {
      const g = globalThis as unknown as { CSS?: { escape(s: string): string } };
      if (typeof g.CSS === 'undefined') {
        g.CSS = { escape: (x: string) => x.replace(/["\\\]]/g, '\\$&') };
      }
      Element.prototype.scrollIntoView = vi.fn();
    });

    test('opens the same popover a direct chip click would', async () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      renderer.openField('title');
      // The click is deferred one frame so the programmatic scrollIntoView's
      // scroll event (which closes popovers — Amendment III) lands first.
      await new Promise(requestAnimationFrame);

      expect(document.body.querySelector('.pop')).not.toBeNull();

      renderer.destroy();
    });

    test('a checkbox chip is pointed at, never toggled', () => {
      // A checkbox chip's click IS an edit (it flips the value). A preview
      // double click must not silently change frontmatter, so it degrades to
      // the reveal wash.
      const schema = makeSchema({
        draft: { type: 'boolean', widget: 'checkbox', priority: 10 },
      });
      const { store } = makeStore({ draft: true });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      renderer.openField('draft');

      expect(renderer.getValues().draft).toBe(true);
      expect(document.body.querySelector('.pop')).toBeNull();
      const chip = container.querySelector('.chip[data-key="draft"]') as HTMLElement;
      expect(chip.classList.contains('moss-revealed')).toBe(true);

      renderer.destroy();
    });

    test('a field with no chip is a no-op, not a throw', () => {
      const schema = makeSchema({
        title: { type: 'string', widget: 'text-input', priority: 10 },
      });
      const { store } = makeStore({ title: 'Hello' });
      const renderer = new ChipBar(schema, store);
      renderer.render(container);

      expect(() => renderer.openField('colophon')).not.toThrow();

      renderer.destroy();
    });

    describe('nav settings: a seeded ghost for a field the page never wrote', () => {
      function renderNav() {
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10 },
          nav: { type: 'boolean', widget: 'checkbox', priority: 20 },
        });
        const { store, write } = makeStore({ title: 'Hello' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);
        return { store, write, renderer };
      }

      test('openField(nav, true) shows the ghost on, and one click writes nav: false', () => {
        const { store, write, renderer } = renderNav();
        renderer.openField('nav', true);

        const ghost = container.querySelector('.chip--provisional[data-key="nav"]') as HTMLElement;
        expect(ghost).toBeTruthy();
        expect(ghost.classList.contains('bool-on')).toBe(true);
        expect(store.getValues()).not.toHaveProperty('nav');
        expect(write).not.toHaveBeenCalled();

        ghost.click();
        expect(store.getValues().nav).toBe(false);
        expect(write).toHaveBeenCalledTimes(1);
        renderer.destroy();
      });

      test('openField on an unwritten text field does nothing', () => {
        const schema = makeSchema({
          title: { type: 'string', widget: 'text-input', priority: 10 },
          subtitle: { type: 'string', widget: 'text-input', priority: 30 },
        });
        const { store, write } = makeStore({ title: 'Hello' });
        const renderer = new ChipBar(schema, store);
        renderer.render(container);

        renderer.openField('subtitle');

        expect(container.querySelector('.chip--provisional')).toBeNull();
        expect(store.getValues()).not.toHaveProperty('subtitle');
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });

      test('dismissing the seeded ghost with Escape writes nothing', async () => {
        const { store, write, renderer } = renderNav();
        renderer.openField('nav', true);
        await new Promise(r => setTimeout(r, 0));

        document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

        expect(container.querySelector('.chip[data-key="nav"]')).toBeNull();
        expect(store.getValues()).not.toHaveProperty('nav');
        expect(write).not.toHaveBeenCalled();
        renderer.destroy();
      });
    });
  });

});

// ── Frontmatter hints + hover flash ──

describe('ChipBar field hints (tier 1: tooltips)', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });
  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('a known field carries the schema-sourced hint as a muted portal tooltip', () => {
    const schema = makeSchema({
      description: { type: 'string', widget: 'text-area', priority: 20 },
      draft: { type: 'boolean', widget: 'checkbox', priority: 60 },
    });
    const { store } = makeStore({ description: 'd', draft: true });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    for (const key of ['description', 'draft']) {
      const chip = container.querySelector(`.chip[data-key="${key}"]`) as HTMLElement;
      expect(chip.getAttribute('data-tooltip'), key).toBeTruthy();
      expect(chip.getAttribute('data-tooltip-style'), key).toBe('muted');
      // Portal tooltips only — never the native browser bubble.
      expect(chip.hasAttribute('title'), key).toBe(false);
    }

    renderer.destroy();
  });

  test('an unknown user field has no hint and gets no tooltip', () => {
    const schema = makeSchema({});
    const { store } = makeStore({ my_custom_thing: 'x' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);

    const chip = container.querySelector('.chip[data-key="my_custom_thing"]') as HTMLElement;
    expect(chip.hasAttribute('data-tooltip')).toBe(false);

    renderer.destroy();
  });
});

describe('ChipBar hover flash (tier 2: chip hover → preview)', () => {
  let container: HTMLElement;
  let hoverEvents: Array<string | null>;
  let onHover: (e: Event) => void;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement('div');
    document.body.appendChild(container);
    hoverEvents = [];
    onHover = (e: Event) => {
      hoverEvents.push((e as CustomEvent<{ field: string | null }>).detail.field);
    };
    document.addEventListener('editor:chip-hover', onHover);
  });
  afterEach(() => {
    document.removeEventListener('editor:chip-hover', onHover);
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  function mkBar() {
    const schema = makeSchema({
      title: { type: 'string', widget: 'text-input', priority: 10 },
      date: { type: 'string', widget: 'date-picker', priority: 30 },
    });
    const { store } = makeStore({ title: 'Hi', date: '2026-01-01' });
    const renderer = new ChipBar(schema, store);
    renderer.render(container);
    return renderer;
  }

  function hoverIn(chip: HTMLElement): void {
    chip.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
  }
  function hoverOut(chip: HTMLElement): void {
    chip.dispatchEvent(new MouseEvent('pointerout', { bubbles: true }));
  }

  test('dwelling on a chip announces the field after the intent delay', () => {
    const renderer = mkBar();
    const chip = container.querySelector('.chip[data-key="date"]') as HTMLElement;

    hoverIn(chip);
    expect(hoverEvents).toEqual([]); // not yet — intent delay pending
    vi.advanceTimersByTime(160);
    expect(hoverEvents).toEqual(['date']);

    renderer.destroy();
  });

  test('scrubbing across the bar announces only the chip the pointer settles on', () => {
    const renderer = mkBar();
    const title = container.querySelector('.chip[data-key="title"]') as HTMLElement;
    const date = container.querySelector('.chip[data-key="date"]') as HTMLElement;

    hoverIn(title);
    vi.advanceTimersByTime(50); // leaves before the delay elapses
    hoverOut(title);
    hoverIn(date);
    vi.advanceTimersByTime(160);

    expect(hoverEvents).toEqual(['date']); // title never strobed

    renderer.destroy();
  });

  test('hover-out clears with field: null — but only after an announce', () => {
    const renderer = mkBar();
    const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;

    // Un-announced hover-out: nothing to clear, no event at all.
    hoverIn(chip);
    hoverOut(chip);
    vi.advanceTimersByTime(300);
    expect(hoverEvents).toEqual([]);

    // Announced hover-out: the clearing null follows the announce.
    hoverIn(chip);
    vi.advanceTimersByTime(160);
    hoverOut(chip);
    expect(hoverEvents).toEqual(['title', null]);

    renderer.destroy();
  });

  test('pointer moves within one chip do not re-arm or clear', () => {
    const renderer = mkBar();
    const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;
    const inner = chip.querySelector('.chip-key') as HTMLElement;

    hoverIn(chip);
    vi.advanceTimersByTime(160);
    // Moving from the chip onto its own child: relatedTarget is inside the chip.
    inner.dispatchEvent(new MouseEvent('pointerout', { bubbles: true, relatedTarget: chip }));
    inner.dispatchEvent(new MouseEvent('pointerover', { bubbles: true, relatedTarget: chip }));
    vi.advanceTimersByTime(300);

    expect(hoverEvents).toEqual(['title']); // one announce, no clear, no re-announce

    renderer.destroy();
  });

  test('destroy mid-hover clears the announced flash (file switch)', () => {
    const renderer = mkBar();
    const chip = container.querySelector('.chip[data-key="title"]') as HTMLElement;

    hoverIn(chip);
    vi.advanceTimersByTime(160);
    renderer.destroy();

    expect(hoverEvents).toEqual(['title', null]);
  });
});

// ── "Copy value" serialization ───────────────────────────────────────
// The plain scalar the file holds — a VALUE to paste, never YAML source.

describe('serializeChipValue', () => {
  test('strings copy bare (no YAML quoting)', () => {
    expect(serializeChipValue('Hello: world')).toBe('Hello: world');
  });

  test('arrays copy in the flow form the chip displays', () => {
    expect(serializeChipValue(['a', 'b'])).toBe('a, b');
    expect(serializeChipValue([])).toBe('');
  });

  test('booleans and numbers copy as their YAML scalars', () => {
    expect(serializeChipValue(true)).toBe('true');
    expect(serializeChipValue(false)).toBe('false');
    expect(serializeChipValue(3.5)).toBe('3.5');
  });

  test('the null/empty family copies as empty (absence is the canonical empty)', () => {
    expect(serializeChipValue(null)).toBe('');
    expect(serializeChipValue(undefined)).toBe('');
    expect(serializeChipValue('')).toBe('');
  });
});
