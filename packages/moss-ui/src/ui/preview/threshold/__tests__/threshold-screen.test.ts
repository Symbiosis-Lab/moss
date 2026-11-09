// State transitions and re-render guard of the nested-site threshold screen.
// Follows the waiting-screen surface's test approach: a real jsdom container,
// injected deps, no Tauri — the transport is a spy, which is the point of the
// seam module.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  ThresholdScreenSurface,
  RENDER_ENTER_GUARD_MS,
  thresholdShapeKey,
} from '../threshold-screen';
import {
  offersMakeRoot,
  rootFolderName,
  type NestedRootInfo,
  type NestedRootsReport,
  type ThresholdPrompt,
} from '../seam';
import type { GuardDecision } from "../../../host";

function nested(name: string, rel: string): NestedRootInfo {
  return {
    rel_path: rel,
    abs_path: `/home/u/Drive/${rel}`,
    name,
    published: false,
    site_id: null,
  };
}

function report(overrides: Partial<NestedRootsReport> = {}): NestedRootsReport {
  return {
    root: '/home/u/Drive',
    root_class: { kind: 'normal' },
    nested: [nested('blog', 'writing/blog')],
    truncated: false,
    dirs_visited: 42,
    ...overrides,
  };
}

const unowned = (overrides: Partial<NestedRootsReport> = {}): ThresholdPrompt => ({
  token: 1,
  kind: 'unowned',
  report: report(overrides),
  suggestion: null,
});

const owned = (
  overrides: Partial<NestedRootsReport> = {},
  suggestion: ThresholdPrompt['suggestion'] = null,
): ThresholdPrompt => ({
  token: 1,
  kind: 'owned',
  report: report(overrides),
  suggestion,
});

