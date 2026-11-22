/** Reading a masthead's first paint: hold the site script back, then total the layout shifts it causes. */

import type { Page } from "@playwright/test";

export async function settle(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

/** Delays the site script so the first frame paints before it runs, as on a slow phone. */
export async function holdScript(page: Page): Promise<void> {
  await page.route(/\/theme\.[^/]*js(\?|$)/, async (route) => {
    await new Promise((r) => setTimeout(r, 700));
    await route.continue();
  });
}

/** A theme that keeps `.nav-right` a box: an unlayered rule outranks every moss layer. */
export async function themeKeepsNavRightABox(page: Page): Promise<void> {
  await page.route(/\.css(\?|$)/, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `${await response.text()}\n.nav-right { display: flex; }\n` });
  });
}

export interface Shifts {
  total: number;
  /** Each shift, with the elements it moved: the reason a total is what it is. */
  entries: string[];
  /** Whether the first contentful paint came before the script arrived; without one there is nothing to shift. */
  paintedFirst: boolean;
}

export async function shifts(page: Page): Promise<Shifts> {
  return page.evaluate(
    () =>
      new Promise<Shifts>((resolve) => {
        const fcp = performance.getEntriesByName("first-contentful-paint")[0]?.startTime ?? Infinity;
        const script = (performance.getEntriesByType("resource") as PerformanceResourceTiming[]).find((r) => /\/theme\.[^/]*js/.test(r.name));
        let total = 0;
        const entries: string[] = [];
        type Shift = PerformanceEntry & { value: number; hadRecentInput: boolean; sources: Array<{ node?: Node; previousRect: DOMRectReadOnly; currentRect: DOMRectReadOnly }> };
        new PerformanceObserver((list) => {
          for (const entry of list.getEntries() as Shift[]) {
            if (entry.hadRecentInput) continue;
            total += entry.value;
            const moved = entry.sources.map((src) => {
              const el = src.node as Element | undefined;
              const r = (x: DOMRectReadOnly) => `${Math.round(x.x)},${Math.round(x.y)} ${Math.round(x.width)}x${Math.round(x.height)}`;
              return `${el?.className || el?.nodeName}: ${r(src.previousRect)} -> ${r(src.currentRect)}`;
            });
            entries.push(`${entry.value.toFixed(4)} @${Math.round(entry.startTime)}ms ${moved.join("; ")}`);
          }
        }).observe({ type: "layout-shift", buffered: true });
        setTimeout(() => resolve({ total, entries, paintedFirst: script !== undefined && fcp < script.responseEnd }), 300);
      }),
  );
}
