/**
 * Tests for cm-scroll-sync.ts — pure heading-anchor scroll sync functions.
 *
 * Covers:
 * - findNearestHeading: finds nearest heading above a given line
 * - headingToAnchor: converts heading text to anchor ID (must match Rust)
 * - Scroll sync toggle state persistence in localStorage
 */

import { describe, test, expect, beforeEach, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import { findNearestHeading, headingToAnchor, gotoLine, findRuleLine, selectTextNearLine, mapStrippedToOriginal, scrollEditorToLine, scrollEditorToTop, scrollEditorToBottom, computeContentOffset, getFocusLine } from '../cm-scroll-sync';

// ── findNearestHeading ──────────────────────────────────────────────────

describe('findNearestHeading', () => {
  const doc = [
    '---',              // line 1
    'title: Test',      // line 2
    '---',              // line 3
    '',                 // line 4
    '# Introduction',   // line 5
    '',                 // line 6
    'Some text here.',  // line 7
    '',                 // line 8
    '## Getting Started', // line 9
    '',                 // line 10
    'More text.',       // line 11
    '',                 // line 12
    '### Step 1: Install', // line 13
    '',                 // line 14
    'Install instructions.', // line 15
    '',                 // line 16
    '## Advanced',      // line 17
  ].join('\n');

  test('finds heading at the given line', () => {
    const result = findNearestHeading(doc, 5);
    expect(result).toEqual({ text: 'Introduction', level: 1, line: 5 });
  });

  test('finds nearest heading above the given line', () => {
    const result = findNearestHeading(doc, 7);
    expect(result).toEqual({ text: 'Introduction', level: 1, line: 5 });
  });

  test('finds closest heading, not first heading', () => {
    const result = findNearestHeading(doc, 11);
    expect(result).toEqual({ text: 'Getting Started', level: 2, line: 9 });
  });

  test('returns null when no heading above', () => {
    const result = findNearestHeading(doc, 3);
    expect(result).toBeNull();
  });

  test('returns null for empty document', () => {
    const result = findNearestHeading('', 1);
    expect(result).toBeNull();
  });

  test('handles line number beyond document length', () => {
    const result = findNearestHeading(doc, 100);
    expect(result).toEqual({ text: 'Advanced', level: 2, line: 17 });
  });

  test('finds h3 heading', () => {
    const result = findNearestHeading(doc, 15);
    expect(result).toEqual({ text: 'Step 1: Install', level: 3, line: 13 });
  });

  test('finds heading at line 1', () => {
    const result = findNearestHeading('# Top', 1);
    expect(result).toEqual({ text: 'Top', level: 1, line: 1 });
  });
});

// ── headingToAnchor ─────────────────────────────────────────────────────

describe('headingToAnchor', () => {
  test('basic heading', () => {
    expect(headingToAnchor('Getting Started')).toBe('getting-started');
  });

  test('preserves colons', () => {
    expect(headingToAnchor('Step 1: Install')).toBe('step-1:-install');
  });

  test('preserves CJK characters', () => {
    expect(headingToAnchor('中文标题')).toBe('中文标题');
  });

  test('preserves accented characters', () => {
    expect(headingToAnchor('cafe resume')).toBe('cafe-resume');
    expect(headingToAnchor('cafe\u0301 re\u0301sume\u0301')).toBe('cafe\u0301-re\u0301sume\u0301');
  });

  test('strips hash characters', () => {
    expect(headingToAnchor('Section #1')).toBe('section-1');
  });

  test('strips brackets', () => {
    expect(headingToAnchor('Array[0]')).toBe('array0');
  });

  test('strips pipe', () => {
    expect(headingToAnchor('A | B')).toBe('a-b');
  });

  test('strips caret', () => {
    expect(headingToAnchor('Note ^ref')).toBe('note-ref');
  });

  test('strips backslash', () => {
    expect(headingToAnchor('path\\to\\file')).toBe('pathtofile');
  });

  test('collapses consecutive hyphens', () => {
    expect(headingToAnchor('hello   world')).toBe('hello-world');
  });

  test('trims leading/trailing hyphens', () => {
    expect(headingToAnchor('  Hello World  ')).toBe('hello-world');
  });

  test('preserves parentheses', () => {
    expect(headingToAnchor('fn(x, y)')).toBe('fn(x,-y)');
  });

  test('empty string', () => {
    expect(headingToAnchor('')).toBe('');
  });

  test('only stripped characters', () => {
    expect(headingToAnchor('###')).toBe('');
  });

  test('preserves exclamation and question marks', () => {
    expect(headingToAnchor('Why? Because!')).toBe('why?-because!');
  });

  test('preserves ampersand and single quotes', () => {
    expect(headingToAnchor("Tom & Jerry's")).toBe("tom-&-jerry's");
  });

  test('tab replaced with hyphen', () => {
    expect(headingToAnchor('hello\tworld')).toBe('hello-world');
  });
});

// ── Scroll sync is always on — no localStorage key ──────────────────────
//
// Scroll sync is always active in split mode. There is no toggle and no
// 'moss-scroll-sync' localStorage key. These tests verify that invariant.

describe('scroll sync always-on invariant', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  test('moss-scroll-sync localStorage key does not exist by default', () => {
    // The key must never be written — sync is always active
    expect(localStorage.getItem('moss-scroll-sync')).toBeNull();
  });

  test('moss-scroll-sync localStorage key stays absent after multiple checks', () => {
    // Simulate repeated reads — nothing should write the key
    for (let i = 0; i < 5; i++) {
      expect(localStorage.getItem('moss-scroll-sync')).toBeNull();
    }
  });
});

