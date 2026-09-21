/**
 * Playwright render gate for `![[clip.mp4|loop]]` ambient video.
 *
 * Verifies (via a real Chromium render):
 *   1. `video[data-loop]` gets wrapped in `.moss-ambient-video`
 *   2. `.moss-ambient-toggle` is injected and keyboard-focusable
 *   3. Toggle click pauses the video and updates aria-label
 *   4. Under emulated `prefers-reduced-motion: reduce`:
 *      — `autoplay` attribute is removed from the video
 *      — toggle starts as play affordance ("Play video")
 *      — `.moss-ambient-video[data-paused]` is set
 *
 * The test injects a minimal inline shim (the logic from ambient-video.ts) +
 * the ambient CSS rules from site.css via page.addScriptTag. No dev server
 * or full bundle needed.
 *
 * NOTE: Browser autoplay policy blocks video in tests without a user gesture.
 * Reduced-motion tests work without actual playback; other tests mock play/pause
 * via page.evaluate.
 */

import { test, expect } from "@playwright/test";

// Minimal CSS matching what site.css emits for ambient video
const AMBIENT_CSS = `
.moss-ambient-video {
  position: relative;
  display: block;
  width: 100%;
}
video[data-loop] {
  display: block;
  width: 100%;
  height: auto;
  aspect-ratio: 16 / 9;
}
.moss-ambient-toggle {
  position: absolute;
  bottom: 0.5rem;
  right: 0.5rem;
  width: 2rem;
  height: 2rem;
  border: none;
  border-radius: 50%;
  background: rgba(0,0,0,0.45);
  color: #fff;
  cursor: pointer;
  opacity: 0;
  transition: none;
}
.moss-ambient-video:hover .moss-ambient-toggle,
.moss-ambient-video:focus-within .moss-ambient-toggle,
.moss-ambient-toggle:focus {
  opacity: 1;
}
.moss-ambient-video[data-paused] .moss-ambient-toggle {
  opacity: 1;
}
@media (prefers-reduced-motion: reduce) {
  .moss-ambient-video .moss-ambient-toggle {
    opacity: 1;
  }
}
`;

// Inline shim matching the logic in ambient-video.ts
// This is used in place of the full bundle to avoid side effects from other modules.
const AMBIENT_INIT_SHIM = `
(function() {
  var PAUSE_LABEL = "Pause video";
  var PLAY_LABEL = "Play video";
  var PAUSE_ICON = "⏸";
  var PLAY_ICON = "▶";

  function wrapAndInit(video) {
    if (video.parentElement && video.parentElement.classList.contains("moss-ambient-video")) return;
    var wrapper = document.createElement("div");
    wrapper.className = "moss-ambient-video";
    video.parentNode.insertBefore(wrapper, video);
    wrapper.appendChild(video);

    var toggle = document.createElement("button");
    toggle.className = "moss-ambient-toggle";
    toggle.type = "button";
    toggle.setAttribute("aria-label", PAUSE_LABEL);
    toggle.textContent = PAUSE_ICON;
    wrapper.appendChild(toggle);

    var prefersReduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (prefersReduced) {
      video.removeAttribute("autoplay");
      video.pause();
      toggle.setAttribute("aria-label", PLAY_LABEL);
      toggle.textContent = PLAY_ICON;
      wrapper.setAttribute("data-paused", "");
    }

    toggle.addEventListener("click", function() {
      if (video.paused) {
        video.play().catch(function() {});
        toggle.setAttribute("aria-label", PAUSE_LABEL);
        toggle.textContent = PAUSE_ICON;
        wrapper.removeAttribute("data-paused");
      } else {
        video.pause();
        toggle.setAttribute("aria-label", PLAY_LABEL);
        toggle.textContent = PLAY_ICON;
        wrapper.setAttribute("data-paused", "");
      }
    });

    video.addEventListener("play", function() {
      toggle.setAttribute("aria-label", PAUSE_LABEL);
      toggle.textContent = PAUSE_ICON;
      wrapper.removeAttribute("data-paused");
    });
    video.addEventListener("pause", function() {
      toggle.setAttribute("aria-label", PLAY_LABEL);
      toggle.textContent = PLAY_ICON;
      wrapper.setAttribute("data-paused", "");
    });
  }

  function initAmbientVideos() {
    var videos = document.querySelectorAll("video[data-loop]");
    if (!videos.length) return;
    videos.forEach(wrapAndInit);
  }

  initAmbientVideos();
})();
`;

