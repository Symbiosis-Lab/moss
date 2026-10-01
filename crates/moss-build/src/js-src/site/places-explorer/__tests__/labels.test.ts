/**
 * Tests for labels.ts's pure half: greedy placement with rectangle
 * collisions, the area budget and its minimum, the own-place rule for a
 * city label beside a marker, language selection and fallback, and the
 * vertical-writing choice for a near-vertical CJK river. The DOM-building
 * half (`LabelLayer`) is covered by the render gates, the layer that can
 * actually measure a rendered box and see it positioned on screen.
 */
import { describe, test, expect } from "vitest";
import {
  labelBudget,
  LABEL_AREA_BUDGET_PX2,
  LABEL_MIN_COUNT,
  placeLabels,
  placeNameMatchesLabel,
  riverAnchor,
  riverOrientation,
  selectLanguageLabels,
  type LabelCandidate,
  type LabelMarkerBox,
} from "../labels";
import type { LabelsData } from "../types";

function city(id: string, name: string, priority: number, x: number, y: number, width = 40, height = 14): LabelCandidate {
  return { id, kind: "city", name, priority, screen: { x, y }, width, height };
}

function range(id: string, name: string, priority: number, x: number, y: number, width = 60, height = 14): LabelCandidate {
  return { id, kind: "range", name, priority, screen: { x, y }, width, height };
}

const VIEWPORT = { width: 400, height: 300 };
const NEVER_OWN = (): boolean => false;

describe("placeLabels — greedy priority placement with rectangle collisions", () => {
  test("two labels far apart both get placed", () => {
    const candidates = [city("a", "Alpha", 0, 50, 50), city("b", "Beta", 1, 300, 250)];
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.size).toBe(2);
  });

  test("a lower-priority (higher rank number) candidate colliding with a higher-priority one's box is dropped, not slid elsewhere", () => {
    // Both anchored at nearly the same point — "a" (priority 0) wins the
    // spot; "b" (priority 1) has nowhere left that clears "a"'s own placed
    // box at this same anchor, so it must be dropped rather than silently
    // appearing somewhere else on the map.
    const candidates = [range("a", "Alpha Range", 0, 200, 150), range("b", "Beta Range", 1, 201, 150)];
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.has("a")).toBe(true);
    expect(placed.has("b")).toBe(false);
  });

  test("priority order decides which of two colliding candidates wins, independent of input array order", () => {
    const low = range("low-priority", "Low", 5, 200, 150);
    const high = range("high-priority", "High", 0, 201, 150);
    const placed = placeLabels([low, high], { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.has("high-priority")).toBe(true);
    expect(placed.has("low-priority")).toBe(false);
  });

  test("a candidate never placed outside the viewport, even with room elsewhere", () => {
    const candidates = [range("offstage", "Offstage", 0, 1000, 1000)];
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.size).toBe(0);
  });

  test("a label never overlaps a reserved UI region", () => {
    const reserved = [{ x: 180, y: 130, width: 40, height: 40 }]; // covers (200,150)
    const candidates = [range("under-ui", "Under UI", 0, 200, 150, 10, 10)];
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved, ownPlace: NEVER_OWN });
    expect(placed.size).toBe(0);
  });

  test("placed label boxes never overlap each other, pairwise, across a densely packed candidate set", () => {
    // 50px-wide boxes on a 20px grid: every immediate neighbour's candidate
    // box WOULD overlap if placed — the real pass must drop most of them
    // rather than let any two placed boxes collide.
    const candidates = Array.from({ length: 12 }, (_, i) => range(`r${i}`, `Region ${i}`, i, 60 + (i % 4) * 20, 40 + Math.floor(i / 4) * 20, 50, 16));
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.size).toBeGreaterThan(0);
    expect(placed.size).toBeLessThan(candidates.length); // proves collisions were actually in play, not a vacuous pass
    const boxes = [...placed.values()].map((p) => p.box);
    for (let i = 0; i < boxes.length; i++) {
      for (let j = i + 1; j < boxes.length; j++) {
        const a = boxes[i];
        const b = boxes[j];
        const overlaps = a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
        expect(overlaps, `box ${i} and ${j} overlap: ${JSON.stringify(a)} / ${JSON.stringify(b)}`).toBe(false);
      }
    }
  });
});

