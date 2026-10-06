#!/usr/bin/env node
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const server = await resolveBaseURL(process.argv[2], { root: fileURLToPath(new URL('../site/', import.meta.url)) });
const { chromium, webkit } = await loadPlaywright();
try {
  for (const [name, engine] of [['chromium', chromium], ['webkit', webkit]]) {
    const browser = await engine.launch();
    try {
      const page = await browser.newPage();
      await page.route('**/*', route => new URL(route.request().url()).origin === new URL(server.baseURL).origin ? route.continue() : route.abort());
      const hostile = ['javascript:parent.__previewExecuted=true', 'data:text/html,<script>parent.__previewExecuted=true</script>', '//attacker.invalid/', 'https://attacker.invalid/', 'http://['];
      for (const preview of hostile) {
        await page.goto(new URL(`ui/shell.html?preview=${encodeURIComponent(preview)}`, server.baseURL).href);
        await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
        assert.equal(await page.evaluate(() => window.__previewExecuted), undefined, `${name}: query preview executed script`);
        assert.equal(await page.locator('iframe').getAttribute('src'), null, `${name}: query preview accepted ${preview}`);
      }
      await page.evaluate(() => window.__shell.setPreview('/assets/brand/favicon.svg'));
      await page.waitForFunction(() => document.querySelector('iframe').contentDocument?.documentElement.localName === 'svg');
      const validPreview = new URL('/assets/brand/favicon.svg', server.baseURL).href;
      assert.equal(await page.locator('iframe').getAttribute('src'), validPreview);
      for (const preview of [...hostile, validPreview.replace('://', '://user:pass@')]) {
        await page.evaluate(src => window.__shell.setPreview(src), preview);
        assert.equal(await page.locator('iframe').getAttribute('src'), validPreview, `${name}: API preview accepted ${preview}`);
      }
      assert.equal(await page.evaluate(() => window.__previewExecuted), undefined);

      const examples = [['blakesnotebook.com', 'https://www.blakesnotebook.com/'], ['zhudasnotebook.com', 'https://www.zhudasnotebook.com/'], ['attacker.invalid', new URL('/', server.baseURL).href]];
      for (const [target, expected] of examples) {
        const receipt = async (path) => {
          const params = new URLSearchParams({ target, live: 'https://attacker.invalid/' });
          if (path !== undefined) params.set('pages', JSON.stringify([{ kind: 'added', path, title: 'A page' }]));
          await page.goto(new URL(`ui/shell.html?${params}`, server.baseURL).href);
          await page.waitForFunction(() => window.__shell);
          await page.addScriptTag({ url: new URL('landing-shell.js', server.baseURL).href });
          await page.evaluate(added => {
            window.__opened = [];
            window.open = href => { window.__opened.push(href); return null; };
            window.__shell.setPending({ added });
          }, path === undefined ? 0 : 1);
          await page.locator('.moss-publish-button').click();
          await page.locator('.moss-modal-actions .moss-btn-primary').click();
          return page.evaluate(() => window.__opened);
        };
        assert.deepEqual(await receipt(), [expected], `${name}: receipt trusted live override`);
        assert.deepEqual(await receipt('/about/'), [new URL('/about/', expected).href], `${name}: known page destination changed`);
        for (const path of ['//attacker.invalid/', '\\\\attacker.invalid/', 'javascript:window.__previewExecuted=true', expected.replace('://', '://user:pass@'), 'http://[']) {
          assert.deepEqual(await receipt(path), [expected], `${name}: receipt accepted ${path}`);
        }
      }
      await page.close();
      console.log(`${name}: preview URLs stay local and receipts use the displayed example site's origin`);
    } finally { await browser.close(); }
  }
} finally { await server.close(); }
