import assert from 'node:assert/strict';
import test from 'node:test';
import { STATIC_COPY, missingStaticCopy } from './landing-copy.mjs';

const catalog = () => Object.fromEntries(STATIC_COPY.map((key) => [key, `copy for ${key}`]));
const pageWith = (strings) => `<!doctype html><script>if (x === undefined) {}</script>${strings.join('\n')}`;

test('a catalog that lacks a listed key throws, naming the key', () => {
  const lacking = catalog();
  delete lacking.intro;
  const everything = pageWith(Object.values(lacking));
  assert.throws(() => missingStaticCopy(everything, lacking), /"intro"/);
});

test('a string present only in escaped form counts as present', () => {
  const copy = { ...catalog(), title: `Tom & Jerry's "show"` };
  const html = pageWith(Object.values({ ...copy, title: 'Tom &amp; Jerry&#39;s &quot;show&quot;' }));
  assert.deepEqual(missingStaticCopy(html, copy), []);
});

test('a string the page lacks is reported', () => {
  const copy = catalog();
  const html = pageWith(Object.entries(copy).filter(([key]) => key !== 'privacy').map(([, value]) => value));
  assert.deepEqual(missingStaticCopy(html, copy), ['privacy']);
});