describe("placeLabels — area budget and its minimum", () => {
  test("labelBudget is area/BUDGET, rounded, for a large viewport", () => {
    const viewport = { width: 1440, height: 900 };
    expect(labelBudget(viewport)).toBe(Math.round((1440 * 900) / LABEL_AREA_BUDGET_PX2));
  });

  test("labelBudget never drops below the minimum even for a tiny viewport", () => {
    const viewport = { width: 50, height: 50 }; // 2500px^2 / 45000 rounds to 0
    expect(labelBudget(viewport)).toBe(LABEL_MIN_COUNT);
  });

  test("placeLabels never places more than the budget, even with far more room and candidates", () => {
    const viewport = { width: 300, height: 300 }; // budget = max(5, round(90000/45000)) = 5
    const candidates = Array.from({ length: 20 }, (_, i) => range(`r${i}`, `Region ${i}`, i, 20 + (i % 10) * 28, 20 + Math.floor(i / 10) * 100, 20, 10));
    const placed = placeLabels(candidates, { viewport, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(labelBudget(viewport)).toBe(5);
    expect(placed.size).toBeLessThanOrEqual(5);
  });

  test("fewer candidates than the budget just places all of them — the budget caps, it never pads", () => {
    const candidates = [range("a", "Alpha", 0, 50, 50), range("b", "Beta", 1, 300, 250)];
    const placed = placeLabels(candidates, { viewport: VIEWPORT, markers: [], reserved: [], ownPlace: NEVER_OWN });
    expect(placed.size).toBe(2);
  });
});

describe("placeLabels — the own-place rule for a city label beside a marker", () => {
  const MARKER: LabelMarkerBox = { screen: { x: 200, y: 150 }, workIds: ["work-a"] };

  test("a city label whose own place matches the covering marker's own place is placed beside it", () => {
    const candidate = city("tel-aviv", "Tel Aviv", 0, 200, 150);
    const ownPlace = (label: LabelCandidate, marker: LabelMarkerBox): boolean => marker.workIds.includes("work-a") && label.name === "Tel Aviv";
    const placed = placeLabels([candidate], { viewport: VIEWPORT, markers: [MARKER], reserved: [], ownPlace });
    expect(placed.has("tel-aviv")).toBe(true);
    expect(placed.get("tel-aviv")!.dot).toBeNull(); // the marker itself is the anchor; no separate dot
  });

  test("a city label merely covered by an unrelated marker is dropped outright, never re-anchored to it (the Gaza/Tel Aviv regression)", () => {
    // The marker's own place is "Jabaliya Camp", nothing to do with "Tel
    // Aviv" — its screen point simply happens to coincide with where Tel
    // Aviv's own label would anchor.
    const candidate = city("tel-aviv", "Tel Aviv", 0, 200, 150);
    const ownPlace = (label: LabelCandidate): boolean => label.name === "Jabaliya Camp"; // never matches this candidate
    const placed = placeLabels([candidate], { viewport: VIEWPORT, markers: [MARKER], reserved: [], ownPlace });
    expect(placed.has("tel-aviv")).toBe(false);
  });

  test("a city label with no nearby marker at all gets its own anchor dot", () => {
    const candidate = city("remote", "Remote City", 0, 50, 50);
    const placed = placeLabels([candidate], { viewport: VIEWPORT, markers: [MARKER], reserved: [], ownPlace: NEVER_OWN });
    const result = placed.get("remote")!;
    expect(result.dot).toEqual({ x: 50, y: 50 });
  });

  test("a city label matching its own marker's place sorts ahead of an unrelated higher-priority (rank 0) city, so the budget never crowds it out", () => {
    const own = city("own-city", "Own City", 5, 200, 150); // low priority rank, but IS this marker's own place
    const unrelated = city("capital", "Faraway Capital", 0, 50, 50); // rank 0, nothing to do with any marker
    const ownPlace = (label: LabelCandidate): boolean => label.name === "Own City";
    // A budget of exactly 1 forces the choice between them.
    const placed = placeLabels([unrelated, own], {
      viewport: VIEWPORT,
      markers: [MARKER],
      reserved: [],
      ownPlace,
      minCount: 1,
      areaBudgetPx2: VIEWPORT.width * VIEWPORT.height, // => budget exactly 1
    });
    expect(placed.has("own-city")).toBe(true);
    expect(placed.has("capital")).toBe(false);
  });
});

describe("selectLanguageLabels — language selection and fallback", () => {
  const DATA: LabelsData = {
    languages: ["en", "zh-Hant"],
    en: { cities: [{ name: "Tokyo", lat: 1, lng: 2, rank: 0 }], ranges: [], peaks: [], rivers: [] },
    "zh-Hant": { cities: [{ name: "東京", lat: 1, lng: 2, rank: 0 }], ranges: [], peaks: [], rivers: [] },
  };

  test("a zh-Hant page reads the zh-Hant set", () => {
    expect(selectLanguageLabels(DATA, "zh-Hant").cities[0].name).toBe("東京");
  });

  test("an en page reads the en set", () => {
    expect(selectLanguageLabels(DATA, "en").cities[0].name).toBe("Tokyo");
  });

  test("a zh-Hans page falls back to en when the data has no zh-Hans set of its own", () => {
    expect(selectLanguageLabels(DATA, "zh-Hans").cities[0].name).toBe("Tokyo");
  });

  test("a zh-Hant page falls back to whichever language IS present when the data is zh-Hant-only and the preferred tag is missing — never throws, never returns empty when data exists", () => {
    const zhHantOnly: LabelsData = { languages: ["zh-Hant"], "zh-Hant": { cities: [{ name: "東京", lat: 1, lng: 2, rank: 0 }], ranges: [], peaks: [], rivers: [] } };
    expect(selectLanguageLabels(zhHantOnly, "en").cities[0].name).toBe("東京");
  });

  test("absent data (no data-labels, or a failed fetch) selects the empty set rather than throwing", () => {
    expect(selectLanguageLabels(null, "en")).toEqual({ cities: [], ranges: [], peaks: [], rivers: [] });
    expect(selectLanguageLabels(undefined, "zh-Hant")).toEqual({ cities: [], ranges: [], peaks: [], rivers: [] });
  });
});

describe("placeNameMatchesLabel — the own-place name heuristic", () => {
  test("an exact match", () => {
    expect(placeNameMatchesLabel("Lisbon", "Lisbon")).toBe(true);
  });
  test("a locally-named sub-area still counts as its city", () => {
    expect(placeNameMatchesLabel("Tokyo Shibuya", "Tokyo")).toBe(true);
    expect(placeNameMatchesLabel("Tokyo", "Tokyo Shibuya")).toBe(true);
  });
  test("two unrelated, merely nearby place names never match", () => {
    expect(placeNameMatchesLabel("Jabaliya Camp", "Tel Aviv")).toBe(false);
  });
  test("an empty name on either side never matches", () => {
    expect(placeNameMatchesLabel("", "Tel Aviv")).toBe(false);
    expect(placeNameMatchesLabel("Tel Aviv", "")).toBe(false);
  });
});

describe("riverAnchor — the course's own arc-length midpoint and local direction", () => {
  test("a straight horizontal line anchors at its exact midpoint with a 0-degree angle", () => {
    const anchor = riverAnchor([{ x: 0, y: 10 }, { x: 100, y: 10 }])!;
    expect(anchor.point).toEqual({ x: 50, y: 10 });
    expect(anchor.angleDeg).toBeCloseTo(0, 6);
  });

  test("a straight vertical line anchors at a 90-degree angle", () => {
    const anchor = riverAnchor([{ x: 10, y: 0 }, { x: 10, y: 100 }])!;
    expect(anchor.angleDeg).toBeCloseTo(90, 6);
  });

  test("a bent line's midpoint falls on whichever segment the half-length point lands in, not a straight-line average of the endpoints", () => {
    // Total length 150 (100 + 50); the midpoint (75) falls 75 units along the
    // first segment — still inside it (100 long) — so the anchor sits on the
    // FIRST segment's own line, not at the geometric centroid of all three points.
    const anchor = riverAnchor([{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 50 }])!;
    expect(anchor.point).toEqual({ x: 75, y: 0 });
    expect(anchor.angleDeg).toBeCloseTo(0, 6);
  });

  test("fewer than two points returns null rather than throwing", () => {
    expect(riverAnchor([{ x: 0, y: 0 }])).toBeNull();
    expect(riverAnchor([])).toBeNull();
  });
});

describe("riverOrientation — vertical-writing choice for a near-vertical CJK river", () => {
  test("a near-vertical river in a CJK context switches to vertical writing-mode, not rotated text", () => {
    expect(riverOrientation(85, true)).toEqual({ mode: "vertical", angleDeg: 0 });
    expect(riverOrientation(-80, true)).toEqual({ mode: "vertical", angleDeg: 0 });
  });

  test("the SAME near-vertical angle in a non-CJK context still rotates — vertical writing-mode is a CJK-only convention", () => {
    const result = riverOrientation(85, false);
    expect(result.mode).toBe("rotate");
    expect(result.angleDeg).toBeCloseTo(85, 6);
  });

  test("a middling angle (30-60 degrees) snaps to plain horizontal in CJK rather than reading poorly at a shallow rotation", () => {
    expect(riverOrientation(45, true)).toEqual({ mode: "horizontal", angleDeg: 0 });
  });

  test("a shallow angle rotates along the river in both CJK and non-CJK contexts", () => {
    expect(riverOrientation(10, true)).toEqual({ mode: "rotate", angleDeg: 10 });
    expect(riverOrientation(10, false)).toEqual({ mode: "rotate", angleDeg: 10 });
  });

  test("an angle past 90 degrees is normalized into [-90, 90], keeping the same on-screen line direction", () => {
    const result = riverOrientation(170, false); // equivalent to -10
    expect(result.angleDeg).toBeCloseTo(-10, 6);
  });
});
