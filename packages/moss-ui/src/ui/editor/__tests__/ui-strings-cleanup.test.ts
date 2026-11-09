import { test, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

test('no mode.* keys remain in ui-strings.ts', () => {
  const src = readFileSync((import.meta.dirname + "/../../i18n/ui-strings.ts"), 'utf8');
  expect(src).not.toMatch(/'mode\.\w+':/);
});