// Fixture: video[data-loop] as the moss synthesizer emits it
const FIXTURE = `<!doctype html>
<html>
<head><meta charset="utf-8"></head>
<body>
  <article>
    <video class="moss-embed moss-embed-video" data-type="video" data-loop
           src="clip.mp4" data-placeholder-src="clip.mp4"
           poster="clip.thumb.jpg" data-thumb-src="clip.thumb.jpg"
           autoplay muted loop playsinline preload="metadata"></video>
  </article>
</body>
</html>`;

async function setupPage(page: import("@playwright/test").Page): Promise<void> {
  await page.setContent(FIXTURE);
  await page.addStyleTag({ content: AMBIENT_CSS });
  await page.addStyleTag({ content: "*,*::before,*::after{transition:none!important;animation:none!important}" });
  await page.addScriptTag({ content: AMBIENT_INIT_SHIM });
  await page.waitForSelector(".moss-ambient-video");
}

test.describe("ambient video — normal motion", () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page);
  });

  test("video is wrapped in .moss-ambient-video", async ({ page }) => {
    const wrapper = page.locator(".moss-ambient-video");
    await expect(wrapper).toHaveCount(1);
    const video = wrapper.locator("video[data-loop]");
    await expect(video).toHaveCount(1);
  });

  test(".moss-ambient-toggle is injected and keyboard-focusable", async ({ page }) => {
    const toggle = page.locator(".moss-ambient-toggle");
    await expect(toggle).toHaveCount(1);
    // Must be a <button type="button"> for native keyboard focus
    await expect(toggle).toHaveAttribute("type", "button");
    // Starts with Pause label (video is supposed to be autoplaying)
    await expect(toggle).toHaveAttribute("aria-label", "Pause video");
  });

  test("toggle click sets aria-label to 'Play video' when video is playing", async ({ page }) => {
    // Mock video.paused to return false (simulate playing)
    await page.evaluate(() => {
      const video = document.querySelector<HTMLVideoElement>("video");
      if (!video) return;
      Object.defineProperty(video, "paused", { get: () => false, configurable: true });
      video.pause = () => {
        video.dispatchEvent(new Event("pause"));
      };
    });

    const toggle = page.locator(".moss-ambient-toggle");
    await toggle.click();

    await expect(toggle).toHaveAttribute("aria-label", "Play video");

    const wrapper = page.locator(".moss-ambient-video");
    await expect(wrapper).toHaveAttribute("data-paused", "");
  });

  test("toggle text is pause icon initially (normal motion)", async ({ page }) => {
    const toggle = page.locator(".moss-ambient-toggle");
    await expect(toggle).toHaveText("⏸");
  });
});

test.describe("ambient video — prefers-reduced-motion: reduce", () => {
  test.beforeEach(async ({ page }) => {
    // emulateMedia is on page, not context, in Playwright
    await page.emulateMedia({ reducedMotion: "reduce" });
    await setupPage(page);
  });

  test("autoplay attribute is removed from video under reduced-motion", async ({ page }) => {
    const video = page.locator("video[data-loop]");
    await expect(video).not.toHaveAttribute("autoplay");
  });

  test("toggle starts as play affordance under reduced-motion", async ({ page }) => {
    const toggle = page.locator(".moss-ambient-toggle");
    await expect(toggle).toHaveAttribute("aria-label", "Play video");
  });

  test("wrapper has data-paused set under reduced-motion", async ({ page }) => {
    const wrapper = page.locator(".moss-ambient-video");
    await expect(wrapper).toHaveAttribute("data-paused", "");
  });

  test("toggle is visible without hover under reduced-motion (always-on affordance)", async ({ page }) => {
    // Under reduced motion + data-paused, the toggle should be opacity:1
    const toggle = page.locator(".moss-ambient-toggle");
    await expect(toggle).toHaveCSS("opacity", "1");
  });

  test("toggle text is play icon under reduced-motion", async ({ page }) => {
    const toggle = page.locator(".moss-ambient-toggle");
    await expect(toggle).toHaveText("▶");
  });
});
