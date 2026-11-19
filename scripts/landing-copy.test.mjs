import assert from 'node:assert/strict';
import test from 'node:test';
import { sectionProblems, tryItWords } from './landing-copy.mjs';

const section = (id, heading, body) => `<section id="${id}"><h2>${heading}</h2><p>${body}</p></section>`;
const page = (override = {}) => `<!doctype html><title>t</title><header id="intro"><h1>Make the thing.</h1><p>Write in a folder.</p></header>`
  + ['c1', 'c2', 'c3', 'c4'].map((id) => override[id] ?? section(id, 'A heading', 'A sentence under the heading.')).join('')
  + `<section id="five"><h2>Free.</h2><p>Bring a site over.</p><section id="beta"><h3>Updates</h3><p>News now and then.</p></section></section>`;

test('a page whose sections carry their words in the markup has no problems', () => {
  assert.deepEqual(sectionProblems(page(), 'en'), []);
});

test('a section whose heading or text only a script, style or noscript writes is reported', () => {
  const script = '<section id="c1"><h2></h2><script>h.textContent = "A heading"</script><p>A sentence under the heading.</p></section>';
  const noscript = '<section id="c2"><h2>A heading</h2><noscript><p>A sentence under the heading.</p></noscript></section>';
  assert.deepEqual(sectionProblems(page({ c1: script, c2: noscript }), 'en'), ['#c1: no heading text', '#c2: no text under its heading']);
});

test('a missing section is reported, and nested sections are read to their own end', () => {
  assert.deepEqual(sectionProblems(page().replace(/<section id="c3">.*?<\/section>/, ''), 'en'), ['#c3: not in the HTML']);
});

test('a Chinese page with an English heading is reported', () => {
  assert.deepEqual(sectionProblems(page(), 'zh-hans').slice(0, 2), ['#intro: heading is not in zh-hans', '#c1: heading is not in zh-hans']);
});

test('the try-it words are read from the page template, entities decoded', () => {
  const html = '<div data-try-it><template data-try-it-words>\n<span data-word="folder">My site</span>\n<span data-word="refused">.md &amp; JPEG</span>\n</template></div>';
  assert.deepEqual(tryItWords(html), { folder: 'My site', refused: '.md & JPEG' });
  assert.equal(tryItWords('<p>no block</p>'), null);
});
