#!/usr/bin/env node
// Verify the docs footer's GitHub icon: on any width, its vertical center must
// match the adjacent footer text's vertical center within 1px; on mobile
// (<=600px) it must also sit flush against the right edge of the footer's own
// content row, with the text links left in place; and its hit area (icon box
// plus the ::after inset pad) must clear the accessible 24x24 CSS px minimum.
// Screenshots of the footer region, desktop and mobile, en and zh-hans, are
// opt-in: set SCREENSHOT_DIR to a directory to save them there for visual
// review. Unset (the default), nothing is written.
import { mkdir } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const base = new URL(baseURL);
const engines = await loadPlaywright();
const engineNames = (process.env.ENGINE || 'chromium,webkit').split(',');
const screenshotDir = process.env.SCREENSHOT_DIR || null;
if (screenshotDir) await mkdir(screenshotDir, { recursive: true });

const locales = [
  { path: 'get-started/', name: 'en' },
  { path: 'zh-hans/开始使用/', name: 'zh-hans' },
  { path: 'zh-hant/開始使用/', name: 'zh-hant' },
];
const widths = [1280, 390];
const failures = [];
const results = [];

try {
for (const engineName of engineNames) {
  const browser = await engines[engineName].launch();
  try {
    for (const width of widths) {
      for (const locale of locales) {
        const page = await browser.newPage({ viewport: { width, height: 900 } });
        try {
          await page.goto(new URL(locale.path, base).href, { waitUntil: 'networkidle' });
          const data = await page.evaluate(() => {
            const icon = document.querySelector('footer.container .footer-github');
            const p = icon?.closest('p');
            const footer = document.querySelector('footer.container');
            if (!icon || !p || !footer) return null;
            const iconRect = icon.getBoundingClientRect();
            const footerRect = footer.getBoundingClientRect();
            const footerStyle = getComputedStyle(footer);
            const contentRight = footerRect.right - parseFloat(footerStyle.paddingRight || '0');
            const textAnchor = p.querySelector('a:not(.footer-github)');
            const textRect = textAnchor.getBoundingClientRect();
            const afterInset = getComputedStyle(icon, '::after').inset;
            const insetPx = parseFloat(afterInset) || 0; // negative value; e.g. "-4px"
            return {
              icon: { top: iconRect.top, bottom: iconRect.bottom, left: iconRect.left, right: iconRect.right, width: iconRect.width, height: iconRect.height },
              textCenterY: (textRect.top + textRect.bottom) / 2,
              textLeft: textRect.left,
              contentRight,
              hitWidth: iconRect.width - 2 * insetPx,
              hitHeight: iconRect.height - 2 * insetPx,
            };
          });
          if (!data) {
            failures.push({ engine: engineName, width, locale: locale.name, problem: 'Footer GitHub icon, its paragraph, or its text sibling not found' });
            continue;
          }
          const iconCenterY = (data.icon.top + data.icon.bottom) / 2;
          const centerDelta = Math.abs(iconCenterY - data.textCenterY);
          const rightEdgeDelta = Math.abs(data.icon.right - data.contentRight);
          const mobile = width <= 600;
          results.push({ engine: engineName, width, locale: locale.name, centerDelta: +centerDelta.toFixed(2), rightEdgeDelta: mobile ? +rightEdgeDelta.toFixed(2) : null, hitWidth: +data.hitWidth.toFixed(2), hitHeight: +data.hitHeight.toFixed(2) });
          if (centerDelta > 1) failures.push({ engine: engineName, width, locale: locale.name, problem: `Icon vertical center off by ${centerDelta.toFixed(2)}px (> 1px)` });
          if (mobile) {
            if (rightEdgeDelta > 1) failures.push({ engine: engineName, width, locale: locale.name, problem: `Icon right edge off the footer's content edge by ${rightEdgeDelta.toFixed(2)}px (> 1px)` });
            if (data.icon.left <= data.textLeft) failures.push({ engine: engineName, width, locale: locale.name, problem: 'Icon is not to the right of the footer text' });
          }
          if (data.hitWidth < 24 || data.hitHeight < 24) failures.push({ engine: engineName, width, locale: locale.name, problem: `Hit area ${data.hitWidth.toFixed(1)}x${data.hitHeight.toFixed(1)} is under the 24x24 minimum` });
          if (screenshotDir && (engineName === 'chromium') && (locale.name === 'en' || locale.name === 'zh-hans')) {
            const footer = page.locator('footer.container');
            const file = `${screenshotDir}/footer-${locale.name}-${mobile ? 'mobile' : 'desktop'}.png`;
            await footer.screenshot({ path: file });
          }
        } finally {
          await page.close();
        }
      }
    }
  } finally {
    await browser.close();
  }
}
} finally { await close(); }

console.log(JSON.stringify({ base: base.href, results, failures }, null, 2));
process.exitCode = failures.length ? 1 : 0;
