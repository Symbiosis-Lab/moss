import { afterEach, describe, expect, it } from "vitest";

import { setLocale } from "../../i18n";
import { importedPagesText } from "../import-outcome";

const said = (total_pages: number, kept_pages: number) => importedPagesText({ total_pages, kept_pages });

describe("importedPagesText", () => {
  afterEach(() => setLocale("en"));

  it("names the pages written and the pages the folder already had", () => {
    expect(said(47, 0)).toBe("Imported 47 pages");
    expect(said(1, 0)).toBe("Imported 1 page");
    expect(said(3, 44)).toBe("Imported 3 pages · 44 already in the folder");
    expect(said(1, 44)).toBe("Imported 1 page · 44 already in the folder");
    expect(said(0, 47)).toBe("The folder already has all 47 pages");
    expect(said(0, 1)).toBe("The folder already has this page");
  });

  it("says the same in Chinese", () => {
    setLocale("zh-hans");
    expect(said(47, 0)).toBe("已导入 47 页");
    expect(said(3, 44)).toBe("已导入 3 页 · 文件夹里已有 44 页");
    expect(said(0, 47)).toBe("这 47 页都已在文件夹里");
    expect(said(0, 1)).toBe("这一页已在文件夹里");
  });

  it.each(["de", "zh-hant"] as const)("fills both counts in %s", (locale) => {
    setLocale(locale);
    for (const [text, counts] of [[said(3, 44), ["3", "44"]], [said(0, 47), ["47"]], [said(1, 44), ["1", "44"]]] as const) {
      expect(text).not.toContain("{");
      for (const count of counts) expect(text).toContain(count);
    }
  });

  it("reads a result without a kept count as none kept", () => {
    expect(importedPagesText({ total_pages: 47 })).toBe("Imported 47 pages");
    expect(importedPagesText({ total_pages: 0 })).toBeNull();
  });

  it("is null only when the import reached no page", () => {
    expect(said(0, 0)).toBeNull();
  });
});
