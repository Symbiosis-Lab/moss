import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

test('locale generation preserves script and style text through HTML end tag variants', async () => {
  const root = await mkdtemp(join(tmpdir(), 'moss-locales-'));
  try {
    for (const directory of ['scripts', 'site/home', 'site/assets/brand', 'crates/moss-build/icons']) await mkdir(join(root, directory), { recursive: true });
    for (const path of ['scripts/generate-landing-locales.mjs', 'site/home/landing-i18n.js', 'crates/moss-build/icons/icon.svg']) {
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
    for (const path of ['scripts/generate-landing-locales.mjs', 'site/index.html', 'site/home/landing-i18n.js', 'crates/moss-build/icons/icon.svg']) {
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
