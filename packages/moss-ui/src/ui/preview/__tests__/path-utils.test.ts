import { describe, test, expect } from "vitest";
import { extractDisplayPath, urlPathToDisplayPath } from "../path-utils";

describe("extractDisplayPath", () => {
  // One table per function.
  // These were 23 single-assertion `test`s across seven sub-describes, all of
  // the shape "URL in, display path out"; the sub-describe headings are kept
  // as the first column because they were the only thing carrying grouping.
  test.each([
    ["full URL",          "http://localhost:8080/about.html",          "/about"],
    [".html stripped",    "http://localhost/page.html",                "/page"],
    ["no .html kept",     "http://localhost/api/data",                 "/api/data"],
    ["nested",            "http://localhost/docs/guide/intro.html",    "/docs/guide/intro"],

    ["/index.html → /",   "http://localhost/index.html",               "/"],
    ["/index → /",        "http://localhost/index",                    "/"],
    ["root",              "http://localhost/",                         "/"],
    ["empty path",        "http://localhost",                          "/"],
    ["dir index.html",    "http://localhost/research/index.html",      "/research/"],
    ["deep dir index",    "http://localhost/docs/guide/index.html",    "/docs/guide/"],
    ["dir index",         "http://localhost/research/index",           "/research/"],

    // %E4%B8%AD%E6%96%87 = 中文, %E6%97%A5%E6%9C%AC%E8%AA%9E = 日本語,
    // Korean 한국어, encoded the same way.
    ["Chinese",           "http://localhost/%E4%B8%AD%E6%96%87.html",  "/中文"],
    ["Japanese",          "http://localhost/%E6%97%A5%E6%9C%AC%E8%AA%9E.html", "/日本語"],
    ["Korean",            "http://localhost/%ED%95%9C%EA%B5%AD%EC%96%B4.html", "/한국어"],
    ["nested CJK",        "http://localhost/%E6%96%87%E6%A1%A3/%E4%B8%AD%E6%96%87.html", "/文档/中文"],

    ["%20 space",         "http://localhost/my%20page.html",           "/my page"],
    ["%20 between words", "http://localhost/hello%20world.html",       "/hello world"],
    ["escaped &",         "http://localhost/test%26demo.html",         "/test&demo"],

    ["about:blank",       "about:blank",                               "/"],
    ["blob: URL",         "blob:http://localhost/12345-67890-abcdef",  "/"],
    ["invalid URL",       "not-a-valid-url",                           "/"],
    ["empty string",      "",                                          "/"],

    ["query string",      "http://localhost/page.html?foo=bar",        "/page"],
    ["hash",              "http://localhost/page.html#section",        "/page"],
    ["mixed encoding",    "http://localhost/docs/%E6%96%87%E6%A1%A3/readme.html", "/docs/文档/readme"],
  ])("%s: %s", (_label, url, expected) => {
    expect(extractDisplayPath(url)).toBe(expected);
  });
});

describe("urlPathToDisplayPath", () => {
  test.each([
    ["root index",           "index.html",                       "/"],
    ["nested index",         "about/index.html",                 "/about/"],
    ["deep nested index",    "posts/hello-world/index.html",     "/posts/hello-world/"],

    // Already-decoded UTF-8 in, unchanged out.
    ["CJK dir + index",      "articles/2025年的珍珠/index.html", "/articles/2025年的珍珠/"],
    ["CJK filename",         "文档/中文.html",                   "/文档/中文"],

    ["non-html file",        "feed.xml",                         "/feed.xml"],
    [".html stripped",       "about.html",                       "/about"],
    ["non-html extension",   "styles.css",                       "/styles.css"],

    ["empty string",         "",                                 "/"],
    ["bare index",           "index",                            "/"],
    ["nested bare index",    "docs/guide/index",                 "/docs/guide/"],
    ["leading slash",        "/about.html",                      "/about"],
    ["leading slash + index", "/about/index.html",               "/about/"],
  ])("%s: %s", (_label, urlPath, expected) => {
    expect(urlPathToDisplayPath(urlPath)).toBe(expected);
  });
});
