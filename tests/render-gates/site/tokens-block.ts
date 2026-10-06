// The site's `@layer tokens` block, built from tokens.json the way
// crates/moss-build/src/build/emit/stylesheet.rs builds it: every token's light
// value in `:root`, and the themed ones again in `[data-theme="dark"]`. A spec
// that injects site.css through `page.setContent` needs this in front of it,
// and reading the contract file means the next token needs no hand edit here.
import fs from 'node:fs';
import path from 'node:path';
import { openCrateDir } from '../../support/crate-paths';

const TOKENS = JSON.parse(
  fs.readFileSync(path.join(openCrateDir('moss-core'), 'src/contract/tokens.json'), 'utf8'));

/** @param {'light'|'dark'} theme */
export function tokenBlock(theme) {
  const lines = [];
  for (const [group, entries] of Object.entries(TOKENS)) {
    if (group.startsWith('$')) continue;
    for (const [name, token] of Object.entries(entries)) {
      if (name.startsWith('$')) continue;
      const value = token.$value;
      if (typeof value === 'string') { if (theme === 'light') lines.push(`--${name}: ${value};`); }
      else if (value[theme] != null) lines.push(`--${name}: ${value[theme]};`);
    }
  }
  return lines.join('\n');
}

export const TOKEN_CSS = `:root{\n${tokenBlock('light')}\n}\n[data-theme="dark"]{\n${tokenBlock('dark')}\n}`;