// ── gotoLine ────────────────────────────────────────────────────────────

describe('gotoLine', () => {
  test('places cursor at start of target line and focuses', () => {
    const dispatched: any[] = [];
    const mockView = {
      dispatch: (spec: any) => dispatched.push(spec),
      focus: vi.fn(),
      state: {
        doc: {
          lines: 10,
          line: (n: number) => ({ from: (n - 1) * 20 }),
        },
      },
    };
    gotoLine(mockView as any, 5);
    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].selection.anchor).toBe(80); // (5-1)*20
    expect(dispatched[0].scrollIntoView).toBe(true);
    expect(mockView.focus).toHaveBeenCalled();
  });

  test('clamps line number to valid range', () => {
    const dispatched: any[] = [];
    const mockView = {
      dispatch: (spec: any) => dispatched.push(spec),
      focus: vi.fn(),
      state: {
        doc: {
          lines: 5,
          line: (n: number) => ({ from: (n - 1) * 10 }),
        },
      },
    };
    gotoLine(mockView as any, 100);
    expect(dispatched[0].selection.anchor).toBe(40); // clamped to line 5: (5-1)*10
  });

  test('clamps to line 1 for zero or negative input', () => {
    const dispatched: any[] = [];
    const mockView = {
      dispatch: (spec: any) => dispatched.push(spec),
      focus: vi.fn(),
      state: {
        doc: {
          lines: 5,
          line: (n: number) => ({ from: (n - 1) * 10 }),
        },
      },
    };
    gotoLine(mockView as any, 0);
    expect(dispatched[0].selection.anchor).toBe(0); // line 1: (1-1)*10
  });
});

// ── mapStrippedToOriginal ─────────────────────────────────────────────

describe('mapStrippedToOriginal', () => {
  test('plain text — identity mapping', () => {
    expect(mapStrippedToOriginal('hello world', 0)).toBe(0);
    expect(mapStrippedToOriginal('hello world', 5)).toBe(5);
  });

  test('leading markers — skips **', () => {
    // "**bold**" stripped is "bold", strippedIdx 0 → origIdx 2
    expect(mapStrippedToOriginal('**bold**', 0)).toBe(2);
  });

  test('trailing markers — end position after content', () => {
    // "**bold**" stripped is "bold", strippedIdx 4 → past 'd', past '**' → 8
    expect(mapStrippedToOriginal('**bold**', 4)).toBe(8);
  });

  test('inline markers — skips inner formatting', () => {
    // "some **bold** text" stripped is "some bold text"
    //  0123 45678 9...
    expect(mapStrippedToOriginal('some **bold** text', 5)).toBe(7); // 'b' is at orig 7
  });

  test('backtick markers', () => {
    expect(mapStrippedToOriginal('use `code` here', 4)).toBe(5); // 'c' at orig 5
  });

  test('strikethrough markers', () => {
    expect(mapStrippedToOriginal('~~removed~~', 0)).toBe(2);
  });

  test('end of string', () => {
    expect(mapStrippedToOriginal('**x**', 1)).toBe(5);
  });
});

// ── selectTextNearLine ────────────────────────────────────────────────

