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
    expect(box).toMatch(/border:\s*var\(--moss-menu-border\)/);
    for (const paint of ['bg', 'shadow', 'hover', 'border']) expect(tokens).toMatch(new RegExp(`\\n  --moss-menu-${paint}:`));
    expect(row).toMatch(/border-radius:\s*var\(--moss-menu-row-radius\)/);
    expect(row).toMatch(/padding:\s*var\(--moss-menu-row-padding\)/);
    expect(box + row).not.toMatch(/\d(px|rem|em)\b/);
  });

  it('muted text reads on the raised dark surface a menu is filled with', () => {
    const luminance = (hex: string) => {
      const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    for (const file of ['styles/shared/tokens.css', 'styles/editor-tokens.css']) {
      const dark = /\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(stripComments(read(file)))![1];
      const hex = (name: string) => new RegExp(`--moss-color-${name}:\\s*(#[0-9a-f]{6})`).exec(dark)![1];
      const ratio = (luminance(hex('muted')) + 0.05) / (luminance(hex('surface')) + 0.05);
      expect(ratio, file).toBeGreaterThanOrEqual(4.5);
    }
    expect(/\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(stripComments(tokens))![1]).toMatch(/--moss-menu-bg:\s*var\(--moss-color-surface\);/);
  });

  it("the address bar's menu tokens are mirrored when the page behind it turns dark or light", () => {
    // WebKit re-reads a chrome token only if the navigation manager writes it.
    const mirror = read('preview/navigation-manager.ts');
    for (const name of ['--moss-chrome-menu-bg', '--moss-chrome-menu-border']) {
      expect(mirror).toContain(`"${name}"`);
      expect(tokens.split(`${name}:`).length - 1, name).toBe(3);
    }
    // Over a dark page the list is filled with the dark surface, which the token restates as rgba.
    const dark = /\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(stripComments(tokens))![1];
    const surface = /--moss-color-surface:\s*#([0-9a-f]{6})/.exec(dark)![1];
    const [r, g, b] = [0, 2, 4].map((i) => parseInt(surface.slice(i, i + 2), 16));
    expect(tokens).toContain(`--moss-chrome-menu-bg: rgba(${r}, ${g}, ${b}, `);
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
