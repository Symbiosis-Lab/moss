import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

const css = fs.readFileSync(
  path.resolve(__dirname, '../shared/primitives.css'),
  'utf-8',
);

function rule(selector: string): string {
  // Match ".class { ... }" (greedy match to closing brace of that block)
  const re = new RegExp(`${selector.replace(/[-/.*+?^$()|[\\]{}]/g, '\\$&')}\\s*\\{([^}]*)\\}`);
  const match = css.match(re);
  if (!match) throw new Error(`No rule found for ${selector}`);
  return match[1];
}

describe('pill primitives', () => {
  it('.moss-pill-input is 36px tall, 18px radius, glass-pill surface', () => {
    const body = rule('\\.moss-pill-input');
    expect(body).toMatch(/height:\s*36px/);
    expect(body).toMatch(/border-radius:\s*18px/);
    expect(body).toMatch(/background:\s*var\(--moss-glass-pill-bg\)/);
  });

  it('.moss-pill-combo has caret space on the right', () => {
    const body = rule('\\.moss-pill-combo');
    expect(body).toMatch(/height:\s*36px/);
    expect(body).toMatch(/border-radius:\s*18px/);
    expect(body).toMatch(/padding:\s*0\s+36px\s+0\s+18px/);
  });

  it('.moss-select--pill is a self-sufficient glass pill (no --boxed dependency)', () => {
    const body = rule('\\.moss-select--pill(?!:)');
    expect(body).toMatch(/height:\s*36px/);
    expect(body).toMatch(/border-radius:\s*18px/);
    expect(body).toMatch(/background-color:\s*var\(--moss-glass-pill-bg\)/);
    expect(body).toMatch(/padding:\s*0\s+36px\s+0\s+18px/);
    // background shorthand would clobber .moss-select's chevron image
    expect(body).not.toMatch(/background:\s/);
  });

  it('.moss-pill-btn--primary is filled pill', () => {
    const body = rule('\\.moss-pill-btn--primary');
    expect(body).toMatch(/height:\s*36px/);
    expect(body).toMatch(/border-radius:\s*18px/);
    expect(body).toMatch(/background:\s*var\(--moss-pill-filled-bg\)/);
    expect(body).toMatch(/color:\s*var\(--moss-pill-filled-text\)/);
  });

  it('skeleton rows stagger their pulse animation to avoid a strobe effect', () => {
    // At least one animation-delay must appear on .moss-skeleton-row or
    // .moss-skeleton-bar selectors. This ensures the strobe-pulse fix is present.
    expect(css).toMatch(/\.moss-(pill-dropdown-skeleton|skeleton-bar)[^{]*\{[^}]*animation-delay:/);
  });
});
