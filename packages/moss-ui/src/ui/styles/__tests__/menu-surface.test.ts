import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

/**
 * One look for every menu, dropdown and suggestion list: the `.moss-menu` box
 * and its `.moss-menu-row` rows, sized by the `--moss-menu-*` tokens. These
 * tests fail when a menu is built or styled around them.
 */

const UI = path.resolve(__dirname, '../..');
const read = (rel: string) => fs.readFileSync(path.join(UI, rel), 'utf-8');
const files = (ext: string, dir = UI, out: string[] = []): string[] => {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) { if (e.name !== '__tests__' && e.name !== 'node_modules') files(ext, p, out); }
    else if (e.name.endsWith(ext) && !e.name.endsWith('.test.ts') && !e.name.startsWith('test-setup')) out.push(path.relative(UI, p));
  }
  return out;
};
const stripComments = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');

describe('the menu surface', () => {
  const tokens = read('styles/shared/tokens.css');
  const shared = stripComments(read('styles/shared/ctx-menu.css'));

  it('a row is concentric with its menu: its radius is the menu radius less the inset', () => {
    expect(tokens).toMatch(/--moss-menu-row-radius:\s*calc\(var\(--moss-menu-radius\) - var\(--moss-menu-inset\)\);/);
    expect(tokens).toMatch(/--moss-menu-radius:\s*var\(--moss-pill-radius\);/);
  });

  it('the box and the row read the tokens and set no length of their own', () => {
    const box = /\.moss-menu,[^{]*\{([^}]*)\}/.exec(shared)![1];
    const row = /\.moss-menu-row,[^{]*\{([^}]*)\}/.exec(shared)![1];
    expect(box).toMatch(/border-radius:\s*var\(--moss-menu-radius\)/);
    expect(box).toMatch(/padding:\s*var\(--moss-menu-inset\)/);
    expect(box).toMatch(/box-shadow:\s*var\(--moss-menu-shadow\)/);
    expect(box).toMatch(/background:\s*var\(--moss-menu-bg\)/);
    // No fallback at the point of use: an undeclared paint token would leave a menu transparent.
    for (const paint of ['bg', 'shadow', 'hover']) expect(tokens).toMatch(new RegExp(`--moss-menu-${paint}:\\s*var\\(`));
    expect(row).toMatch(/border-radius:\s*var\(--moss-menu-row-radius\)/);
    expect(row).toMatch(/padding:\s*var\(--moss-menu-row-padding\)/);
    expect(box + row).not.toMatch(/\d(px|rem|em)\b/);
  });

  it('no other rule shapes or paints a menu box', () => {
    // A selector that ends on a menu's own box: `.ctx-menu`, `.omenu`, `.cm-slash-menu`, `.x-dropdown`, a popover's `.x-picker-list`.
    const MENU_BOX = /\.(?:[\w-]+-)?(?:o?menu|dropdown|picker-list)(?:--[\w-]+)?\s*$/;
    const FORBIDDEN = /(?:^|;)\s*(border-radius|box-shadow|background(?:-color)?|padding|border)\s*:/g;
    const offenders: string[] = [];
    for (const file of files('.css')) {
      for (const m of stripComments(read(file)).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        for (const selector of m[1].split(',')) {
          if (!MENU_BOX.test(selector.trim()) || /\.moss-menu\s*$/.test(selector.trim())) continue;
          for (const d of m[2].matchAll(FORBIDDEN)) offenders.push(`${file}: ${selector.trim()} { ${d[1]} }`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });

  it('whatever builds a menu or a listbox wears the shared classes', () => {
    const ROLE = /role['"]?\s*[,:=]\s*['"`](?:menu|listbox)['"`]/;
    const own = files('.ts').filter((f) => ROLE.test(read(f)) && !read(f).includes('moss-menu'));
    expect(own).toEqual([]);
  });
});
