/**
 * Tests for state.ts's URL round trip. writeCamera/writeSelection/writeScope
 * call history.replaceState directly (jsdom supports it), so a write can be
 * read straight back with readUrlState against the live location.
 */
import { afterEach, describe, test, expect } from "vitest";
import { readUrlState, writeCamera, writeScope, writeSelection } from "../state";

afterEach(() => {
  history.replaceState(null, "", "/places/");
});

describe("readUrlState", () => {
  test("no params at all is the all scope, no selection, no camera", () => {
    const state = readUrlState(new URLSearchParams(""));
    expect(state).toEqual({ scope: { kind: "all" }, articleId: null, camera: null });
  });

  test("place= sets a place scope", () => {
    const state = readUrlState(new URLSearchParams("place=lisbon"));
    expect(state.scope).toEqual({ kind: "place", id: "lisbon" });
  });

  test("article= selects a work without changing the scope", () => {
    const state = readUrlState(new URLSearchParams("article=harbor-walk"));
    expect(state.scope).toEqual({ kind: "all" });
    expect(state.articleId).toBe("harbor-walk");
  });

  test("a full, valid camera round-trips", () => {
    const state = readUrlState(new URLSearchParams("p=patterson&z=2.5&x=400.12&y=210.5"));
    expect(state.camera).toEqual({ x: 400.12, y: 210.5, zoom: 2.5 });
  });

  test("an unrecognised projection drops the whole camera, not just itself", () => {
    const state = readUrlState(new URLSearchParams("p=mercator&z=2.5&x=400&y=210"));
    expect(state.camera).toBeNull();
  });

  test("a non-finite z/x/y drops the whole camera", () => {
    for (const qs of ["p=patterson&z=NaN&x=0&y=0", "p=patterson&z=2&x=&y=0", "p=patterson&z=0&x=0&y=0"]) {
      expect(readUrlState(new URLSearchParams(qs)).camera).toBeNull();
    }
  });
});

describe("write round trip", () => {
  test("writeCamera then readUrlState sees the same camera back", () => {
    writeCamera({ x: 12.3, y: 45.6, zoom: 3.2 });
    const read = readUrlState();
    expect(read.camera).toEqual({ x: 12.3, y: 45.6, zoom: 3.2 });
  });

  test("writeSelection(null) clears article= instead of writing it empty", () => {
    writeSelection("harbor-walk");
    expect(new URL(location.href).searchParams.get("article")).toBe("harbor-walk");
    writeSelection(null);
    expect(new URL(location.href).searchParams.has("article")).toBe(false);
  });

  test("writeScope round-trips a place scope and clears back to all", () => {
    writeScope({ kind: "place", id: "porto" });
    expect(readUrlState().scope).toEqual({ kind: "place", id: "porto" });
    writeScope({ kind: "all" });
    expect(readUrlState().scope).toEqual({ kind: "all" });
  });
});
