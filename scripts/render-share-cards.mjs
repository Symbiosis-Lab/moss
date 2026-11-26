// Renders the landing's share cards (og:image) from the hero itself, one per
// language, so a card never drifts from the page a shared link opens. Run it
// after the hero's words change:
//
//   target/debug/moss-cli build site && node scripts/render-share-cards.mjs
//
// With no argument it serves the compiled build; a URL renders a deployed site.
import { join } from 'node:path';
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';

const OUT = new URL('../site/assets/brand/', import.meta.url).pathname;
const CARDS = [['', 'share-en.png'], ['zh-hans/', 'share-zh-hans.png'], ['zh-hant/', 'share-zh-hant.png']];
// Only the hero and the mark: the stage and the copy below it are cut, and the
// pair is lowered to sit in the middle of a 1200x630 card.
const CARD_STYLE = '.language-picker, .page, #col { opacity: 0 !important; } #intro .intro-in { top: 150px !important; }';

const { chromium } = await loadPlaywright();
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const browser = await chromium.launch();
try {
  for (const [path, file] of CARDS) {
    const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
    await page.goto(new URL(path, baseURL).href, { waitUntil: 'load' });
    await whenReady(page);
    await page.addStyleTag({ content: CARD_STYLE });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: join(OUT, file) });
    console.log(`wrote site/assets/brand/${file}`);
    await page.close();
  }
} finally {
  await browser.close();
  await close();
}
