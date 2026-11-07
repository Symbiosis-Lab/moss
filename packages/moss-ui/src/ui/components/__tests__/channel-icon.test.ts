/**
 * Unit tests for the ChannelIcon UI primitive.
 *
 * ChannelIcon is intentionally domain-agnostic: it knows how a glyph + corner
 * badge look, not what they mean. These tests exercise the component contract
 * — DOM shape, badge variants, in-place updates — without simulating any
 * specific channel state machine.
 */

import { describe, test, expect, vi } from "vitest";
import {
  createChannelIcon,
  updateChannelIconBadge,
  type ChannelIconBadge,
} from "../channel-icon";

describe("ChannelIcon", () => {
  describe("glyph rendering", () => {
    test("inlines a valid SVG and tags it for stylability", () => {
      const svg = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>';
      const root = createChannelIcon({ iconSvg: svg });

      const inner = root.querySelector("svg");
      expect(inner).not.toBeNull();
      expect(inner?.classList.contains("channel-icon-svg")).toBe(true);
      // Inline width/height removed so CSS can size the glyph
      expect(inner?.getAttribute("width")).toBeNull();
      expect(inner?.getAttribute("height")).toBeNull();
    });

    test("strips root width/height in the string, never from the live element", () => {
      // WebKit logs `Error: Invalid value for <svg> attribute width=""` when
      // width/height are removed from a parsed SVG element (re-parsing the
      // now-empty value fails). The dimensions must be stripped from the
      // markup BEFORE innerHTML so removeAttribute never runs against them.
      const removeSpy = vi.spyOn(Element.prototype, "removeAttribute");
      try {
        const svg =
          '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><circle cx="128" cy="128" r="100"/></svg>';
        const root = createChannelIcon({ iconSvg: svg });

        const inner = root.querySelector("svg");
        expect(inner?.getAttribute("width")).toBeNull();
        expect(inner?.getAttribute("height")).toBeNull();
        expect(removeSpy).not.toHaveBeenCalledWith("width");
        expect(removeSpy).not.toHaveBeenCalledWith("height");
      } finally {
        removeSpy.mockRestore();
      }
    });

    test("keeps width/height on nested shapes when stripping the root", () => {
      // Real case: the terrarium plugin icon sizes inner <rect>s with
      // width/height attributes — only the root <svg>'s must go.
      const svg =
        '<svg viewBox="0 0 256 256" width="256" height="256">' +
        '<rect x="52" y="168" width="152" height="28"/></svg>';
      const root = createChannelIcon({ iconSvg: svg });

      const inner = root.querySelector("svg");
      expect(inner?.getAttribute("width")).toBeNull();
      expect(inner?.getAttribute("height")).toBeNull();
      const rect = root.querySelector("rect");
      expect(rect?.getAttribute("width")).toBe("152");
      expect(rect?.getAttribute("height")).toBe("28");
    });

    test("strips single-quoted and unquoted root dimensions", () => {
      const svg = "<svg viewBox='0 0 24 24' width='24' height=24><circle r='10'/></svg>";
      const root = createChannelIcon({ iconSvg: svg });

      const inner = root.querySelector("svg");
      expect(inner?.getAttribute("width")).toBeNull();
      expect(inner?.getAttribute("height")).toBeNull();
      expect(inner?.getAttribute("viewBox")).toBe("0 0 24 24");
    });

    test("falls back to first uppercase letter when SVG is null", () => {
      const root = createChannelIcon({ iconSvg: null, fallbackLetter: "matters" });
      const fallback = root.querySelector(".channel-icon-fallback");
      expect(fallback?.textContent).toBe("M");
    });

    test("falls back to '?' when no fallbackLetter provided", () => {
      const root = createChannelIcon({ iconSvg: null });
      const fallback = root.querySelector(".channel-icon-fallback");
      expect(fallback?.textContent).toBe("?");
    });

    test("falls back when iconSvg is invalid", () => {
      const root = createChannelIcon({ iconSvg: "not svg", fallbackLetter: "X" });
      expect(root.querySelector("svg")).toBeNull();
      expect(root.querySelector(".channel-icon-fallback")?.textContent).toBe("X");
    });
  });

  describe("badge variants", () => {
    test("no badge by default", () => {
      const root = createChannelIcon({ iconSvg: null });
      expect(root.querySelector(".channel-icon-badge")).toBeNull();
    });

    test("circle and dot are morph-family disc poses", () => {
      // The ring (armed) and filled dot (published) are poses of the shared
      // morph skeleton — the disc's fill/stroke are styled per data-shape in
      // CSS, not baked into per-kind markup.
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "circle" },
      });
      const badge = root.querySelector<HTMLElement>(".channel-icon-badge");
      expect(badge?.classList.contains("channel-icon-badge--circle")).toBe(true);
      expect(badge?.classList.contains("channel-icon-badge--morph")).toBe(true);
      expect(badge?.dataset.shape).toBe("circle");
      expect(badge?.querySelector(".icon-morph-disc")).not.toBeNull();

      const dotRoot = createChannelIcon({ iconSvg: null, badge: { kind: "dot" } });
      const dotBadge = dotRoot.querySelector<HTMLElement>(".channel-icon-badge");
      expect(dotBadge?.classList.contains("channel-icon-badge--dot")).toBe(true);
      expect(dotBadge?.classList.contains("channel-icon-badge--morph")).toBe(true);
      expect(dotBadge?.dataset.shape).toBe("dot");
    });

    test("spinner badge tags its inner SVG for animation", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "spinner" },
      });
      const badge = root.querySelector(".channel-icon-badge--spinner");
      expect(badge).not.toBeNull();
      expect(badge?.querySelector("svg.channel-icon-spinner")).not.toBeNull();
    });

    test.each<[ChannelIconBadge["kind"], string]>([
      ["circle", "channel-icon-badge--circle"],
      ["dot", "channel-icon-badge--dot"],
    ])("%s badge gets the matching modifier class", (kind, modifier) => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind } as ChannelIconBadge,
      });
      expect(root.querySelector(`.${modifier}`)).not.toBeNull();
    });

    test("the attention tone is a class on the morph badge, not a separate kind", () => {
      // Amber has to ride the SAME disc as `dot`, or the rail's "this needs
      // you" state would swap nodes and lose the ring → dot morph. The hue
      // itself lives in CSS; the markup only says which tone applies.
      const amber = createChannelIcon({
        iconSvg: null,
        badge: { kind: "dot", tone: "attention" },
      });
      const badge = amber.querySelector(".channel-icon-badge");
      expect(badge?.classList.contains("channel-icon-badge--morph")).toBe(true);
      expect(badge?.classList.contains("channel-icon-badge--attention")).toBe(true);
      expect((badge as HTMLElement).dataset.shape).toBe("dot");
      expect(amber.querySelector(".icon-morph-disc")?.getAttribute("fill")).toBeNull();
    });

    test("the tone is dropped when the badge morphs back to a toneless state", () => {
      // Reusing one node means a stale class is a stale color: a channel that
      // fails and is then published again must not stay amber.
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "dot", tone: "attention" },
      });
      updateChannelIconBadge(root, { kind: "dot" });
      const badge = root.querySelector(".channel-icon-badge");
      expect(badge?.classList.contains("channel-icon-badge--attention")).toBe(false);
    });

    test("morph strokes take no color from the markup", () => {
      // The lamps above sit on nothing and name their own color; the morph
      // strokes sit ON a disc, so their color has to follow whichever disc is
      // under them and belongs in CSS (`--icon-morph-ink`). They were hard-
      // coded `stroke="white"`, which is 2.54:1 on the light success disc and
      // 1.92:1 on the dark one — both below the 3:1 WCAG 1.4.11 floor for
      // non-text graphics, and unfixable from here because the markup cannot
      // see which pose it is in.
      const root = createChannelIcon({ iconSvg: null, badge: { kind: "dot" } });
      for (const cls of [".icon-morph-a", ".icon-morph-b"]) {
        const line = root.querySelector(cls);
        expect(line).not.toBeNull();
        expect(line?.getAttribute("stroke")).toBeNull();
      }
    });
  });

  describe("updateChannelIconBadge", () => {
    test("adds a badge when none was mounted", () => {
      const root = createChannelIcon({ iconSvg: null });
      expect(root.querySelector(".channel-icon-badge")).toBeNull();

      updateChannelIconBadge(root, { kind: "circle" });
      expect(root.querySelector(".channel-icon-badge--circle")).not.toBeNull();
    });

    test("replaces an existing badge with a different kind", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "spinner" },
      });
      expect(root.querySelector(".channel-icon-badge--spinner")).not.toBeNull();

      updateChannelIconBadge(root, { kind: "dot" });
      expect(root.querySelector(".channel-icon-badge--spinner")).toBeNull();
      expect(root.querySelector(".channel-icon-badge--dot")).not.toBeNull();
    });

    test("circle → dot keeps the SAME node (armed → published is a morph)", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "circle" },
      });
      const badge = root.querySelector<HTMLElement>(".channel-icon-badge--circle");
      expect(badge).not.toBeNull();

      updateChannelIconBadge(root, { kind: "dot" });
      expect(root.querySelector(".channel-icon-badge--circle")).toBeNull();
      expect(root.querySelector(".channel-icon-badge--dot")).toBe(badge);
      expect(badge!.dataset.shape).toBe("dot");
    });

    test("same kind keeps the SAME node — replacing restarts the spinner's CSS rotation", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "spinner" },
      });
      const before = root.querySelector(".channel-icon-badge--spinner");
      expect(before).not.toBeNull();

      // Per-tick progress updates re-apply the same badge; node must survive.
      updateChannelIconBadge(root, { kind: "spinner" });
      updateChannelIconBadge(root, { kind: "spinner" });
      expect(root.querySelector(".channel-icon-badge--spinner")).toBe(before);
    });

    test("same-kind update reconciles the queued dim state in place", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "spinner", dim: true },
      });
      const el = root.querySelector(".channel-icon-badge--spinner")!;
      expect(el.classList.contains("is-queued")).toBe(true);

      updateChannelIconBadge(root, { kind: "spinner" });
      expect(root.querySelector(".channel-icon-badge--spinner")).toBe(el);
      expect(el.classList.contains("is-queued")).toBe(false);
    });

    test("the two lamp poses are ONE node, so armed → published is a fill", () => {
      // The rail's whole legibility rests on this: circle and dot are the same
      // disc, so publishing reads as
      // the ring filling in. A node swap anywhere would jump-cut it.
      const root = createChannelIcon({ iconSvg: null, badge: { kind: "circle" } });
      const badge = root.querySelector<HTMLElement>(".channel-icon-badge")!;
      expect(badge.classList.contains("channel-icon-badge--morph")).toBe(true);
      expect(badge.dataset.shape).toBe("circle");
      // The shared skeleton: one disc, two stroke lines.
      expect(badge.querySelector(".icon-morph-disc")).not.toBeNull();
      expect(badge.querySelector(".icon-morph-a")).not.toBeNull();
      expect(badge.querySelector(".icon-morph-b")).not.toBeNull();

      updateChannelIconBadge(root, { kind: "dot" });
      expect(root.querySelector(".channel-icon-badge")).toBe(badge);
      expect(badge.classList.contains("channel-icon-badge--circle")).toBe(false);
      expect(badge.dataset.shape).toBe("dot");

      updateChannelIconBadge(root, { kind: "circle" });
      expect(root.querySelector(".channel-icon-badge")).toBe(badge);
      expect(badge.dataset.shape).toBe("circle");
    });

    test("crossing the family boundary still swaps the node (spinner is not a lamp)", () => {
      const root = createChannelIcon({ iconSvg: null, badge: { kind: "spinner" } });
      const spinner = root.querySelector(".channel-icon-badge")!;

      updateChannelIconBadge(root, { kind: "dot" });
      const morph = root.querySelector(".channel-icon-badge")!;
      expect(morph).not.toBe(spinner);
      expect(morph.classList.contains("channel-icon-badge--morph")).toBe(true);
    });

    test("removes the badge when called with undefined", () => {
      const root = createChannelIcon({
        iconSvg: null,
        badge: { kind: "circle" },
      });
      expect(root.querySelector(".channel-icon-badge")).not.toBeNull();

      updateChannelIconBadge(root, undefined);
      expect(root.querySelector(".channel-icon-badge")).toBeNull();
    });
  });
});
