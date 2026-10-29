import { expect, type Page } from "@playwright/test";

/**
 * Until the page renders frames again. WebKit draws a large canvas bake (the
 * places explorer's world map, its tiles after a fly-in) on its GPU process and
 * renders no frame until the bake lands: a few seconds at a device scale of 1,
 * about ten at 2. A click made inside that window waits for a stable frame that
 * never comes, and a screenshot catches canvases that have not drawn yet.
 */
export async function untilRendering(page: Page): Promise<void> {
  await expect
    .poll(
      () =>
        page.evaluate(
          () =>
            new Promise<boolean>((resolve) => {
              let frames = 0;
              const tick = (): void => {
                if (++frames === 3) resolve(true);
                else requestAnimationFrame(tick);
              };
              requestAnimationFrame(tick);
              setTimeout(() => resolve(false), 500);
            }),
        ),
      { timeout: 30000, intervals: [0] },
    )
    .toBe(true);
}
