/**
 * Tests for FLIP-based immersive fullscreen toggle.
 *
 * The initImmersiveMode() function:
 * - Wraps an iframe in a container div with a fullscreen toggle button
 * - Enters fullscreen on button click (adds body.immersive-fs-active)
 * - Exits fullscreen on button click or ESC key
 * - Uses FLIP technique for smooth transition (tested via class assertions)
 * - Uses native Fullscreen API when available, with CSS-only fallback
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { initImmersiveMode } from "../immersive-mode";

/** jsdom doesn't have TransitionEvent -- create a minimal substitute */
function createTransitionEndEvent(propertyName: string): Event {
  const event = new Event("transitionend", { bubbles: true });
  (event as any).propertyName = propertyName;
  return event;
}

function createArticlePage(options: { hasIframe?: boolean } = {}) {
  const { hasIframe = true } = options;

  const main = document.createElement("main");
  const article = document.createElement("article");
  article.className = "container";

  const h1 = document.createElement("h1");
  h1.textContent = "Test Article";
  article.appendChild(h1);

  const desc = document.createElement("p");
  desc.textContent = "Description text above iframe";
  article.appendChild(desc);

  if (hasIframe) {
    const iframe = document.createElement("iframe");
    iframe.src = "./demo.html";
    article.appendChild(iframe);
  }

  main.appendChild(article);
  document.body.appendChild(main);

  const header = document.createElement("header");
  document.body.insertBefore(header, main);

  const footer = document.createElement("footer");
  document.body.appendChild(footer);

  return { main, article };
}

/** Flush the microtask queue so async enterFullscreen() completes */
function flushMicrotasks(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe("initImmersiveMode", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  test("wraps an article-container iframe even with no body class", () => {
    document.body.className = ""; // explicitly no immersive-ready
    document.body.innerHTML = `
      <main><article class="container">
        <iframe src="./app/index.html"></iframe>
      </article></main>`;

    initImmersiveMode();

    const wrapper = document.querySelector(
      "main > article.container > .immersive-iframe-wrapper"
    );
    expect(wrapper).not.toBeNull();
    expect(wrapper!.querySelector(":scope > iframe")).not.toBeNull();
    expect(wrapper!.querySelector(".immersive-fullscreen-btn")).not.toBeNull();
  });

  test("does nothing when the article container has no direct-child iframe", () => {
    document.body.innerHTML = `<main><article class="container"><p>text</p></article></main>`;
    initImmersiveMode();
    expect(document.querySelector(".immersive-iframe-wrapper")).toBeNull();
  });

  test("does nothing when no iframe found in article", () => {
    createArticlePage({ hasIframe: false });

    initImmersiveMode();

    expect(document.querySelector(".immersive-iframe-wrapper")).toBeNull();
    expect(document.querySelector(".immersive-fullscreen-btn")).toBeNull();
  });

  test("wraps iframe in .immersive-iframe-wrapper div", () => {
    createArticlePage();

    initImmersiveMode();

    const wrapper = document.querySelector(".immersive-iframe-wrapper");
    expect(wrapper).not.toBeNull();
    expect(wrapper!.tagName).toBe("DIV");

    const iframe = wrapper!.querySelector("iframe");
    expect(iframe).not.toBeNull();
  });

  test("creates .immersive-fullscreen-btn button inside wrapper", () => {
    createArticlePage();

    initImmersiveMode();

    const wrapper = document.querySelector(".immersive-iframe-wrapper");
    const button = wrapper!.querySelector(".immersive-fullscreen-btn");
    expect(button).not.toBeNull();
    expect(button!.tagName).toBe("BUTTON");
  });

  test("iframe src is preserved after wrapping", () => {
    createArticlePage();

    initImmersiveMode();

    const iframe = document.querySelector(".immersive-iframe-wrapper iframe") as HTMLIFrameElement;
    expect(iframe.src).toContain("demo.html");
  });

  test("button has aria-label 'Enter fullscreen'", () => {
    createArticlePage();

    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn");
    expect(button!.getAttribute("aria-label")).toBe("Enter fullscreen");
  });

  test("wraps every local direct-child iframe", () => {
    document.body.innerHTML = `
      <main><article class="container">
        <iframe src="./app-a/index.html"></iframe>
        <iframe src="../app-b/index.html"></iframe>
      </article></main>`;
    initImmersiveMode();
    const wrappers = document.querySelectorAll("main > article.container > .immersive-iframe-wrapper");
    expect(wrappers.length).toBe(2);
    wrappers.forEach(w => expect(w.querySelector(".immersive-fullscreen-btn")).not.toBeNull());
  });

  test("skips external iframes, wraps the local one even when external precedes it", () => {
    document.body.innerHTML = `
      <main><article class="container">
        <iframe src="https://cdn.example.com/embed/index.html"></iframe>
        <iframe src="../local-app/index.html"></iframe>
      </article></main>`;
    initImmersiveMode();
    const external = document.querySelector('iframe[src^="https://"]');
    const local = document.querySelector('iframe[src^="../"]');
    expect(external!.closest(".immersive-iframe-wrapper")).toBeNull();   // external untouched
    expect(local!.closest(".immersive-iframe-wrapper")).not.toBeNull();  // local wrapped
  });

  test("skips protocol-relative external iframes", () => {
    document.body.innerHTML = `<main><article class="container"><iframe src="//cdn.example.com/x.html"></iframe></article></main>`;
    initImmersiveMode();
    expect(document.querySelector(".immersive-iframe-wrapper")).toBeNull();
  });
});