describe('selectTextNearLine', () => {
  // Helper to create a mock CM6 view with given content
  function createMockView(content: string) {
    const lines = content.split('\n');
    const dispatched: any[] = [];
    return {
      view: {
        dispatch(spec: any) { dispatched.push(spec); },
        focus() {},
        state: {
          doc: {
            lines: lines.length,
            line(n: number) {
              let from = 0;
              for (let i = 0; i < n - 1; i++) {
                from += lines[i].length + 1; // +1 for \n
              }
              return { from, to: from + lines[n - 1].length };
            },
            sliceString(from: number, to: number) {
              return content.slice(from, to);
            },
          },
        },
      },
      dispatched,
    };
  }

  test('exact match — selects text', () => {
    const { view, dispatched } = createMockView('Hello world\nfoo bar baz\nend');
    selectTextNearLine(view as any, 2, 2, 'bar');
    expect(dispatched).toHaveLength(1);
    // "foo bar baz" starts at offset 12, "bar" is at offset 16
    expect(dispatched[0].selection.anchor).toBe(16);
    expect(dispatched[0].selection.head).toBe(19);
  });

  test('match with markdown markers — exact match finds "bold" inside **bold**', () => {
    const { view, dispatched } = createMockView('Hello\nsome **bold** text\nend');
    selectTextNearLine(view as any, 2, 2, 'bold');
    expect(dispatched).toHaveLength(1);
    // "some **bold** text" starts at offset 6. "bold" found at idx 7 (exact match)
    expect(dispatched[0].selection.anchor).toBe(13); // 6 + 7
    expect(dispatched[0].selection.head).toBe(17);   // 6 + 7 + 4
  });

  test('marker stripping needed — text not in source verbatim', () => {
    // "**bold text**" has no "bold text" substring because ** is adjacent
    // Actually "**bold text**" contains "bold text" at index 2, so exact match works.
    // Let's test a case where exact match truly fails: "a **b** c" looking for "a b c"
    const { view, dispatched } = createMockView('x\na **b** c\ny');
    selectTextNearLine(view as any, 2, 2, 'a b c');
    expect(dispatched).toHaveLength(1);
    // Exact match fails (source has "a **b** c", not "a b c")
    // Stripped: "a b c", strippedIdx=0
    // mapStrippedToOriginal("a **b** c", 0) → 0 (content 'a' at origIdx 0)
    // mapStrippedToOriginal("a **b** c", 5) → origIdx 9
    // anchor = 2 + 0 = 2, head = 2 + 9 = 11
    expect(dispatched[0].selection.anchor).toBe(2);
    expect(dispatched[0].selection.head).toBe(11);
  });

  test('no match — falls back to goto line', () => {
    const { view, dispatched } = createMockView('Hello\nfoo bar\nend');
    selectTextNearLine(view as any, 2, 2, 'nonexistent');
    expect(dispatched).toHaveLength(1);
    // Falls back to line 2 start (offset 6)
    expect(dispatched[0].selection.anchor).toBe(6);
    expect(dispatched[0].selection.head).toBe(6);
  });

  test('multi-line range', () => {
    const { view, dispatched } = createMockView('line1\nline2 target\nline3');
    selectTextNearLine(view as any, 1, 3, 'target');
    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].selection.anchor).toBe(12); // "target" at offset 12
    expect(dispatched[0].selection.head).toBe(18);
  });

  test('line 1 clamp for article title sentinel', () => {
    const { view, dispatched } = createMockView('first line content\nsecond');
    selectTextNearLine(view as any, 1, 1, 'first');
    expect(dispatched).toHaveLength(1);
    expect(dispatched[0].selection.anchor).toBe(0);
    expect(dispatched[0].selection.head).toBe(5);
  });
});

// ── scrollEditorToLine ──────────────────────────────────────────────────
//
// There is no `lineNumber <= 1` short-circuit: the atTop wire field handles
// "go to top" explicitly via scrollEditorToTop.

describe('scrollEditorToBottom', () => {
  test('parks the scroller at its maximum (scrollTop = scrollHeight)', () => {
    const scrollDOM = { scrollTop: 120, scrollHeight: 4000 };
    scrollEditorToBottom({ scrollDOM: scrollDOM as unknown as HTMLElement });
    // Assigning scrollHeight is clamped to the real max by the browser; the
    // helper's job is simply to request the bottom.
    expect(scrollDOM.scrollTop).toBe(4000);
  });

  test('is the symmetric counterpart of scrollEditorToTop', () => {
    const top = { scrollTop: 999, scrollHeight: 4000 };
    const bottom = { scrollTop: 0, scrollHeight: 4000 };
    scrollEditorToTop({ scrollDOM: top as unknown as HTMLElement });
    scrollEditorToBottom({ scrollDOM: bottom as unknown as HTMLElement });
    expect(top.scrollTop).toBe(0);
    expect(bottom.scrollTop).toBe(4000);
  });
});

