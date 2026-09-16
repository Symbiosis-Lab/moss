// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { pickThemeFromMessage } from "../iframe-bridge-theme";

describe("pickThemeFromMessage", () => {
  it("accepts a valid set-theme message", () => {
    expect(pickThemeFromMessage({ type: "moss-set-theme", theme: "dark" })).toBe("dark");
    expect(pickThemeFromMessage({ type: "moss-set-theme", theme: "light" })).toBe("light");
  });
  it("rejects wrong type, bad theme, or junk", () => {
    expect(pickThemeFromMessage({ type: "moss-set-theme", theme: "blue" })).toBeNull();
    expect(pickThemeFromMessage({ type: "other", theme: "dark" })).toBeNull();
    expect(pickThemeFromMessage(null)).toBeNull();
    expect(pickThemeFromMessage("nope")).toBeNull();
  });
});

/**
 * Page-colour reporting — the iframe half of the shell's chrome tint.
 *
 * The regression this file exists for: the re-post after a theme flip used to
 * live in `frontend/site/link-preview.ts`, which only ships when
 * `[site].link_preview` is true. On a site with it off, the site's own moon
 * toggle changed the page's backdrop and told the shell nothing, so the shell
 * kept a stale colour — and in `navigation-manager.ts` a non-null stale colour
 * outranks the (correct) shell theme, wedging the chrome on the old pole.
 *
 * Owning the re-post here puts it in the always-injected bridge, alongside
 * chrome-ambient and site-accent, which learned the same lesson first.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { readPageColor, startPageColor } from "../iframe-bridge-theme";

/** A getComputedStyle stand-in: jsdom resolves neither `var()` nor the
 *  cascade for custom properties, so the reader's input is injected. */
function styleReader(
  rootToken: string,
  bodyBg = "rgba(0, 0, 0, 0)",
): (el: Element) => CSSStyleDeclaration {
  return (el: Element) =>
    ({
      getPropertyValue: (name: string) =>
        el === document.documentElement && name === "--moss-color-bg" ? rootToken : "",
      backgroundColor: el === document.body ? bodyBg : "rgba(0, 0, 0, 0)",
    }) as unknown as CSSStyleDeclaration;
}

const live: { stop: () => void }[] = [];

afterEach(() => {
  for (const s of live.splice(0)) s.stop();
  document.documentElement.removeAttribute("data-theme");
  vi.useRealTimers();
});

function start(post: (c: string | null) => void, read: () => string | null) {
  const handle = startPageColor(window, post, read);
  live.push(handle);
  return handle;
}

describe("readPageColor", () => {
  it("prefers the site's --moss-color-bg token", () => {
    expect(readPageColor(window, styleReader("#1c1914"))).toBe("#1c1914");
  });

  it("falls back to the body background when the token is absent", () => {
    expect(readPageColor(window, styleReader("", "rgb(250, 248, 245)"))).toBe(
      "rgb(250, 248, 245)",
    );
  });

  it("reports null when neither resolves, so the shell can clear a stale tint", () => {
    // A transparent body is not a colour — reporting it as one would paint the
    // chrome from a value that says nothing about what is actually behind it.
    expect(readPageColor(window, styleReader("", "rgba(0, 0, 0, 0)"))).toBeNull();
  });
});

describe("startPageColor", () => {
  it("posts the current backdrop on start", () => {
    const post = vi.fn();
    start(post, () => "#1c1914");
    expect(post).toHaveBeenCalledWith("#1c1914");
  });

  it("re-posts when the site flips its own theme (the moon toggle)", () => {
    // THE REGRESSION. The site's moon writes data-theme itself, so the shell's
    // push back down is a no-op and never triggered a re-read. Watching the
    // attribute catches the site's own write, the shell's push, and cold start
    // alike — the same three-writer story chrome-ambient and site-accent tell.
    const post = vi.fn();
    let color = "#1c1914";
    start(post, () => color);
    post.mockClear();

    color = "#faf8f5";
    document.documentElement.setAttribute("data-theme", "light");

    return Promise.resolve().then(() => {
      expect(post).toHaveBeenCalledWith("#faf8f5");
    });
  });

  it("stops reporting once detached", async () => {
    const post = vi.fn();
    const handle = startPageColor(window, post, () => "#1c1914");
    handle.stop();
    post.mockClear();

    document.documentElement.setAttribute("data-theme", "light");
    await Promise.resolve();

    expect(post).not.toHaveBeenCalled();
  });
});
