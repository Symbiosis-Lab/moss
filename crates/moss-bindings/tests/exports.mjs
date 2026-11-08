import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

assert.ok(process.argv[2], 'Usage: node exports.mjs <wasm-pack output directory>');
const dir = resolve(process.argv[2]);
const wasm = await import(pathToFileURL(resolve(dir, 'moss_bindings.js')).href);
wasm.initSync({ module: readFileSync(resolve(dir, 'moss_bindings_bg.wasm')) });
const json = JSON.parse;
const text = '---\ntitle: Example\n---\nBody';

const tests = {
  MossSite() {
    const site = new wasm.MossSite('{"files":["a.md","b.md"],"lang":"en"}');
    try {
      assert.equal(json(site.check('a.md', '---\ndraft: [bad]\n---\n'))[0].severity, 1);
      assert.equal(json(site.embeddedNotes('a.md', '![[b]]'))[0], 'b.md');
      site.setNotes('{"b.md":"Embedded **words**"}');
      assert.ok(json(site.preview('a.md', '![[b]]', '/')).html.includes('Embedded <strong>words</strong>'));
      assert.equal(json(site.preview('a.md', text, '/')).title, 'Example');
    } finally {
      site.free();
    }
  },
  builtinSchemaJson() {
    assert.equal(json(wasm.builtinSchemaJson()).frontmatter.fields.title.type, 'string');
  },
  buildTree() {
    const tree = json(wasm.buildTree('[{"path":"index.md","is_dir":false}]', '{"root_name":"Site"}'));
    assert.equal(tree.children[0].is_home, true);
  },
  decideSave() {
    assert.deepEqual(json(wasm.decideSave(undefined, 'old-hash', '{}', 'new')), { status: 'gone' });
    assert.equal(json(wasm.decideSave(undefined, undefined, '{}', 'new')).status, 'write');
  },
  parseFrontmatter() {
    assert.equal(json(wasm.parseFrontmatter(text)).frontmatter.title, 'Example');
  },
  describeSourceRole() {
    assert.equal(json(wasm.describeSourceRole('footer.md')).slot, 'footer-left');
  },
  computeHeadingState() {
    assert.equal(json(wasm.computeHeadingState('a.md', '{"title":"Example"}', 'Body')).text, 'Example');
  },
  bootstrapSourcePath() {
    assert.equal(json(wasm.bootstrapSourcePath('site', 'a.md')), 'site/a.md');
  },
};

assert.deepEqual(
  Object.keys(wasm).filter(name => name !== 'default' && name !== 'initSync' && !name.startsWith('__wbg')).sort(),
  Object.keys(tests).sort(),
  'Every module export must have a smoke test',
);
for (const test of Object.values(tests)) test();
console.log('All WASM exports passed');