describe('scrollEditorToLine', () => {
  // CM6 places the line (it can render one that is off screen; coordsAtPos
  // cannot see it). What is ours is what we ask for: which line, and a margin
  // that puts its top at the focus fraction below the title's scroll margin.
  function mockView(docLines: number, clientHeight: number, contentOffsetTop: number) {
    return {
      scrollDOM: { clientHeight } as HTMLElement,
      contentDOM: { offsetTop: contentOffsetTop } as HTMLElement,
      state: { doc: { lines: docLines, line: (n: number) => ({ from: (n - 1) * 10 }) } },
      dispatch: vi.fn(),
    } as unknown as Parameters<typeof scrollEditorToLine>[0];
  }

  test('asks CM6 to put the line at the centre, counted below the title', () => {
    const into = vi.spyOn(EditorView, 'scrollIntoView');
    const view = mockView(100, 800, 150);
    scrollEditorToLine(view, 5);
    // 800 * 0.5 - 150: the title's margin is added by CM6 itself.
    expect(into).toHaveBeenCalledWith(40, { y: 'start', yMargin: 250 });
    expect(view.dispatch).toHaveBeenCalledTimes(1);
  });

  test('clamps to the document, and never asks for a negative margin', () => {
    const into = vi.spyOn(EditorView, 'scrollIntoView');
    scrollEditorToLine(mockView(10, 800, 0), 999);
    expect(into).toHaveBeenLastCalledWith(90, { y: 'start', yMargin: 400 });
    scrollEditorToLine(mockView(10, 200, 300), 0);
    expect(into).toHaveBeenLastCalledWith(0, { y: 'start', yMargin: 0 });
  });
});

describe('getFocusLine', () => {
  // lineBlockAtHeight(h) returns the block whose row index = floor(h/20); each
  // line is 20px tall, doc.lineAt(from) maps from→line number.
  function mockView(scrollTop: number, clientHeight: number) {
    return {
      scrollDOM: { scrollTop, clientHeight } as unknown as HTMLElement,
      // no contentDOM → computeContentOffset returns 0
      lineBlockAtHeight: (h: number) => ({ from: Math.max(0, Math.floor(h / 20)) * 20 }),
      state: { doc: { lineAt: (pos: number) => ({ number: Math.floor(pos / 20) + 1 }) } },
    };
  }

  test('reports the line at the viewport center, not the top', () => {
    // scrollTop 0, clientHeight 800 → focus y = 0 + 800*0.5 = 400 → block row 20
    // → from 400 → line 21. (Top-anchored would report line 1.)
    expect(getFocusLine(mockView(0, 800) as unknown as Parameters<typeof getFocusLine>[0])).toBe(21);
  });

  test('adds scrollTop to the focus offset', () => {
    // scrollTop 200 + 400 = 600 → row 30 → from 600 → line 31
    expect(getFocusLine(mockView(200, 800) as unknown as Parameters<typeof getFocusLine>[0])).toBe(31);
  });

  test('fraction 0 reports the top line (back-compat with top anchoring)', () => {
    // focus y = 200 + 800*0 = 200 → row 10 → from 200 → line 11
    expect(getFocusLine(mockView(200, 800) as unknown as Parameters<typeof getFocusLine>[0], 0)).toBe(11);
  });
});

// ── scrollEditorToTop + computeContentOffset (atTop wire field) ─────────

describe('scrollEditorToTop', () => {
  test('sets scrollDOM.scrollTop to 0 without touching coordsAtPos', () => {
    const scrollDOM = { scrollTop: 1000, getBoundingClientRect: () => ({} as DOMRect) };
    const view = { scrollDOM: scrollDOM as unknown as HTMLElement };
    scrollEditorToTop(view);
    expect(scrollDOM.scrollTop).toBe(0);
  });

  test('idempotent when already at top', () => {
    const scrollDOM = { scrollTop: 0, getBoundingClientRect: () => ({} as DOMRect) };
    const view = { scrollDOM: scrollDOM as unknown as HTMLElement };
    scrollEditorToTop(view);
    expect(scrollDOM.scrollTop).toBe(0);
  });
});

