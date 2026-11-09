/**
 * One token carries the window's corner: --moss-window-radius, which the desktop
 * sets per platform (24px macOS, 8px Windows and Linux). A static twin would show
 * macOS's corner on Windows. Small control radii do not derive from it: a sixth
 * of 8px is 1.3px, which reads as square.
 */
import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

const SRC = path.resolve(import.meta.dirname, '../../..');
const read = (rel: string) => fs.readFileSync(path.join(SRC, rel), 'utf8');

const sources = (dir: string): string[] =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return e.name === '__tests__' ? [] : sources(p);
    return /\.(css|ts)$/.test(e.name) ? [p] : [];
  });

describe('window corner token', () => {
  it('has no static twin', () => {
    const readers = sources(SRC).filter((f) => fs.readFileSync(f, 'utf8').includes('--moss-radius-window'));
    expect(readers.map((f) => path.relative(SRC, f))).toEqual([]);
  });

  it('defaults to 24px for carriers with no init script', () => {
    expect(read('ui/styles/shared/tokens.css')).toMatch(/--moss-window-radius:\s*24px;/);
  });

  it('does not size the context menu check box or chevron', () => {
    expect(read('ui/styles/shared/ctx-menu.css')).not.toMatch(/--moss-window-radius/);
  });
});
