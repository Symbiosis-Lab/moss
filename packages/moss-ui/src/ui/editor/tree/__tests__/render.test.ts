import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { mkSeg, place, mkGroup, CHEV_SVG } from '../render';

// Read the stylesheet as a string (vitest stubs `.css` imports to empty, so
// `?raw` won't work). The connector rail relies on three CSS invariants that
// jsdom can't compute (no layout), so we lock them against the stylesheet TEXT
// so a future refactor can't silently break "connects without touching".
const FT_CSS = readFileSync(join(import.meta.dirname, '../../file-tree.css'), 'utf8');

describe('connector rail CSS contract', () => {
  it('masks the rail: .dot and .chev use an opaque surface background', () => {
    // Opaque background over the z-index:0 rail is what makes the rail appear to
    // "connect without touching" — visible in the gaps, hidden behind each icon.
    expect(FT_CSS).toMatch(/\.dot\s*\{[^}]*background:\s*var\(--moss-color-surface\)/);
    expect(FT_CSS).toMatch(/\.chev\s*\{[^}]*background:\s*var\(--moss-color-surface\)/);
  });

  it('stacks rows above the rail: .seg z-index 1 > rail z-index 0', () => {
    expect(FT_CSS).toMatch(/\.seg\s*\{[^}]*z-index:\s*1/);
    expect(FT_CSS).toMatch(/\[role="group"\]::before\s*\{[^}]*z-index:\s*0/);
  });

  it('ties the rail +9px half-icon to the 18px icon box', () => {
    // The rail sits at content-left + 9px = the icon CENTER only while the icon
    // box is 18px wide. If the icon width changes, this test forces the +9 too.
    expect(FT_CSS).toMatch(/\.chev\s*\{[^}]*width:\s*18px/);
    expect(FT_CSS).toMatch(/\.dot\s*\{[^}]*width:\s*18px/);
    expect(FT_CSS).toMatch(/\[role="group"\]::before\s*\{[^}]*\+\s*9px/);
  });

  it('hosts the rail on a real box, never display:contents', () => {
    // A display:contents wrapper can't host a ::before; the wrapper must be a box.
    expect(FT_CSS).toMatch(/\.nav-body\s*\[role="group"\]\s*\{[^}]*display:\s*flex/);
    expect(FT_CSS).not.toMatch(/\[role="group"\]\s*\{[^}]*display:\s*contents/);
  });
});

describe('mkSeg', () => {
  it('creates a folder segment with chevron SVG', () => {
    const el = mkSeg('posts', true);
    expect(el.tagName).toBe('DIV');
    expect(el.classList.contains('seg')).toBe(true);
    expect(el.classList.contains('leaf')).toBe(false);

    const chev = el.querySelector('.chev') as HTMLElement;
    expect(chev).not.toBeNull();
    expect(chev!.tagName).toBe('SPAN');
    expect(chev!.innerHTML).toContain('<svg');

    // Text span
    const spans = el.querySelectorAll('span');
    const textSpan = spans[spans.length - 1];
    expect(textSpan.textContent).toBe('posts');
  });

  it('creates a file segment with dot (no SVG)', () => {
    const el = mkSeg('hello.md', false);
    expect(el.classList.contains('seg')).toBe(true);
    expect(el.classList.contains('leaf')).toBe(true);

    const dot = el.querySelector('.dot') as HTMLElement;
    expect(dot).not.toBeNull();
    expect(dot!.tagName).toBe('SPAN');
    expect(dot!.innerHTML).toBe(''); // no SVG inside dot

    expect(el.querySelector('.chev')).toBeNull();

    const spans = el.querySelectorAll('span');
    const textSpan = spans[spans.length - 1];
    expect(textSpan.textContent).toBe('hello.md');
  });

  it('adds extra classes like "open"', () => {
    const el = mkSeg('posts', true, 'open');
    expect(el.classList.contains('seg')).toBe(true);
    expect(el.classList.contains('open')).toBe(true);
  });
});

describe('place', () => {
  let el: HTMLElement;

  beforeEach(() => {
    el = document.createElement('div');
  });

  it('sets --ft-offset (px) and data-depth for a tree row from the shared offset array', () => {
    const result = place(el, 2, 140);
    expect(result).toBe(el);
    expect(el.style.getPropertyValue('--ft-offset')).toBe('140px');
    expect(el.getAttribute('data-depth')).toBe('2');
    expect(el.style.gridColumn).toBe('');
  });

  it('depth 0 with offset 0 sets --ft-offset 0px', () => {
    place(el, 0, 0);
    expect(el.style.getPropertyValue('--ft-offset')).toBe('0px');
    expect(el.getAttribute('data-depth')).toBe('0');
  });
});

describe('mkGroup', () => {
  it('creates a role="group" wrapper carrying the child column as --ft-rail-offset', () => {
    const g = mkGroup(140);
    expect(g.tagName).toBe('DIV');
    expect(g.getAttribute('role')).toBe('group');
    expect(g.style.getPropertyValue('--ft-rail-offset')).toBe('140px');
    // NOT display:contents — the wrapper is a real box that hosts the rail.
    expect(g.style.display).toBe('');
  });

  it('handles a zero offset (root-level group)', () => {
    expect(mkGroup(0).style.getPropertyValue('--ft-rail-offset')).toBe('0px');
  });
});

describe('constants', () => {
  it('CHEV_SVG contains an SVG element', () => {
    expect(CHEV_SVG).toContain('<svg');
    expect(CHEV_SVG).toContain('polyline');
  });
});
