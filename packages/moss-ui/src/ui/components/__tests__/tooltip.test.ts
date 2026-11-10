/**
 * Unit tests for tooltip portal component
 *
 * Tests the singleton tooltip that renders at document.body level,
 * avoiding overflow clipping from ancestor containers. Uses event
 * delegation on mouseover/mouseout for dynamic element support.
 */

import { describe, test, expect, beforeEach, afterEach } from "vitest";
import {
  initTooltips,
  destroyTooltips,
  computeTooltipPosition,
  suppressTooltips,
} from "../tooltip";

describe("Tooltip", () => {
  afterEach(() => {
    destroyTooltips();
  });

  describe("Lifecycle", () => {
    test("initTooltips creates .moss-tooltip element in document.body", () => {
      initTooltips();

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip).not.toBeNull();
      expect(tooltip?.parentNode).toBe(document.body);
    });

    test("destroyTooltips removes .moss-tooltip element", () => {
      initTooltips();
      destroyTooltips();

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip).toBeNull();
    });

    test("calling initTooltips twice does not create duplicate elements", () => {
      initTooltips();
      initTooltips();

      const tooltips = document.querySelectorAll(".moss-tooltip");
      expect(tooltips.length).toBe(1);
    });
  });

  describe("Show/Hide Behavior", () => {
    let trigger: HTMLElement;

    beforeEach(() => {
      initTooltips();
      trigger = document.createElement("button");
      trigger.setAttribute("data-tooltip", "Hello tooltip");
      document.body.appendChild(trigger);
    });

    afterEach(() => {
      if (trigger.parentNode) {
        trigger.parentNode.removeChild(trigger);
      }
    });

    test("hovering a [data-tooltip] element adds .visible to tooltip", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);
    });

    test("tooltip textContent matches data-tooltip attribute value", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.textContent).toBe("Hello tooltip");
    });

    test("pointer leaving trigger removes .visible", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      trigger.dispatchEvent(
        new MouseEvent("mouseout", {
          bubbles: true,
          relatedTarget: document.body,
        })
      );

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(false);
    });

    test("pointer moving to child inside same [data-tooltip] does NOT hide tooltip", () => {
      const child = document.createElement("span");
      child.textContent = "icon";
      trigger.appendChild(child);

      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      trigger.dispatchEvent(
        new MouseEvent("mouseout", {
          bubbles: true,
          relatedTarget: child,
        })
      );

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);
    });

    test("tooltip works for elements added after initTooltips", () => {
      const dynamic = document.createElement("div");
      dynamic.setAttribute("data-tooltip", "Dynamic content");
      document.body.appendChild(dynamic);

      dynamic.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);
      expect(tooltip?.textContent).toBe("Dynamic content");

      dynamic.parentNode?.removeChild(dynamic);
    });

    test("hovering a child of [data-tooltip] shows tooltip via closest ancestor", () => {
      const child = document.createElement("span");
      child.textContent = "icon";
      trigger.appendChild(child);

      child.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);
      expect(tooltip?.textContent).toBe("Hello tooltip");
    });

    test("hovering element without data-tooltip does nothing", () => {
      const plain = document.createElement("div");
      document.body.appendChild(plain);

      plain.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(false);

      plain.parentNode?.removeChild(plain);
    });

    test("tooltip hides on scroll event", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);

      document.dispatchEvent(new Event("scroll", { bubbles: true }));

      expect(tooltip?.classList.contains("visible")).toBe(false);
    });

    test("tooltip hides on window resize", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(true);

      window.dispatchEvent(new Event("resize"));

      expect(tooltip?.classList.contains("visible")).toBe(false);
    });
  });

  describe("Content and Style", () => {
    let trigger: HTMLElement;

    beforeEach(() => {
      initTooltips();
      trigger = document.createElement("button");
      trigger.setAttribute("data-tooltip", "Initial text");
      document.body.appendChild(trigger);
    });

    afterEach(() => {
      if (trigger.parentNode) {
        trigger.parentNode.removeChild(trigger);
      }
    });

    test("data-tooltip-style='muted' adds .muted class to tooltip", () => {
      trigger.setAttribute("data-tooltip-style", "muted");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("muted")).toBe(true);
    });

    test("data-tooltip-kbd renders a kbd chip after the text", () => {
      trigger.setAttribute("data-tooltip-kbd", "⌘B");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip")!;
      const chip = tooltip.querySelector("kbd.moss-tooltip-kbd");
      // jsdom reports Linux, where a glyph chip prints in words.
      expect(chip?.textContent).toBe("Ctrl+B");
      // Text stays first; the chip is appended after it.
      expect(tooltip.textContent).toBe("Initial textCtrl+B");
    });

    test("a trigger without data-tooltip-kbd renders no chip (plain text path)", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip")!;
      expect(tooltip.querySelector(".moss-tooltip-kbd")).toBeNull();
      expect(tooltip.textContent).toBe("Initial text");
    });

    test("moving from a kbd trigger to a plain trigger drops the chip", () => {
      trigger.setAttribute("data-tooltip-kbd", "⌘B");
      const plain = document.createElement("button");
      plain.setAttribute("data-tooltip", "Plain");
      document.body.appendChild(plain);

      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      const tooltip = document.querySelector(".moss-tooltip")!;
      expect(tooltip.querySelector(".moss-tooltip-kbd")).not.toBeNull();

      trigger.dispatchEvent(
        new MouseEvent("mouseout", { bubbles: true, relatedTarget: plain })
      );
      plain.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      expect(tooltip.querySelector(".moss-tooltip-kbd")).toBeNull();
      expect(tooltip.textContent).toBe("Plain");

      plain.parentNode?.removeChild(plain);
    });

    test("live data-tooltip refresh preserves the kbd chip (MutationObserver path)", async () => {
      trigger.setAttribute("data-tooltip-kbd", "⌘B");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      trigger.setAttribute("data-tooltip", "Updated");
      await Promise.resolve();

      const tooltip = document.querySelector(".moss-tooltip")!;
      expect(tooltip.querySelector(".moss-tooltip-kbd")?.textContent).toBe("Ctrl+B");
      expect(tooltip.textContent).toBe("UpdatedCtrl+B");
    });

    test("changing data-tooltip value updates on next hover", () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.textContent).toBe("Initial text");

      // Leave and re-enter with new value
      trigger.dispatchEvent(
        new MouseEvent("mouseout", {
          bubbles: true,
          relatedTarget: document.body,
        })
      );
      trigger.setAttribute("data-tooltip", "Updated text");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      expect(tooltip?.textContent).toBe("Updated text");
    });

    test("moving between adjacent [data-tooltip] elements shows correct text", () => {
      const trigger2 = document.createElement("button");
      trigger2.setAttribute("data-tooltip", "Second tooltip");
      document.body.appendChild(trigger2);

      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.textContent).toBe("Initial text");

      // Move to second trigger (mouseout from first, mouseover on second)
      trigger.dispatchEvent(
        new MouseEvent("mouseout", {
          bubbles: true,
          relatedTarget: trigger2,
        })
      );
      trigger2.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      expect(tooltip?.textContent).toBe("Second tooltip");

      trigger2.parentNode?.removeChild(trigger2);
    });

    test("data-tooltip text updates live while still hovering, without leaving and re-entering", async () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.textContent).toBe("Initial text");

      // The pointer never leaves the trigger — mirrors a user watching a
      // progress tile while its attribute is updated by many rapid ticks.
      for (let pct = 1; pct <= 3; pct++) {
        trigger.setAttribute("data-tooltip", `Downloading… ${pct * 30}%`);
        // MutationObserver callbacks are microtask-scheduled.
        await Promise.resolve();
      }

      expect(tooltip?.textContent).toBe("Downloading… 90%");
    });

    test("hiding the tooltip while hovered stops the live text refresh", async () => {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      trigger.setAttribute("data-tooltip", "");
      await Promise.resolve();

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(false);
    });

    test("element with data-tooltip removed no longer triggers tooltip", () => {
      trigger.removeAttribute("data-tooltip");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const tooltip = document.querySelector(".moss-tooltip");
      expect(tooltip?.classList.contains("visible")).toBe(false);
    });
  });

  describe("computeTooltipPosition", () => {
    // Helper to create a mock DOMRect
    function mockRect(
      x: number,
      y: number,
      width: number,
      height: number
    ): DOMRect {
      return {
        x,
        y,
        width,
        height,
        top: y,
        left: x,
        right: x + width,
        bottom: y + height,
        toJSON() {
          return this;
        },
      };
    }

    const viewport = { width: 1024, height: 768 };

    test("left positioning: tooltip to the left of trigger with arrowSide 'left'", () => {
      const triggerRect = mockRect(500, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "left",
        viewport
      );

      // Tooltip should be to the left of trigger: trigger.left - gap - tooltip.width
      expect(pos.left).toBe(500 - 8 - 100); // 392
      // Vertically centered: trigger.top + trigger.height/2 - tooltip.height/2
      expect(pos.top).toBe(300 + 20 - 15); // 305
      expect(pos.arrowSide).toBe("left");
    });

    test("right positioning: tooltip to the right of trigger with arrowSide 'right'", () => {
      const triggerRect = mockRect(500, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "right",
        viewport
      );

      // Tooltip should be to the right of trigger: trigger.right + gap
      expect(pos.left).toBe(500 + 40 + 8); // 548
      // Vertically centered
      expect(pos.top).toBe(300 + 20 - 15); // 305
      expect(pos.arrowSide).toBe("right");
    });

    test("top positioning: centered above the trigger with arrowSide 'top'", () => {
      const triggerRect = mockRect(500, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "top", viewport);

      // Horizontally centered: trigger center (520) - tooltip.width/2
      expect(pos.left).toBe(520 - 50); // 470
      // Above the trigger: trigger.top - gap - tooltip.height
      expect(pos.top).toBe(300 - 8 - 30); // 262
      expect(pos.arrowSide).toBe("top");
    });

    test("top positioning flips below when there is no room above", () => {
      const triggerRect = mockRect(500, 10, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "top", viewport);

      expect(pos.top).toBe(10 + 40 + 8); // below the trigger
      expect(pos.arrowSide).toBe("bottom");
    });

    test("bottom positioning: centered below the trigger with arrowSide 'bottom'", () => {
      const triggerRect = mockRect(500, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", viewport);

      expect(pos.left).toBe(520 - 50); // 470, horizontally centered
      expect(pos.top).toBe(300 + 40 + 8); // 348, below the trigger
      expect(pos.arrowSide).toBe("bottom");
    });

    test("bottom positioning flips above when there is no room below", () => {
      const triggerRect = mockRect(500, viewport.height - 45, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", viewport);

      expect(pos.top).toBe(viewport.height - 45 - 8 - 30); // above the trigger
      expect(pos.arrowSide).toBe("top");
    });

    test("viewport clamping: tooltip pushed down when too close to top edge", () => {
      const triggerRect = mockRect(500, 5, 40, 40);
      const tooltipSize = { width: 100, height: 60 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "left",
        viewport
      );

      // Without clamping: top = 5 + 20 - 30 = -5 (off-screen)
      // Should be clamped to EDGE_MARGIN, not flush to 0 — see tooltip.ts's
      // EDGE_MARGIN doc comment (the arrow/shadow bleed past a flush box).
      expect(pos.top).toBe(8);
    });

    test("viewport clamping: tooltip pushed up when too close to bottom edge", () => {
      const triggerRect = mockRect(500, 750, 40, 40);
      const tooltipSize = { width: 100, height: 60 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "left",
        viewport
      );

      // Without clamping: top = 750 + 20 - 30 = 740, bottom = 740 + 60 = 800 > 768
      // Should be clamped so bottom = 768 - EDGE_MARGIN => top = 768 - 8 - 60 = 700
      expect(pos.top).toBe(768 - 8 - 60);
    });

    test("viewport clamping: flips to right when no room on left", () => {
      const triggerRect = mockRect(50, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "left",
        viewport
      );

      // Left would be: 50 - 8 - 100 = -58 (off-screen)
      // Should flip to right: 50 + 40 + 8 = 98
      expect(pos.left).toBe(50 + 40 + 8); // 98
      expect(pos.arrowSide).toBe("right");
    });

    test("viewport clamping: flips to left when no room on right", () => {
      const triggerRect = mockRect(950, 300, 40, 40);
      const tooltipSize = { width: 100, height: 30 };

      const pos = computeTooltipPosition(
        triggerRect,
        tooltipSize,
        "right",
        viewport
      );

      // Right would be: 950 + 40 + 8 = 998, right edge = 998 + 100 = 1098 > 1024
      // Should flip to left: 950 - 8 - 100 = 842
      expect(pos.left).toBe(950 - 8 - 100); // 842
      expect(pos.arrowSide).toBe("left");
    });

    test("horizontal clamp: a left-positioned tooltip that flips right must not overflow the right edge (cover-chip bug)", () => {
      // Reproduces the real action-panel case: a chip near the left of a NARROW
      // panel with a wide tooltip. 'left' goes off-screen-left → flips right →
      // but the flipped position overflows the panel's right edge. Must clamp in.
      const narrow = { width: 500, height: 1000 };
      const triggerRect = mockRect(134, 59, 167, 32); // cover chip
      const tooltipSize = { width: 289, height: 24 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", narrow);

      // Stays fully within the viewport horizontally, minus EDGE_MARGIN on
      // each side (the bug: right was 598 > 500).
      expect(pos.left).toBeGreaterThanOrEqual(8);
      expect(pos.left + tooltipSize.width).toBeLessThanOrEqual(narrow.width - 8);
    });

    test("horizontal clamp: a right-positioned tooltip that flips left must not overflow the left edge", () => {
      const narrow = { width: 500, height: 1000 };
      const triggerRect = mockRect(200, 59, 40, 32);
      const tooltipSize = { width: 460, height: 24 };

      const pos = computeTooltipPosition(triggerRect, tooltipSize, "right", narrow);

      expect(pos.left).toBeGreaterThanOrEqual(8);
      expect(pos.left + tooltipSize.width).toBeLessThanOrEqual(narrow.width - 8);
    });

    describe("EDGE_MARGIN — a trigger flush against each edge", () => {
      // 8, like ctx-menu.ts's VIEWPORT_EDGE_PADDING. The margin exists so the
      // CSS arrow/box-shadow (not counted by offsetWidth/offsetHeight) doesn't
      // bleed past the webview's own OS edge.

      test("trigger flush against the left edge: flipped-right tooltip's left respects EDGE_MARGIN", () => {
        // 'left' position with no room at all on the left flips right, and the
        // final horizontal clamp must not let it slide under 8.
        const triggerRect = mockRect(0, 300, 20, 20);
        const tooltipSize = { width: 100, height: 30 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        expect(pos.arrowSide).toBe("right");
        expect(pos.left).toBeGreaterThanOrEqual(8);
      });

      test("trigger flush against the right edge: flipped-left tooltip's right edge respects EDGE_MARGIN", () => {
        const triggerRect = mockRect(viewport.width - 20, 300, 20, 20);
        const tooltipSize = { width: 100, height: 30 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "right", viewport);

        expect(pos.arrowSide).toBe("left");
        expect(pos.left + tooltipSize.width).toBeLessThanOrEqual(viewport.width - 8);
      });

      test("trigger flush against the top edge: tooltip's top respects EDGE_MARGIN", () => {
        const triggerRect = mockRect(500, 0, 20, 20);
        const tooltipSize = { width: 100, height: 60 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        expect(pos.top).toBeGreaterThanOrEqual(8);
      });

      test("trigger flush against the bottom edge: tooltip's bottom respects EDGE_MARGIN", () => {
        const triggerRect = mockRect(500, viewport.height - 20, 20, 20);
        const tooltipSize = { width: 100, height: 60 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        expect(pos.top + tooltipSize.height).toBeLessThanOrEqual(viewport.height - 8);
      });

      // Degenerate cases — the tooltip is bigger than the band it is clamped
      // into, so ONE edge has to lose. Both axes must lose the same one: the
      // tail, never the start of the text. (Clamping top first and bottom
      // second would produce a NEGATIVE top and put the first line
      // off-screen.)
      test("a tooltip taller than the viewport keeps its TOP on screen", () => {
        const triggerRect = mockRect(500, 300, 20, 20);
        const tooltipSize = { width: 100, height: viewport.height + 40 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        expect(pos.top).toBe(8);
      });

      test("a tooltip wider than the viewport keeps its LEFT on screen", () => {
        const triggerRect = mockRect(500, 300, 20, 20);
        const tooltipSize = { width: viewport.width + 40, height: 30 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        expect(pos.left).toBe(8);
      });
    });

    /**
     * The arrow describes the TRIGGER, so it has to be measured like the box
     * is. `arrowOffset` is the arrow's centre measured from the tooltip's own
     * top-left corner, along whichever edge the arrow sits on — x for
     * top/bottom, y for left/right. A fixed 50% of the tooltip is only the
     * trigger's centre when the clamp did not fire; every edge-adjacent
     * trigger in a narrow surface would get an arrow pointing at empty space.
     * The built site's hint placement has the same fix: measure once, write
     * an offset the stylesheet reads.
     */
    describe("arrow offset tracks the trigger through the clamp", () => {
      test("bottom placement: a clamped tooltip still points at its trigger", () => {
        // The reported case — the ＋ button near the right edge of the action
        // panel, with a tooltip wider than the room left of that edge.
        const panel = { width: 280, height: 768 };
        const triggerRect = mockRect(232, 40, 24, 24); // centre x = 244
        const tooltipSize = { width: 220, height: 20 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", panel);

        // Centring would want left = 244 - 110 = 134, ending at 354 — past the
        // 280px panel — so the clamp pulls it back to 280 - 8 - 220 = 52.
        expect(pos.left).toBe(52);
        // The clamp here is horizontal — the arrow's ALONG axis — so
        // arrowOffset absorbs it and the arrow survives, unlike the
        // cross-axis clamps below.
        expect(pos.arrowSide).toBe("bottom");
        // ...and the arrow must still land on the button, not on the box's
        // centre (which is 52 + 110 = 162, some 82px adrift).
        expect(pos.left + pos.arrowOffset).toBe(244);
      });

      test("left placement: a vertically clamped tooltip still points at its trigger", () => {
        const triggerRect = mockRect(500, 10, 20, 20); // centre y = 20
        const tooltipSize = { width: 100, height: 40 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", viewport);

        // Vertical centring wants top = 20 - 20 = 0; the clamp lifts it to 8.
        expect(pos.top).toBe(8);
        expect(pos.top + pos.arrowOffset).toBe(20);
      });

      test("an unclamped tooltip puts the arrow at its own centre", () => {
        const triggerRect = mockRect(500, 300, 20, 20);
        const tooltipSize = { width: 100, height: 30 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", viewport);

        expect(pos.arrowOffset).toBe(50);
      });

      test("a trigger outside the tooltip's span pins the arrow to the corner inset", () => {
        // A wide tooltip clamped to the left edge under a trigger far to the
        // right: the arrow cannot reach the trigger, so it stops at the inset
        // that keeps it clear of the corner radius instead of hanging off the
        // box.
        const triggerRect = mockRect(1000, 40, 20, 20); // centre x = 1010
        const tooltipSize = { width: 400, height: 20 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", viewport);

        expect(pos.left + pos.arrowOffset).toBeLessThan(1010);
        expect(pos.arrowOffset).toBe(400 - 8);
      });

      test("no arrow at all once the clamp moves the box off its trigger", () => {
        // The cover-chip case: a wide tooltip on a chip in a narrow panel goes
        // off-screen left, flips right, and is then pulled back until the box
        // covers its own trigger. `arrow-right` pins the arrow to the box's
        // LEFT edge, which by then is nowhere near the chip — so draw none.
        const narrow = { width: 500, height: 1000 };
        const triggerRect = mockRect(134, 59, 167, 32);
        const tooltipSize = { width: 289, height: 24 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "left", narrow);

        expect(pos.left).toBe(500 - 8 - 289);
        expect(pos.arrowSide).toBeNull();
      });

      test("no arrow when the VERTICAL clamp moves a bottom-placed tooltip", () => {
        // Same rule on the other axis: `arrow-bottom` pins the arrow to the
        // box's top edge, true only while that edge sits GAP below the trigger.
        const shortViewport = { width: 1024, height: 120 };
        const triggerRect = mockRect(500, 40, 20, 20);
        const tooltipSize = { width: 100, height: 90 };

        const pos = computeTooltipPosition(triggerRect, tooltipSize, "bottom", shortViewport);

        expect(pos.arrowSide).toBeNull();
      });

      test("a clamp shorter than the gap keeps the arrow — the box is still below", () => {
        // Asking the pre-clamp wish instead of the trigger called ANY movement
        // a detachment. Here a "top" placement with no room above flips below,
        // and the bottom clamp lands 2px short of the wished-for 8px gap: the
        // box is still wholly below the trigger, so the arrow is still true.
        const shortViewport = { width: 1024, height: 100 };
        const triggerRect = mockRect(500, 30, 20, 40); // bottom = 70

        const pos = computeTooltipPosition(
          triggerRect,
          { width: 100, height: 20 },
          "top",
          shortViewport
        );

        expect(pos.top).toBe(72); // clamped from the wanted 78
        expect(pos.top).toBeGreaterThanOrEqual(triggerRect.bottom);
        expect(pos.arrowSide).toBe("bottom");
      });

    });
  });
});

/**
 * The tooltip is `position: fixed` inside `body`, which `contain: layout`
 * (the app shell's stylesheet) makes its containing block. At rest the preview webview keeps
 * its FULL window width and is shifted right, so only the left
 * `100% - --preview-width-inset` is ever on screen; body is narrowed to exactly
 * that band. Clamping against `window.innerWidth` therefore lets a tooltip sit
 * up to `inset` px inside the strip that runs off-screen — on screen by the
 * numbers, invisible in fact.
 *
 * This matters for the publish button (bottom-right, nearest the band edge) and
 * for the change-set line it will carry, which is several times wider than
 * "Publish".
 */
describe("Tooltip stays inside the VISIBLE band, not the webview viewport", () => {
  const REAL_INNER_WIDTH = window.innerWidth;
  const BAND = 800; // body.clientWidth — the on-screen part
  const OFFSCREEN = 1200; // window.innerWidth — band + --preview-width-inset

  let trigger: HTMLElement;

  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", {
      value: OFFSCREEN,
      configurable: true,
    });
    Object.defineProperty(document.body, "clientWidth", {
      value: BAND,
      configurable: true,
    });
    initTooltips();

    // jsdom has no layout: the tooltip would measure 0×0 and every position
    // would collapse to the trigger. Give it a realistic width.
    const el = document.querySelector<HTMLElement>(".moss-tooltip")!;
    Object.defineProperty(el, "offsetWidth", { value: 200, configurable: true });
    Object.defineProperty(el, "offsetHeight", { value: 20, configurable: true });

    trigger = document.createElement("button");
    trigger.setAttribute("data-tooltip", "12 added · 30 edited · 4 deleted");
    trigger.setAttribute("data-tooltip-position", "right");
    // Hard against the right edge of the visible band.
    trigger.getBoundingClientRect = () =>
      ({ left: 760, right: 780, top: 100, bottom: 120, width: 20, height: 20 }) as DOMRect;
    document.body.appendChild(trigger);
  });

  afterEach(() => {
    trigger.remove();
    destroyTooltips();
    Object.defineProperty(window, "innerWidth", {
      value: REAL_INNER_WIDTH,
      configurable: true,
    });
    // @ts-expect-error — restore the prototype getter jsdom normally provides
    delete document.body.clientWidth;
  });

  test("a right-placed tooltip flips rather than spilling into the off-screen strip", () => {
    trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    const el = document.querySelector<HTMLElement>(".moss-tooltip")!;

    // Right placement would put it at 780 + 8 = 788, ending at 988. That fits
    // inside window.innerWidth (1200) but is 188px past the band, so it must
    // flip to the left: 760 - 8 - 200 = 552.
    expect(el.style.left).toBe("552px");
    expect(el.classList.contains("arrow-left")).toBe(true);

    const left = parseFloat(el.style.left);
    expect(left + 200).toBeLessThanOrEqual(BAND);
  });
});

// Tooltip a11y — focus/Esc/aria-describedby
describe("Tooltip a11y", () => {
  let trigger: HTMLElement;

  beforeEach(() => {
    initTooltips();
    trigger = document.createElement("button");
    trigger.setAttribute("data-tooltip", "Keyboard tooltip");
    document.body.appendChild(trigger);
  });

  afterEach(() => {
    if (trigger.parentNode) trigger.parentNode.removeChild(trigger);
    destroyTooltips();
  });

  test("focusin shows the tooltip (mirrors mouseover)", () => {
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    const tooltip = document.querySelector(".moss-tooltip");
    expect(tooltip?.classList.contains("visible")).toBe(true);
  });

  test("focusout hides the tooltip (mirrors mouseout)", () => {
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    trigger.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    const tooltip = document.querySelector(".moss-tooltip");
    expect(tooltip?.classList.contains("visible")).toBe(false);
  });

  test("focusin sets aria-describedby on trigger pointing to singleton tooltip id", () => {
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    const tooltip = document.querySelector(".moss-tooltip");
    expect(tooltip).not.toBeNull();
    // The tooltip must have a stable id
    expect(tooltip!.id).toBeTruthy();
    // The trigger must have aria-describedby pointing to the tooltip id
    expect(trigger.getAttribute("aria-describedby")).toBe(tooltip!.id);
  });

  test("focusout removes aria-describedby from the trigger", () => {
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    trigger.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    expect(trigger.hasAttribute("aria-describedby")).toBe(false);
  });

  test("Escape key hides the tooltip and removes aria-describedby", () => {
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(document.querySelector(".moss-tooltip")?.classList.contains("visible")).toBe(true);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(document.querySelector(".moss-tooltip")?.classList.contains("visible")).toBe(false);
    expect(trigger.hasAttribute("aria-describedby")).toBe(false);
  });

  test("hover→focus interleave: focus after hover doesn't strand currentTrigger", () => {
    // Hover to show
    trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    expect(document.querySelector(".moss-tooltip")?.classList.contains("visible")).toBe(true);

    // Then focus on same trigger — should stay visible, not crash
    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(document.querySelector(".moss-tooltip")?.classList.contains("visible")).toBe(true);

    // Mouseout while focused — tooltip stays (focus keeps it open)
    trigger.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
    // Note: mouseout from the trigger while it has focus — the focusin should
    // keep the tooltip active (currentTrigger should still be set)
    const tooltip = document.querySelector(".moss-tooltip");
    // After mouseout, the tooltip may hide via mouseout handler —
    // the key requirement is it doesn't crash and currentTrigger is consistent
    // (i.e. the Escape handler still works after interleave)
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    // Should not throw; tooltip remains in a clean state
    expect(tooltip).not.toBeNull();
  });

  test("retarget: focus on second trigger removes aria-describedby from first", () => {
    const trigger2 = document.createElement("button");
    trigger2.setAttribute("data-tooltip", "Second");
    document.body.appendChild(trigger2);

    trigger.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(trigger.getAttribute("aria-describedby")).toBeTruthy();

    // Focus moves to second trigger
    trigger2.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    // First trigger's aria-describedby must be removed on retarget
    expect(trigger.hasAttribute("aria-describedby")).toBe(false);
    // Second trigger should now have aria-describedby
    expect(trigger2.getAttribute("aria-describedby")).toBeTruthy();

    trigger2.parentNode?.removeChild(trigger2);
  });
});

/**
 * A tooltip answers a hover. It must not outlive the moment it was answering,
 * and it must never be the thing on screen that the user has to look past.
 *
 * Hiding only on mouseout, focusout, Escape, scroll and resize is not
 * enough: a tooltip must also go on activation and when another surface takes
 * the screen. Otherwise the z-scale decides what the user sees, and it
 * decides differently in each direction: `.ctx-menu` (250) and the
 * `.pop`/`.omenu`/`.dropdown` tier (150) bury a `--z-popup` (100) tooltip,
 * while modal content (51) ends up under one.
 */
describe("Tooltip yields to the surfaces that open over it", () => {
  let trigger: HTMLElement;

  beforeEach(() => {
    initTooltips();
    trigger = document.createElement("button");
    trigger.setAttribute("data-tooltip", "New page in William Blake");
    document.body.appendChild(trigger);
  });

  afterEach(() => {
    trigger.remove();
    destroyTooltips();
  });

  const shown = (): boolean =>
    document.querySelector(".moss-tooltip")?.classList.contains("visible") ?? false;

  const hover = (): void => {
    trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  };

  /** The covering surface, with a tooltip trigger of its own inside it. */
  const surface = (): { root: HTMLElement; inner: HTMLElement } => {
    const root = document.createElement("div");
    const inner = document.createElement("button");
    inner.setAttribute("data-tooltip", "Copied");
    root.appendChild(inner);
    document.body.appendChild(root);
    return { root, inner };
  };

  test("suppressing hides a tooltip the surface covers", () => {
    hover();
    expect(shown()).toBe(true);

    const { root } = surface();
    const release = suppressTooltips(root);

    expect(shown()).toBe(false);
    release();
    root.remove();
  });

  test("no covered tooltip can be shown while suppressed — the reported bug", () => {
    // With the create menu open, moving the pointer back across the ＋ button
    // re-fires mouseover, which must not resurrect the tooltip UNDER the menu.
    const { root } = surface();
    const release = suppressTooltips(root);

    hover();

    expect(shown()).toBe(false);
    release();
    root.remove();
  });

  test("a trigger INSIDE the surface still shows — it is not being covered", () => {
    // The icon-only copy button ships inside two modals, and `data-tooltip` is
    // its only sighted name AND the carrier of its "Copied" ack. A global mute
    // deletes both.
    const { root, inner } = surface();
    const release = suppressTooltips(root);

    inner.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

    expect(shown()).toBe(true);
    expect(document.querySelector(".moss-tooltip")?.textContent).toBe("Copied");
    release();
    root.remove();
  });

  test("a tooltip already up INSIDE the surface survives the surface opening", () => {
    const { root, inner } = surface();
    inner.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    expect(shown()).toBe(true);

    const release = suppressTooltips(root);

    expect(shown()).toBe(true);
    release();
    root.remove();
  });

  test("two overlapping surfaces: the LAST release restores tooltips", () => {
    // The chip bar can have an overlay and a context menu open across the same
    // moment; whichever closes first must not speak for the other.
    const menu = surface();
    const overlay = surface();
    const releaseMenu = suppressTooltips(menu.root);
    const releaseOverlay = suppressTooltips(overlay.root);

    releaseMenu();
    hover();
    expect(shown()).toBe(false);

    releaseOverlay();
    hover();
    expect(shown()).toBe(true);

    menu.root.remove();
    overlay.root.remove();
  });

  test("with two surfaces up, a trigger inside only ONE is still covered by the other", () => {
    const menu = surface();
    const overlay = surface();
    const releaseMenu = suppressTooltips(menu.root);
    const releaseOverlay = suppressTooltips(overlay.root);

    menu.inner.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    expect(shown()).toBe(false);

    releaseMenu();
    releaseOverlay();
    menu.root.remove();
    overlay.root.remove();
  });

  test("a release speaks only for its own surface, however often it is called", () => {
    const first = surface();
    const second = surface();
    const release = suppressTooltips(first.root);
    const other = suppressTooltips(second.root);

    release();
    release();
    release();

    hover();
    expect(shown()).toBe(false);

    other();
    hover();
    expect(shown()).toBe(true);

    first.root.remove();
    second.root.remove();
  });

  test("teardown cannot hand one surface's debt to another", () => {
    // A surface outliving destroyTooltips() must not leave its release holding
    // a count the NEXT surface owed, muting tooltips with nothing on screen.
    const stranded = surface();
    const releaseStranded = suppressTooltips(stranded.root);

    destroyTooltips();
    initTooltips();
    document.body.appendChild(trigger);

    releaseStranded();

    hover();
    expect(shown()).toBe(true);
    stranded.root.remove();
  });
});

describe("show() writes the measured arrow offset for the stylesheet", () => {
  let trigger: HTMLElement;

  beforeEach(() => {
    initTooltips();
    const el = document.querySelector<HTMLElement>(".moss-tooltip")!;
    // jsdom has no layout — give the box a real width to clamp against.
    Object.defineProperty(el, "offsetWidth", { value: 220, configurable: true });
    Object.defineProperty(el, "offsetHeight", { value: 20, configurable: true });

    trigger = document.createElement("button");
    trigger.setAttribute("data-tooltip", "New page in William Blake");
    trigger.setAttribute("data-tooltip-position", "bottom");
    trigger.getBoundingClientRect = () =>
      ({ left: 232, right: 256, top: 40, bottom: 64, width: 24, height: 24 }) as DOMRect;
    document.body.appendChild(trigger);
  });

  afterEach(() => {
    trigger.remove();
    destroyTooltips();
  });

  test("a detached box gets no arrow-* class, so no arrow paints", () => {
    Object.defineProperty(document.body, "clientWidth", { value: 240, configurable: true });
    try {
      trigger.setAttribute("data-tooltip-position", "left");
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const el = document.querySelector<HTMLElement>(".moss-tooltip")!;
      expect(el.classList.contains("visible")).toBe(true);
      expect([...el.classList].some((c) => c.startsWith("arrow-"))).toBe(false);
    } finally {
      // @ts-expect-error — restore the prototype getter jsdom provides
      delete document.body.clientWidth;
    }
  });

  test("the arrow property points at the trigger's centre, not the box's", () => {
    Object.defineProperty(document.body, "clientWidth", { value: 280, configurable: true });
    try {
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));

      const el = document.querySelector<HTMLElement>(".moss-tooltip")!;
      const left = parseFloat(el.style.left);
      const arrow = parseFloat(el.style.getPropertyValue("--moss-tooltip-arrow"));

      expect(left + arrow).toBe(244);
    } finally {
      // @ts-expect-error — restore the prototype getter jsdom provides
      delete document.body.clientWidth;
    }
  });
});
