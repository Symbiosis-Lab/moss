import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ChipBar } from '../chip-bar';
import { FrontmatterStore } from '../frontmatter-store';
import { DocumentHistory } from '../document-history';

// jsdom does not implement ResizeObserver; ChipBar.renderChips uses it.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as any).ResizeObserver = StubResizeObserver;
// jsdom implements no scrolling; revealField scrolls the chip into view.
(Element.prototype as any).scrollIntoView = function () {};

const SCHEMA_FIXTURE: any = {
  generator: 'test',
  version: '1',
  shortcodes: {},
  frontmatter: {
    fields: {
      title: {
        name: 'title', widget: 'text-input', type: 'string', priority: 10,
        required: true, description: '', group: 'Common',
      },
      tags: {
        name: 'tags', widget: 'tag-input', type: 'array', priority: 20,
        required: false, description: '', group: 'Common',
      },
      draft: {
        name: 'draft', widget: 'checkbox', type: 'boolean', priority: 30,
        required: false, description: '', group: 'Common',
      },
    },
  },
};

/**
 * WHERE A FRONTMATTER EDIT BECOMES ONE HISTORY STEP.
 *
 * The granularity is the whole point of capturing at the popover rather than at
 * the store. `text-input` widgets commit on every `input` event — only the disk
 * write is debounced — so a store-level hook would put one entry per KEYSTROKE
 * in the log and make ⌘Z strictly worse than the native undo it replaces.
 */
