import assert from 'node:assert/strict';
import { execFile, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { promisify } from 'node:util';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

test('locale generation preserves script and style text through HTML end tag variants', async () => {
  const root = await mkdtemp(join(tmpdir(), 'moss-locales-'));
  try {
    for (const directory of ['scripts', 'site/home', 'site/assets/brand', 'crates/moss-build/icons']) await mkdir(join(root, directory), { recursive: true });
    for (const path of ['scripts/generate-landing-locales.mjs', 'scripts/landing-copy.mjs', 'site/home/landing-i18n.js', 'crates/moss-build/icons/icon.svg']) {
      await copyFile(new URL(`../${path}`, import.meta.url), join(root, path));
    }
    const blocks = [
      '<script>const copy = "Your internet publisher is here.";</script >',
      '<script>const copy = "Your internet publisher is here.";</script\t>',
      '<script>const copy = "</style>Your internet publisher is here.";</script>',
      '<style>.probe::after { content: "Your internet publisher is here."; }</style\n>',
      '<style>.probe::after { content: "Your internet publisher is here."; }</style data-test>',
      '<script>const copy = "Your internet publisher is here.";</script/>',
      '<style>.probe::after { content: "Your internet publisher is here."; }</style/>',
      '<script>const copy = "</script-name>Your internet publisher is here.";</script>',
    ];
    const source = await readFile(new URL('../site/index.html', import.meta.url), 'utf8');
    for (const block of blocks) {
      await writeFile(join(root, 'site/index.html'), source.replace('</head>', block + '\n</head>'));
      execFileSync(process.execPath, [join(root, 'scripts/generate-landing-locales.mjs')], { stdio: 'pipe' });
      for (const locale of ['zh-hans', 'zh-hant']) {
        const output = await readFile(join(root, `site/${locale}/index.html`), 'utf8');
        assert(output.includes(block), `${locale} changed protected text in ${block}`);
        assert(output.includes(locale === 'zh-hans' ? '你的互联网发布器。' : '你的網路發布器。'), `${locale} did not translate visible text`);
      }
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

// The Chinese pages are this script's output, committed beside the English page they come from. A change
// to site/index.html that is not regenerated leaves those routes running the old page.
test('the committed Chinese landing pages are generated from the current English page', async () => {
  const root = await mkdtemp(join(tmpdir(), 'moss-locales-'));
  try {
    for (const directory of ['scripts', 'site/home', 'site/assets/brand', 'crates/moss-build/icons']) await mkdir(join(root, directory), { recursive: true });
    for (const path of ['scripts/generate-landing-locales.mjs', 'scripts/landing-copy.mjs', 'site/index.html', 'site/home/landing-i18n.js', 'crates/moss-build/icons/icon.svg']) {
      await copyFile(new URL(`../${path}`, import.meta.url), join(root, path));
    }
    execFileSync(process.execPath, [join(root, 'scripts/generate-landing-locales.mjs')], { stdio: 'pipe' });
    for (const locale of ['zh-hans', 'zh-hant']) {
      const generated = await readFile(join(root, `site/${locale}/index.html`), 'utf8');
      const committed = await readFile(new URL(`../site/${locale}/index.html`, import.meta.url), 'utf8');
      assert(generated === committed, `site/${locale}/index.html is stale; run node scripts/generate-landing-locales.mjs`);
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('the site preview check reports a homepage that is not the landing and goes on', async () => {
  const page = '<!doctype html><html lang="en"><head><link rel="stylesheet" href="/site.css"></head><body><p>Another site.</p></body></html>';
  const server = createServer((request, response) => {
    if (request.url === '/') response.writeHead(200, { 'content-type': 'text/html' }).end(page);
    else response.writeHead(404).end();
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  try {
    const url = `http://127.0.0.1:${server.address().port}/`;
    const error = await promisify(execFile)(process.execPath, [new URL('./check-site-preview.mjs', import.meta.url).pathname, url]).then(() => null, (e) => e);
    assert(error, 'a site without the landing passed the preview check');
    assert.equal(error.code, 1, error.stderr);
    const { failures } = JSON.parse(error.stdout);
    assert.deepEqual(failures.filter((f) => f.url === url).map((f) => f.problem), ['Custom landing homepage is missing'], 'one report for the homepage, not the landing\'s copy and footer checks');
    assert(failures.some((f) => f.url === `${url}site.css`), 'the asset checks still ran');
  } finally { await new Promise((done) => server.close(done)); }
});
