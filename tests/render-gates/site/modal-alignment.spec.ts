import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

// Synthetic markup with production styles: this tests geometry, not native
// Settings or sign-in integration. Inline imports so no server/build is needed.
function stylesheet(path: string): string {
  return readFileSync(path, 'utf8').replace(/@import "([^";]+)";/g,
    (_, name) => stylesheet(resolve(dirname(path), name)));
}
const css = stylesheet(resolve('packages/moss-ui/src/ui/style.css'));
const consumers = [
  ['delete-confirm', 'confirm moss-modal--delete-confirm'],
  ['special-file', 'confirm moss-modal--special-file'],
  ['versions-restore', 'confirm'], ['reverify', 'confirm moss-modal--reverify'],
  ['publish', 'publish'], ['addresses', 'addresses'], ['receipt', 'receipt'],
  ['preflight', 'confirm moss-modal--publish-preflight'],
];

test('shared modal geometry across window and panel layouts', async ({ page }) => {
  for (const viewport of [{ width: 1280, height: 800 }, { width: 640, height: 600 }]) {
    for (const titlebar of [false, true]) {
      for (const panel of ['closed', 'inset', 'tiled']) {
        await test.step(`${viewport.width} titlebar=${titlebar} panel=${panel}`, async () => {
          await page.setViewportSize(viewport);
          const inset = panel === 'inset' ? 160 : 0;
          const bandWidth = viewport.width - (panel === 'tiled' ? 160 : 0);
          const topBand = titlebar ? 48 : 0;
          await page.setContent(`<!doctype html><body class="${panel === 'closed' ? '' : 'moss-action-panel-open'}"
            style="--action-panel-width:${inset}px;--preview-width-inset:${viewport.width - bandWidth}px">
            ${titlebar ? '<div id="moss-titlebar" class="moss-titlebar"></div>' : ''}
            <section class="moss-modal moss-modal--settings visible">
              <nav class="moss-settings-rail">Settings</nav>
              <main class="moss-settings-content"><h2>General</h2></main>
            </section></body>`);
          await page.addStyleTag({ content: css });
          const modal = page.locator('.moss-modal');
          await modal.evaluate(el => Promise.all(el.getAnimations().map(animation => animation.finished)));
          const box = await modal.boundingBox();
          const contentBox = await page.locator('.moss-settings-content').boundingBox();
          expect(box).not.toBeNull();
          expect(box!.y + box!.height / 2).toBeCloseTo((viewport.height + topBand) / 2, 1);
          expect(box!.x + box!.width / 2).toBeCloseTo((bandWidth + inset) / 2, 1);
          expect(box!.height).toBeCloseTo(viewport.width <= 700 ? viewport.height - topBand : 680, 1);
          expect(box!.y).toBeGreaterThanOrEqual(topBand);
          expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
          expect(box!.x).toBeGreaterThanOrEqual(inset);
          expect(box!.x + box!.width).toBeLessThanOrEqual(bandWidth);
          // Tab content can be much shorter or taller without moving the frame.
          for (const height of [20, 1800, 200]) {
            await page.locator('.moss-settings-content').evaluate((el, height) => {
              el.innerHTML = `<div style="height:${height}px">Tab content</div>`;
            }, height);
            expect(await modal.boundingBox()).toEqual(box);
            const content = await page.locator('.moss-settings-content').boundingBox();
            expect(content).toEqual(contentBox);
            expect(content!.y + content!.height).toBeLessThanOrEqual(box!.y + box!.height);
          }
          for (const [name, classes] of consumers) {
            await modal.evaluate((el, classes) => {
              el.className = `moss-modal moss-modal--${classes} visible`;
              el.innerHTML = '<div class="moss-stack"><h2 class="moss-modal-title">Confirm</h2><p>Example content</p><button>Continue</button></div>';
            }, classes);
            await modal.evaluate(el => Promise.all(el.getAnimations().map(animation => animation.finished)));
            const rect = (await modal.boundingBox())!;
            expect(rect.y + rect.height / 2, name).toBeCloseTo((viewport.height + topBand) / 2, 1);
            expect(rect.x + rect.width / 2, name).toBeCloseTo((bandWidth + inset) / 2, 1);
            expect(rect.y, name).toBeGreaterThanOrEqual(topBand);
            expect(rect.y + rect.height, name).toBeLessThanOrEqual(viewport.height);
          }
          await modal.evaluate(el => { el.className = 'moss-modal moss-modal--publish moss-modal--publish-form visible'; });
          await modal.evaluate(el => Promise.all(el.getAnimations().map(animation => animation.finished)));
          expect((await modal.boundingBox())!.y).toBeCloseTo(viewport.height * 0.12, 1);
        });
      }
    }
  }
  await test.step('short wide window constrains Settings below the titlebar', async () => {
    await page.setViewportSize({ width: 1280, height: 300 });
    await page.setContent('<!doctype html><body><div id="moss-titlebar"></div><section class="moss-modal moss-modal--settings visible"><main class="moss-settings-content">Settings</main></section></body>');
    await page.addStyleTag({ content: css });
    const modal = page.locator('.moss-modal');
    await modal.evaluate(el => Promise.all(el.getAnimations().map(animation => animation.finished)));
    const box = (await modal.boundingBox())!;
    expect(box.y).toBeCloseTo(56, 1);
    expect(box.y + box.height).toBeCloseTo(292, 1);
  });
});