describe("enter fullscreen", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  test("adds immersive-fs-active class to body on button click", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();

    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
  });

  test("changes button aria-label to 'Exit fullscreen'", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();

    expect(button.getAttribute("aria-label")).toBe("Exit fullscreen");
  });
});

describe("exit fullscreen", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
  });

  test("removes immersive-fs-active on second button click", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLButtonElement;
    // Enter fullscreen
    button.click();
    await flushMicrotasks();
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);

    // In jsdom, transitionend doesn't fire automatically, so we dispatch it
    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;

    // Complete the enter animation so the button is re-enabled (button.disabled
    // is set true inside rAF during enterFullscreen and cleared on transitionend).
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    // Exit fullscreen
    button.click();

    // The exit animation sets a transform and waits for transitionend.
    // In jsdom there are no real transitions, so we fire transitionend manually.
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    expect(document.body.classList.contains("immersive-fs-active")).toBe(false);
  });

  test("ESC key exits fullscreen", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);

    // Press ESC
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    // Fire transitionend for the exit animation
    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    expect(document.body.classList.contains("immersive-fs-active")).toBe(false);
  });

  test("ESC key does nothing when not in fullscreen", () => {
    createArticlePage();
    initImmersiveMode();

    // Should not throw or add/remove any classes
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    expect(document.body.classList.contains("immersive-fs-active")).toBe(false);
  });

  test("postMessage with moss-escape-key exits fullscreen", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);

    // Simulate postMessage from same-origin child iframe (forwarded by iframe-bridge)
    window.dispatchEvent(new MessageEvent("message", {
      data: { type: "moss-escape-key" },
      origin: window.location.origin
    }));

    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    expect(document.body.classList.contains("immersive-fs-active")).toBe(false);
  });

  test("button aria-label reverts to 'Enter fullscreen' after exit", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLButtonElement;
    button.click();
    await flushMicrotasks();

    // Complete the enter animation so the button is re-enabled
    // (button.disabled is set true inside rAF during enterFullscreen and
    // cleared on transitionend; without this the exit click is swallowed).
    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    button.click();

    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    expect(button.getAttribute("aria-label")).toBe("Enter fullscreen");
  });
});

describe("Fullscreen API integration", () => {
  let mockRequestFullscreen: ReturnType<typeof vi.fn>;
  let mockExitFullscreen: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";

    // Mock requestFullscreen on documentElement
    mockRequestFullscreen = vi.fn().mockResolvedValue(undefined);
    document.documentElement.requestFullscreen = mockRequestFullscreen;

    // Mock exitFullscreen on document
    mockExitFullscreen = vi.fn().mockResolvedValue(undefined);
    document.exitFullscreen = mockExitFullscreen;

    // Default: not in native fullscreen
    Object.defineProperty(document, "fullscreenElement", {
      value: null,
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";

    // Clean up mocks
    delete (document.documentElement as any).requestFullscreen;
    delete (document as any).exitFullscreen;
  });

  test("requestFullscreen is called on enter", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();

    expect(mockRequestFullscreen).toHaveBeenCalledTimes(1);
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
  });

  test("document.exitFullscreen is called when fullscreenElement is set", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;

    // Enter fullscreen
    button.click();
    await flushMicrotasks();
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);

    // Simulate native fullscreen being active
    (document as any).fullscreenElement = document.documentElement;

    // The enter animation's rAF may have fired by now (jsdom schedules rAF via
    // setTimeout), disabling the button until its transitionend. Fire it so the
    // next click isn't swallowed by the disabled state.
    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    // Click to exit -- should call document.exitFullscreen()
    button.click();

    expect(mockExitFullscreen).toHaveBeenCalledTimes(1);

    // State is NOT removed yet -- that happens in fullscreenchange listener
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
  });

  test("fullscreenchange event syncs state and runs exit animation", async () => {
    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;

    // Enter fullscreen
    button.click();
    await flushMicrotasks();
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);

    // Simulate native fullscreen being exited by the browser
    // (e.g. user pressed browser Esc, or exitFullscreen resolved)
    (document as any).fullscreenElement = null;
    document.dispatchEvent(new Event("fullscreenchange"));

    // The fullscreenchange listener calls runExitAnimation, which sets up
    // a transitionend listener. Fire it to complete.
    const wrapper = document.querySelector(".immersive-iframe-wrapper") as HTMLElement;
    wrapper.dispatchEvent(createTransitionEndEvent("transform"));

    expect(document.body.classList.contains("immersive-fs-active")).toBe(false);
    expect(button.getAttribute("aria-label")).toBe("Enter fullscreen");
  });

  test("graceful fallback when requestFullscreen is undefined", async () => {
    // Remove requestFullscreen to simulate older browser / cross-origin restriction
    delete (document.documentElement as any).requestFullscreen;

    createArticlePage();
    initImmersiveMode();

    const button = document.querySelector(".immersive-fullscreen-btn") as HTMLElement;
    button.click();
    await flushMicrotasks();

    // Should still enter CSS-only fullscreen without throwing
    expect(document.body.classList.contains("immersive-fs-active")).toBe(true);
    expect(button.getAttribute("aria-label")).toBe("Exit fullscreen");
  });
});