describe('ChipBar → combined history capture', () => {
  let bar: ChipBar;
  let store: FrontmatterStore;
  let edits: { key: string; before: unknown; after: unknown }[];

  const mount = (initial: Record<string, unknown> = {}) => {
    document.body.innerHTML = '';
    edits = [];
    store = new FrontmatterStore({ initial, requestSave: vi.fn().mockResolvedValue(undefined) });
    bar = new ChipBar(SCHEMA_FIXTURE, store, {
      onFieldEdit: (key, before, after) => edits.push({ key, before, after }),
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    bar.render(container);
  };

  /** Open a chip's edit popover the way a click does. */
  const openChip = (key: string) => {
    const chip = document.querySelector(`.chip[data-key="${key}"]`) as HTMLElement;
    expect(chip, `no chip rendered for ${key}`).toBeTruthy();
    chip.click();
  };

  const typeInto = (value: string) => {
    const input = document.querySelector('.pop input, .pop textarea') as HTMLInputElement;
    expect(input, 'the popover rendered no input').toBeTruthy();
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };

  beforeEach(() => mount());

  it('records ONE entry for a whole popover visit, not one per keystroke', () => {
    // Seeded: renderChips skips a field whose value is undefined, so a chip to
    // click into only exists once the field does.
    mount({ title: 'H' });
    openChip('title');
    // Six input events, the way six keystrokes arrive.
    for (const v of ['H', 'He', 'Hel', 'Hell', 'Hello', 'Hello!']) typeInto(v);
    expect(edits).toEqual([]); // nothing yet — the visit is still open

    bar.closeEditOverlay();

    expect(edits).toEqual([{ key: 'title', before: 'H', after: 'Hello!' }]);
  });

  it('records nothing when the visit changed nothing', () => {
    mount({ title: 'Same' });
    openChip('title');
    typeInto('Same');
    bar.closeEditOverlay();
    expect(edits).toEqual([]);
  });

  it('carries the value the popover OPENED with, not the one before the last keystroke', () => {
    // The boundary is what makes an undo jump the whole visit. If `before` were
    // resampled per commit, undoing would step back one character at a time
    // through a value the user changed in one go.
    mount({ title: 'Old' });
    openChip('title');
    typeInto('New');
    bar.closeEditOverlay();
    expect(edits).toEqual([{ key: 'title', before: 'Old', after: 'New' }]);
  });

  it('a heading commit records even though it never opens a popover', () => {
    // setFieldValue bypasses commitField entirely — it is how the editor
    // heading writes `title:` in Title mode. Without its own capture that edit
    // would be missing from the log while sitting in the file.
    bar.setFieldValue('title', 'From the heading');
    expect(edits).toEqual([
      { key: 'title', before: undefined, after: 'From the heading' },
    ]);
  });

  it('a write to a DIFFERENT field is recorded even while a popover is open', () => {
    // The popover's boundary snapshot covers `snap.key` alone, so suppressing
    // every key while one is open drops a write to another field out of the log
    // entirely — applied to the store, absent from undo, which is the one
    // failure mode this capture exists to prevent. No writer does this today
    // (the heading and create-flow commit on blur, which precedes the chip's
    // click), so the narrow guard is a trap set for the next one — and a trap
    // with no test is not set.
    mount({ title: 'Old' });
    openChip('title');
    bar.setFieldValue('draft', true);
    expect(edits).toEqual([{ key: 'draft', before: undefined, after: true }]);

    // …and the popover's OWN field is still owned by the boundary: one entry
    // for the visit, not one per write.
    edits.length = 0;
    bar.setFieldValue('title', 'Newer');
    expect(edits).toEqual([]);
    bar.closeEditOverlay();
    expect(edits).toEqual([{ key: 'title', before: 'Old', after: 'Newer' }]);
  });

  it('reports a removal as after: undefined, which is what makes undo re-add it', () => {
    mount({ title: 'Doomed' });
    bar.setFieldValue('title', undefined);
    expect(edits).toEqual([{ key: 'title', before: 'Doomed', after: undefined }]);
  });

  it('emits nothing during destroy, even though teardown closes the popover', () => {
    // ChipBar.destroy calls closePop, which fires the boundary. That emit would
    // run against a store about to be destroyed and be stamped with the
    // INCOMING document's generation — a forged entry for a file the user has
    // already left.
    mount({ title: 'Was' });
    openChip('title');
    typeInto('Half-typed');
    edits.length = 0;

    bar.destroy();

    expect(edits).toEqual([]);
  });

  it('records nothing for a write that changes nothing', () => {
    // The heading commits on blur whether or not the text moved, so without the
    // equality guard every focus-and-leave pushes a phantom entry — and ⌘Z then
    // appears to do nothing for as many presses as the user has clicked away.
    mount({ title: 'Same' });
    bar.setFieldValue('title', 'Same');
    expect(edits).toEqual([]);
  });

  it('a checkbox toggle is undoable', () => {
    // A direct store.setField call puts the
    // gesture in the file but not in the log: toggling `draft` and pressing ⌘Z
    // reverted whatever came BEFORE the toggle, so the user's most recent
    // action was not the one taken back.
    mount({ draft: false });
    const chip = document.querySelector('.chip[data-key="draft"]') as HTMLElement;
    expect(chip, 'no chip rendered for draft').toBeTruthy();
    chip.click();
    expect(store.getValues().draft).toBe(true);
    expect(edits).toEqual([{ key: 'draft', before: false, after: true }]);
  });

  it('the seeded nav click is one step, and undo removes nav again', () => {
    // Through the real DocumentHistory: the first write of a provisional ghost
    // must be undoable as an add (before undefined), not left in the file.
    const navSchema: any = {
      ...SCHEMA_FIXTURE,
      frontmatter: {
        fields: {
          ...SCHEMA_FIXTURE.frontmatter.fields,
          nav: { name: 'nav', widget: 'checkbox', type: 'boolean', priority: 40, required: false, description: '', group: 'Common' },
        },
      },
    };
    store = new FrontmatterStore({ initial: { title: 'Hello' }, requestSave: vi.fn().mockResolvedValue(undefined) });
    const hist = new DocumentHistory({
      view: () => null,
      generation: () => 1,
      readField: (k) => store.getValues()[k],
      writeField: (k, v) => (v === undefined ? store.removeField(k) : store.setField(k, v)),
      surface: () => {},
    });
    bar = new ChipBar(navSchema, store, { onFieldEdit: (k, b, a) => hist.recordField(k, b, a) });
    bar.render(document.body.appendChild(document.createElement('div')));

    bar.openField('nav', true);
    (document.querySelector('.chip--provisional[data-key="nav"]') as HTMLElement).click();
    expect(store.getValues().nav).toBe(false);

    expect(hist.undo()).toBe(true);
    expect(store.getValues()).not.toHaveProperty('nav');
  });

  it('removing a field from the context menu is undoable', () => {
    // The most destructive chip gesture there is. It wrote through
    // store.removeField, which no capture point watches.
    mount({ tags: ['a', 'b'] });
    const chip = document.querySelector('.chip[data-key="tags"]') as HTMLElement;
    chip.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const remove = [...document.querySelectorAll('.ctx-menu [role="menuitem"]')].find((el) =>
      /remove/i.test(el.textContent ?? ''),
    ) as HTMLElement;
    expect(remove, 'no Remove field entry in the chip context menu').toBeTruthy();
    remove.click();

    expect(store.getValues().tags).toBeUndefined();
    expect(edits).toEqual([{ key: 'tags', before: ['a', 'b'], after: undefined }]);
  });

  it('hasOpenEditOverlay tells ⌘Z to close the popover instead of reaching past it', () => {
    mount({ title: 'Anything' });
    expect(bar.hasOpenEditOverlay()).toBe(false);
    openChip('title');
    expect(bar.hasOpenEditOverlay()).toBe(true);
    bar.closeEditOverlay();
    expect(bar.hasOpenEditOverlay()).toBe(false);
  });

  // ── What the user SEES when a step lands ────────────────────────────────
  //
  // These assert on the bar's own DOM, not on a hook the test supplies. The
  // difference matters: document-history.test.ts proves that `surface(key,
  // removed, restored)` is called with the right flags, and it proved that
  // while `flashAddButton` and `announce` could both be stubbed to `return;`
  // with the whole editor suite still green — the callee was never reached by
  // any assertion. That is the shape of the `.article-cover img` bug: a module
  // whose tests build the world they then find.
  describe('surfacing an undone step', () => {
    const region = () => document.querySelector('[role="status"][aria-live="polite"]');

    it('flashes the add button and NAMES the field that left', async () => {
      // A removal is the case most in need of feedback and the only one no
      // chip can carry: renderChips skips a field whose value is undefined, so
      // there is no element left to wash.
      mount({ tags: ['a'] });
      bar.flashAddButton('tags');

      const btn = document.querySelector('.chip-unified-btn');
      expect(btn?.classList.contains('moss-just-committed')).toBe(true);

      // Empty first, then filled on the next tick — a region populated in the
      // same tick it is inserted is not announced by most screen readers.
      expect(region()?.textContent).toBe('');
      await new Promise((r) => setTimeout(r, 0));
      expect(region()?.isConnected).toBe(true);
      expect(region()?.textContent).toMatch(/tags/i);
    });

    it('re-creates the live region that a re-render detached', async () => {
      // renderChips rebuilds the container's children, and a detached region
      // announces nothing at all — silently, which is how an accessibility
      // regression survives.
      mount({ tags: ['a'], draft: true });
      bar.flashAddButton('tags');
      await new Promise((r) => setTimeout(r, 0));
      const first = region();

      bar.render(document.querySelector('div')!);
      bar.flashAddButton('draft');
      await new Promise((r) => setTimeout(r, 0));

      expect(region()?.isConnected).toBe(true);
      expect(region()).not.toBe(first);
      expect(region()?.textContent).toMatch(/draft/i);
    });

    it('promotes a chip the previous render could not have measured', () => {
      // Undo of a REMOVAL creates a key that did not exist a moment ago, so it
      // cannot appear in `hiddenFields` — that measurement is taken in a rAF
      // over the previous render. Without the explicit flag the wash plays for
      // one frame and the chip then collapses into the overflow `+N`, which is
      // the case the "move to front if needed" feedback was written for.
      mount({ title: 'T', tags: ['a'], draft: true });
      const keysBefore = [...document.querySelectorAll('.chip')].map((c) =>
        c.getAttribute('data-key'),
      );
      expect(keysBefore[0]).not.toBe('draft');

      bar.revealField('draft', { promote: true });

      const keysAfter = [...document.querySelectorAll('.chip')].map((c) =>
        c.getAttribute('data-key'),
      );
      expect(keysAfter[0]).toBe('draft');
      const chip = document.querySelector('.chip[data-key="draft"]');
      expect(chip?.classList.contains('moss-revealed')).toBe(true);
    });
  });
});