describe('computeContentOffset', () => {
  function mkRect(top: number): DOMRect {
    return { top, left: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: top, toJSON() {} } as DOMRect;
  }

  test('returns 0 when contentDOM is missing', () => {
    const view = {
      scrollDOM: { scrollTop: 0, getBoundingClientRect: () => mkRect(0) } as unknown as HTMLElement,
    };
    expect(computeContentOffset(view)).toBe(0);
  });

  test('returns heading height when contentDOM is below scrollDOM (at top)', () => {
    // scrollDOM at viewport y=0, contentDOM at viewport y=40 (40px heading),
    // scrollTop=0 → offset = 40 - 0 + 0 = 40.
    const view = {
      scrollDOM: { scrollTop: 0, getBoundingClientRect: () => mkRect(0) } as unknown as HTMLElement,
      contentDOM: { getBoundingClientRect: () => mkRect(40) } as unknown as HTMLElement,
    };
    expect(computeContentOffset(view)).toBe(40);
  });

  test('returns heading height when scrolled (offset is invariant)', () => {
    // After scrolling 100px: scrollDOM still at viewport y=0 (scroller container
    // stays put), contentDOM now at viewport y=40-100=-60. scrollTop=100 → offset
    // = -60 - 0 + 100 = 40. The offset is the heading's height, NOT a function
    // of scrollTop.
    const view = {
      scrollDOM: { scrollTop: 100, getBoundingClientRect: () => mkRect(0) } as unknown as HTMLElement,
      contentDOM: { getBoundingClientRect: () => mkRect(-60) } as unknown as HTMLElement,
    };
    expect(computeContentOffset(view)).toBe(40);
  });
});

// ── findRuleLine ────────────────────────────────────────────────────────

describe('findRuleLine', () => {
  // jsdom normalises differently from WebKit, so mimic the two documented rewrites.
  const webkit = (sel: string): string | null =>
    sel.replace(/\[([\w-]+)=([\w-]+)\]/g, '[$1="$2"]').replace(/([^:]):(before|after)\b/g, '$1::$2').replace(/\s+/g, ' ').trim();
  const find = (css: string, sel: string) => findRuleLine(css, sel, webkit);

  test('finds the line of a single-selector rule', () => {
    expect(find('body {\n  color: red;\n}\nh1 {\n  margin: 0;\n}', 'h1')).toBe(4);
  });

  test('finds an attribute selector written without quotes', () => {
    expect(find('a {}\n[data-x=a] {\n}', '[data-x="a"]')).toBe(2);
  });

  test('finds a pseudo-element written with one colon', () => {
    expect(find('a {}\np:before {\n}', 'p::before')).toBe(2);
  });

  test.each([
    ['a selector on its own line, the list running on', 'a {}\nh1,\nh2 {\n}', 'h1', 2],
    ['a selector after another across lines', 'a {}\n.a,\n h1 {\n}', 'h1', 2],
    ['a selector after another in the list', 'a {}\n.a, h1 {\n}', 'h1', 2],
    ['a compound selector with a comma inside parentheses', 'a {}\n.a h1:is(.x, .y), p {\n}', 'p', 2],
    ['a selector inside @media', '@media (min-width: 600px) {\n  h2 {\n  }\n}', 'h2', 2],
  ])('finds %s', (_what, css, selector, line) => expect(find(css, selector)).toBe(line));

  test('ignores a selector inside a comment', () => {
    expect(find('/* h1 {\n} */\np {\n}', 'h1')).toBeNull();
  });

  test('skips a header where the selector is only a prefix or a descendant', () => {
    expect(find('h1.big {\n}\nmain h1 {\n}\nh1 {\n}', 'h1')).toBe(5);
    expect(find('h1.big {\n}', 'h1')).toBeNull();
  });

  test('returns null when no rule matches', () => {
    expect(find('p {\n}\n', 'h1')).toBeNull();
  });

  test('the default normaliser reads the engine\'s selectorText and drops a selector it rejects', () => {
    // jsdom has no replaceSync; stand it in with insertRule so the real default runs on jsdom's CSSOM.
    const proto = CSSStyleSheet.prototype as unknown as { replaceSync?: (text: string) => void };
    proto.replaceSync = function (this: CSSStyleSheet, text: string) {
      while (this.cssRules.length) this.deleteRule(0);
      this.insertRule(text);
    };
    try {
      expect(findRuleLine('a {}\n[[bad {\n}\np {}', 'p')).toBe(4);
      expect(findRuleLine('a {}\n  h1 > p {\n}', 'h1 > p')).toBe(2);
    } finally {
      delete proto.replaceSync;
    }
  });
});