describe('ThresholdScreenSurface', () => {
  let container: HTMLElement;
  let decisions: GuardDecision[];
  let surface: ThresholdScreenSurface;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    decisions = [];
    surface = new ThresholdScreenSurface({
      decide: (d) => decisions.push(d),
    });
  });

  afterEach(() => {
    surface.drop();
    container.remove();
    vi.useRealTimers();
  });

  describe('case (a): one nested site', () => {
    it('renders the card, focuses the primary button, and opens on Return/click', () => {
      surface.showPrompt(container, unowned());
      const primary = container.querySelector<HTMLButtonElement>('[data-threshold-primary]')!;
      expect(document.activeElement).toBe(primary);
      // The single card is informational, not a second click target.
      expect(container.querySelector('button[data-threshold-card]')).toBeNull();
      primary.click();
      expect(decisions).toEqual([
        { kind: 'open_nested', abs_path: '/home/u/Drive/writing/blog' },
      ]);
    });

    it('offers make-root only when the report says preview-only + Normal', () => {
      surface.showPrompt(container, unowned());
      expect(container.querySelector('[data-threshold-make-root]')).not.toBeNull();

      surface.drop();
      const published = unowned({ nested: [{ ...nested('blog', 'writing/blog'), published: true }] });
      surface.showPrompt(container, published);
      expect(container.querySelector('[data-threshold-make-root]')).toBeNull();

      surface.drop();
      const cloud = unowned({
        root_class: { kind: 'cloud_provider_root', provider: 'google_drive' },
      });
      surface.showPrompt(container, cloud);
      expect(container.querySelector('[data-threshold-make-root]')).toBeNull();
    });

    it('full paths ride on data-tooltip, never title=', () => {
      surface.showPrompt(container, unowned());
      const card = container.querySelector('.moss-threshold__card')!;
      expect(card.getAttribute('data-tooltip')).toBe('/home/u/Drive/writing/blog');
      expect(container.querySelector('[title]')).toBeNull();
    });

    it('re-render guard: the same prompt twice is a no-op', () => {
      surface.showPrompt(container, unowned());
      const stack = container.querySelector('.moss-threshold__stack')!;
      surface.showPrompt(container, unowned());
      expect(container.querySelector('.moss-threshold__stack')).toBe(stack);
    });
  });

  describe('make-root confirm beat', () => {
    it('swaps state in place with Go back pre-focused, and back returns to the prompt', () => {
      surface.showPrompt(container, unowned());
      // The grid is the overlay's, so a state swap leaves it running: this
      // beat used to stop and restart it, which re-seeds the particle field
      // and makes the background jump.
      const canvas = container.querySelector('#blueprint-canvas');
      container.querySelector<HTMLButtonElement>('[data-threshold-make-root]')!.click();
      expect(container.querySelector('#blueprint-canvas')).toBe(canvas);

      const back = container.querySelector<HTMLButtonElement>('[data-threshold-back]')!;
      expect(document.activeElement).toBe(back);
      // The confirm is present but NOT the default keypress.
      expect(container.querySelector('[data-threshold-confirm]')).not.toBeNull();

      back.click();
      // Back on the prompt, focus returned to the link that opened the beat.
      expect(container.querySelector('[data-threshold-make-root]')).not.toBeNull();
      expect(document.activeElement).toBe(
        container.querySelector('[data-threshold-make-root]'),
      );
      expect(decisions).toEqual([]);
    });

    it('confirm submits open_here', () => {
      surface.showPrompt(container, unowned());
      container.querySelector<HTMLButtonElement>('[data-threshold-make-root]')!.click();
      container.querySelector<HTMLButtonElement>('[data-threshold-confirm]')!.click();
      expect(decisions).toEqual([{ kind: 'open_here' }]);
    });
  });

  describe('case (b): several nested sites', () => {
    const many = (still = false) =>
      surface.showPrompt(
        container,
        unowned({ nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes')] }),
        still,
      );

    it('cards are a roving-tabindex list, first focused, arrows move', () => {
      many();
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      expect(cards).toHaveLength(2);
      expect(document.activeElement).toBe(cards[0]);
      expect(cards[0].tabIndex).toBe(0);
      expect(cards[1].tabIndex).toBe(-1);

      cards[0].dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
      );
      expect(document.activeElement).toBe(cards[1]);
      expect(cards[1].tabIndex).toBe(0);
      expect(cards[0].tabIndex).toBe(-1);

      cards[1].click();
      expect(decisions).toEqual([
        { kind: 'open_nested', abs_path: '/home/u/Drive/b/notes' },
      ]);
    });

    it('never offers make-root for more than one site', () => {
      many();
      expect(container.querySelector('[data-threshold-make-root]')).toBeNull();
    });

    it('progressive reveal appends late finds without moving focus', () => {
      many(true);
      const first = container.querySelector<HTMLButtonElement>('[data-threshold-card]')!;
      expect(document.activeElement).toBe(first);
      const footer = container.querySelector('[data-threshold-footer]')!;
      expect(footer.hasAttribute('hidden')).toBe(false);

      surface.showPrompt(
        container,
        unowned({
          nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes'), nested('zine', 'c/zine')],
        }),
        true,
      );
      const cards = container.querySelectorAll('[data-threshold-card]');
      expect(cards).toHaveLength(3);
      // Focus never moved, the existing card was not re-created.
      expect(document.activeElement).toBe(first);
      expect(cards[0]).toBe(first);

      // Walk ends: the footer comes down without touching the list.
      surface.showPrompt(
        container,
        unowned({
          nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes'), nested('zine', 'c/zine')],
        }),
        false,
      );
      expect(container.querySelector('[data-threshold-footer]')!.hasAttribute('hidden')).toBe(true);
      expect(container.querySelectorAll('[data-threshold-card]')).toHaveLength(3);
      expect(document.activeElement).toBe(first);
    });

    it('a late find appended to case (a) upgrades to the picker markup instead', () => {
      surface.showPrompt(container, unowned(), true);
      surface.showPrompt(
        container,
        unowned({ nested: [nested('blog', 'writing/blog'), nested('notes', 'b/notes')] }),
        true,
      );
      // A single-card informational layout cannot absorb a second candidate:
      // this is a re-render into the list form.
      expect(container.querySelectorAll('button[data-threshold-card]')).toHaveLength(2);
    });
  });

  describe('truncated walk', () => {
    const capped = () =>
      unowned({
        truncated: true,
        nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes')],
      });

    it('says the list may be incomplete once the walk ends capped — not while still checking', () => {
      surface.showPrompt(container, capped(), true);
      const note = container.querySelector('[data-threshold-truncated]')!;
      // While the footer still promises more finds, the note stays down:
      // "still checking" and "stopped early" cannot both be true.
      expect(note.hasAttribute('hidden')).toBe(true);
      expect(
        container.querySelector('[data-threshold-footer]')!.hasAttribute('hidden'),
      ).toBe(false);

      surface.showPrompt(container, capped(), false);
      expect(
        container.querySelector('[data-threshold-truncated]')!.hasAttribute('hidden'),
      ).toBe(false);
      expect(
        container.querySelector('[data-threshold-footer]')!.hasAttribute('hidden'),
      ).toBe(true);
    });

    it('an untruncated walk shows no note at all', () => {
      surface.showPrompt(container, unowned(), false);
      expect(container.querySelector('[data-threshold-truncated]')).toBeNull();
    });

    it('a walk that ends capped without new finds still surfaces the note', () => {
      // Same candidates, truncated flips at the end: the shape changed even
      // though the list did not.
      const open = unowned({ nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes')] });
      surface.showPrompt(container, open, true);
      expect(container.querySelector('[data-threshold-truncated]')).toBeNull();
      surface.showPrompt(container, capped(), false);
      const note = container.querySelector('[data-threshold-truncated]')!;
      expect(note.hasAttribute('hidden')).toBe(false);
    });
  });

  describe('case (d): one question, un-vault the loser', () => {
    const prompt = owned({ nested: [nested('blog', 'writing/blog')] });

    it('presents the nested site and the outer folder as peers, nested focused', () => {
      surface.showPrompt(container, prompt);
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      expect(cards).toHaveLength(2);
      expect(cards[1].hasAttribute('data-threshold-outer')).toBe(true);
      expect(document.activeElement).toBe(cards[0]);
      // The removal-mechanism note is on screen; there is no keep-root escape
      // and no make-root offer (superseded by the amendment).
      expect(container.querySelector('.moss-threshold__note')).not.toBeNull();
      expect(container.querySelector('[data-threshold-make-root]')).toBeNull();
    });

    it('picking the nested site opens it — once, however many clicks land after', () => {
      surface.showPrompt(container, prompt);
      const cards = container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]');
      // Double-click, then a click on the OPPOSITE card: in case (d) the two
      // decisions delete opposite .moss folders, so a second decision is not
      // a duplicate — it is a contradiction. Only the first may fire.
      cards[0].click();
      cards[0].click();
      cards[1].click();
      expect(decisions).toEqual([
        { kind: 'open_nested', abs_path: '/home/u/Drive/writing/blog' },
      ]);
    });

    it('picking the outer folder opens here', () => {
      surface.showPrompt(container, prompt);
      const cards = container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]');
      cards[1].click();
      expect(decisions).toEqual([{ kind: 'open_here' }]);
    });

    it('a decision disarms every control on the screen', () => {
      surface.showPrompt(container, prompt);
      container.querySelector<HTMLButtonElement>('[data-threshold-card]')!.click();
      const buttons = Array.from(container.querySelectorAll<HTMLButtonElement>('button'));
      expect(buttons.length).toBeGreaterThan(0);
      for (const b of buttons) expect(b.disabled).toBe(true);
      // The cancel escape is disarmed too — the guard acts on ONE decision.
      container.querySelector<HTMLButtonElement>('[data-threshold-cancel]')!.click();
      expect(decisions).toHaveLength(1);
    });

    it('a new prompt render re-arms the latch', () => {
      surface.showPrompt(container, prompt);
      container.querySelector<HTMLButtonElement>('[data-threshold-card]')!.click();
      expect(decisions).toHaveLength(1);
      // The guard answered (e.g. cancel round-trip) and asks again.
      surface.showPrompt(container, owned({ nested: [nested('notes', 'b/notes')] }));
      container.querySelector<HTMLButtonElement>('[data-threshold-card]')!.click();
      expect(decisions).toHaveLength(2);
    });

    it('after drop(), leftover markup is inert', () => {
      surface.showPrompt(container, prompt);
      surface.drop();
      // drop() does not clear the container (the caller overwrites innerHTML),
      // but a dropped surface must not keep a live decision path in the DOM.
      container.querySelector<HTMLButtonElement>('[data-threshold-card]')!.click();
      container.querySelector<HTMLButtonElement>('[data-threshold-cancel]')!.click();
      expect(decisions).toEqual([]);
    });

    it('leads with a nested suggestion: guess-led title, suggested card is the focused primary', () => {
      const two = { nested: [nested('blog', 'writing/blog'), nested('notes', 'b/notes')] };
      surface.showPrompt(
        container,
        owned(two, { kind: 'nested', abs_path: '/home/u/Drive/b/notes' }),
      );
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      // Suggested card first and focused; the rest are equal-weight peers.
      expect(cards[0].dataset.abs).toBe('/home/u/Drive/b/notes');
      expect(cards[0].hasAttribute('data-threshold-primary')).toBe(true);
      expect(document.activeElement).toBe(cards[0]);
      expect(container.querySelector('.moss-threshold__title')!.textContent).toContain('notes');
      // The other folders are still one interaction away — no auto-proceed.
      expect(decisions).toEqual([]);
      cards[2].click(); // the outer folder, still an equal-weight card
      expect(decisions).toEqual([{ kind: 'open_here' }]);
    });

    it('leads with an outer suggestion: the outer card is first and focused', () => {
      surface.showPrompt(container, owned({}, { kind: 'outer' }));
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      expect(cards[0].hasAttribute('data-threshold-outer')).toBe(true);
      expect(document.activeElement).toBe(cards[0]);
      expect(container.querySelector('.moss-threshold__title')!.textContent).toContain('Drive');
    });

    it('a suggestion naming no known candidate falls back to the neutral peer layout', () => {
      surface.showPrompt(container, owned({}, { kind: 'nested', abs_path: '/nowhere' }));
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      expect(document.activeElement).toBe(cards[0]);
      expect(cards[0].dataset.abs).toBe('/home/u/Drive/writing/blog');
      expect(container.querySelector('.moss-threshold__title')!.textContent).not.toContain(
        'looks like',
      );
    });

    it('late finds slot in above the outer-folder card', () => {
      surface.showPrompt(container, prompt, true);
      surface.showPrompt(
        container,
        owned({ nested: [nested('blog', 'writing/blog'), nested('notes', 'b/notes')] }),
        true,
      );
      const cards = Array.from(
        container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
      );
      expect(cards).toHaveLength(3);
      expect(cards[2].hasAttribute('data-threshold-outer')).toBe(true);
      expect(cards[1].dataset.abs).toBe('/home/u/Drive/b/notes');
    });
  });
});

