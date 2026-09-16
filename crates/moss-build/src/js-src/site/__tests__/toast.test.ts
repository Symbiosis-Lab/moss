/**
 * Regression tests for the shared site toast copy.
 *
 * The toast text must follow the page's `<html lang>` and distinguish
 * Simplified Chinese from Traditional Chinese.
 */

import { afterEach, describe, expect, test } from "vitest";
import { showToast } from "../toast";

/** The toast fills its live region one frame after mounting it. */
const nextFrame = (): Promise<unknown> => new Promise(requestAnimationFrame);

async function renderToast(lang: string): Promise<string> {
  document.body.innerHTML = "";
  document.documentElement.lang = lang;
  showToast();
  await nextFrame();

  const toast = document.querySelector<HTMLElement>(".share-toast");
  expect(toast).not.toBeNull();
  return toast!.textContent ?? "";
}

afterEach(() => {
  document.body.innerHTML = "";
  document.documentElement.lang = "";
});

describe("showToast", () => {
  test.each(["zh-Hans", "zh-CN"])("uses Simplified Chinese copy for %s", async (lang) => {
    expect(await renderToast(lang)).toBe("已复制到剪贴板");
  });

  test.each(["zh-TW", "zh-Hant"])("uses Traditional Chinese copy for %s", async (lang) => {
    expect(await renderToast(lang)).toBe("已複製到剪貼簿");
  });

  test.each(["fr", "en-US"])("falls back to English copy for %s", async (lang) => {
    expect(await renderToast(lang)).toBe("Copied to clipboard");
  });

  // Nothing takes focus when a toast appears, so without a live region a
  // screen-reader user who copies a heading permalink is told nothing at all
  // (WCAG 4.1.3, a screen-reader announcement regression). role="status" carries an implicit polite
  // announcement, which is why no explicit aria-live is set.
  test("announces itself as a live region", async () => {
    document.body.innerHTML = "";
    showToast();

    // The region must be in the DOM before its text is, or the insertion reads
    // as a new region rather than a change to one and goes unannounced.
    const toast = document.querySelector<HTMLElement>(".share-toast");
    expect(toast?.getAttribute("role")).toBe("status");
    expect(toast?.textContent).toBe("");

    await nextFrame();
    expect(toast?.textContent).toBe("Copied to clipboard");
  });
});
