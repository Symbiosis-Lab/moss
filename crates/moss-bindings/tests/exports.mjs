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
  siteBundle() {
    const { bundle } = json(wasm.siteBundle('{"files":["index.md"],"config_toml":"[site]\\nlang = \\"de\\""}'));
    assert.equal(bundle.lang, 'de');
    assert.throws(() => wasm.siteBundle('[]'));
  },
  homeFile() {
    assert.equal(json(wasm.homeFile('["post.md","index.md"]')), 'index.md');
    assert.equal(json(wasm.homeFile('[]')), null);
  },
  siteTextPaths() {
    assert.deepEqual(json(wasm.siteTextPaths('["z.MD","child/index.md","index.md","photo.jpg"]')), ['index.md', 'z.MD']);
  },
  fieldSpans() {
    assert.equal(json(wasm.fieldSpans('---\ntitle: Example\n---\n'))[0].key, 'title');
  },
  builtinSchemaJson() {
    assert.equal(json(wasm.builtinSchemaJson()).frontmatter.fields.title.type, 'string');
  },
  resolveReferences() {
    const input = { files: ['notes/Page.md'], scope: 'site', from_source: 'index.md', targets: [{ text: 'Page#heading', is_embed: false }] };
    const [reference] = json(wasm.resolveReferences(JSON.stringify(input)));
    assert.equal(reference.resolved_path, 'notes/Page.md');
    assert.equal(reference.url, null);
    assert.equal(reference.anchor, 'heading');
  },
  resolveAsset() {
    const input = { files: ['a/photo.jpg', 'b/photo.jpg'], scope: 'site', from_source: 'post.md', targets: [] };
    assert.equal(json(wasm.resolveAsset(JSON.stringify(input), 'photo.jpg')).absolute_path, 'a/photo.jpg');
    assert.equal(json(wasm.resolveAsset(JSON.stringify(input), 'missing.png')), null);
  },
  editorScan() {
    assert.deepEqual(json(wasm.editorScan('Plain text')), { blocks: [], legacy_dash: false });
    assert.equal(json(wasm.editorScan(':::grid\nOne\n:::')).blocks.length, 1);
  },
  freeName() {
    assert.equal(wasm.freeName('photos', 'a.jpg', '["photos/a.jpg","photos/a-2.jpg","other/a-3.jpg"]'), 'a-3.jpg');
    assert.equal(wasm.freeName('', 'a.jpg', '[]'), 'a.jpg');
    assert.throws(() => wasm.freeName('', 'a.jpg', '{}'));
  },
  imageExtensions() {
    assert.deepEqual(json(wasm.imageExtensions()), ['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'avif']);
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
  Object.keys(wasm).filter(name => name !== 'default' && name !== 'initSync' && name !== '__wasm' && !name.startsWith('__wbg')).sort(),
  Object.keys(tests).sort(),
  'Every module export must have a smoke test',
);
for (const test of Object.values(tests)) test();
console.log('All WASM exports passed');