describe('focus stability and the post-render Enter guard', () => {
  let container: HTMLElement;
  let decisions: GuardDecision[];
  let surface: ThresholdScreenSurface;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    decisions = [];
    surface = new ThresholdScreenSurface({
      decide: (d) => decisions.push(d),
    });
  });

  afterEach(() => {
    surface.drop();
    container.remove();
    vi.useRealTimers();
  });

  const two = () => unowned({ nested: [nested('blog', 'a/blog'), nested('notes', 'b/notes')] });
  // A SHALLOWER late find: new first candidate, so the append prefix-check
  // fails and the surface must fully re-render.
  const shallower = () =>
    unowned({
      nested: [nested('zine', 'zine'), nested('blog', 'a/blog'), nested('notes', 'b/notes')],
    });

  it('a full re-render keeps focus on the card the user chose, not the primary', () => {
    surface.showPrompt(container, two(), true);
    const cards = container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]');
    // The user engages: arrows down to the second candidate.
    cards[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(cards[1]);

    surface.showPrompt(container, shallower(), true);
    const after = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
    );
    expect(after).toHaveLength(3);
    const notes = after.find((c) => c.dataset.abs === '/home/u/Drive/b/notes')!;
    expect(document.activeElement).toBe(notes);
    // Roving tabindex follows the restored focus.
    expect(notes.tabIndex).toBe(0);
  });

  it('without user engagement, a re-render focuses the primary as before', () => {
    surface.showPrompt(container, two(), true);
    surface.showPrompt(container, shallower(), true);
    expect(document.activeElement).toBe(
      container.querySelector('[data-threshold-primary]'),
    );
  });

  it('Enter is swallowed for a beat after a render, then flows again', () => {
    vi.useFakeTimers();
    surface.showPrompt(container, two());
    const primary = container.querySelector<HTMLButtonElement>('[data-threshold-primary]')!;

    const early = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    primary.dispatchEvent(early);
    expect(early.defaultPrevented).toBe(true);

    vi.advanceTimersByTime(RENDER_ENTER_GUARD_MS);
    const late = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    primary.dispatchEvent(late);
    expect(late.defaultPrevented).toBe(false);
  });
});

