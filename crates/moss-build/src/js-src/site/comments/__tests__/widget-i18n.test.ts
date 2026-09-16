/**
 * The comment widget's client-injected status strings must follow the
 * page's `<html lang>` — including the Simplified/Traditional split.
 * Regression guard for the bug where zh-hant pages got Simplified
 * (or English) widget strings.
 */

import { describe, test, expect } from "vitest";
import { COPY, widgetCopy } from "../i18n";

function withLang(lang: string) {
  document.documentElement.lang = lang;
  return widgetCopy();
}

describe("comment widget copy resolution", () => {
  test("zh-TW resolves to Traditional", () => {
    const c = withLang("zh-TW");
    expect(c).toBe(COPY["zh-hant"]);
    expect(c.posted).toBe("已發送");
    expect(c.nameRequired).toBe("請填寫名字");
  });

  test("zh-Hant resolves to Traditional", () => {
    expect(withLang("zh-Hant")).toBe(COPY["zh-hant"]);
  });

  test("zh-CN and bare zh resolve to Simplified", () => {
    expect(withLang("zh-CN")).toBe(COPY["zh-hans"]);
    expect(withLang("zh")).toBe(COPY["zh-hans"]);
    expect(withLang("zh-CN").posted).toBe("已发送");
  });

  test("en and unknown tags resolve to English", () => {
    expect(withLang("en-US")).toBe(COPY.en);
    expect(withLang("fr")).toBe(COPY.en);
    expect(withLang("")).toBe(COPY.en);
  });

  test("all locales define every key, non-empty", () => {
    for (const locale of Object.keys(COPY) as (keyof typeof COPY)[]) {
      for (const [key, value] of Object.entries(COPY[locale])) {
        expect(value, `${locale}.${key}`).toBeTruthy();
      }
    }
  });
});