describe('seam policy helpers', () => {
  it('offersMakeRoot: only unowned + Normal + exactly one preview-only site', () => {
    expect(offersMakeRoot(unownedPrompt())).toBe(true);
    expect(offersMakeRoot({ ...unownedPrompt(), kind: 'owned' })).toBe(false);
    expect(
      offersMakeRoot(unownedPrompt({ root_class: { kind: 'home_dir' } })),
    ).toBe(false);
    expect(
      offersMakeRoot(
        unownedPrompt({ nested: [{ ...nested('a', 'a'), published: true }] }),
      ),
    ).toBe(false);
    // Unreadable state (evicted) is NOT known preview-only — no offer.
    expect(
      offersMakeRoot(
        unownedPrompt({ nested: [{ ...nested('a', 'a'), published: null }] }),
      ),
    ).toBe(false);
    expect(
      offersMakeRoot(
        unownedPrompt({ nested: [nested('a', 'a'), nested('b', 'b')] }),
      ),
    ).toBe(false);
  });

  function unownedPrompt(overrides: Partial<NestedRootsReport> = {}): ThresholdPrompt {
    return { token: 1, kind: 'unowned', report: report(overrides), suggestion: null };
  }

  it('rootFolderName takes the basename, tolerating a trailing slash', () => {
    expect(rootFolderName(report())).toBe('Drive');
    expect(rootFolderName(report({ root: '/home/u/Drive/' }))).toBe('Drive');
  });

  it('thresholdShapeKey changes with kind, root and the candidate set', () => {
    const a = thresholdShapeKey(unownedPrompt());
    expect(thresholdShapeKey(unownedPrompt())).toBe(a);
    expect(thresholdShapeKey({ ...unownedPrompt(), kind: 'owned' })).not.toBe(a);
    expect(
      thresholdShapeKey(unownedPrompt({ nested: [nested('x', 'x')] })),
    ).not.toBe(a);
    // Amendment 2: a changed guess is new markup.
    expect(
      thresholdShapeKey({ ...unownedPrompt(), suggestion: { kind: 'outer' } }),
    ).not.toBe(a);
  });
});
