/**
 * labels.ts — the decorative place-name layer: major cities, mountain
 * ranges, peaks and rivers, from `labels.json` (`emit::place_map_labels`).
 *
 * Split in two, like every other geometry module in this directory
 * (`clusters.ts` vs `markers.ts`): the pure half below (language selection,
 * river orientation, greedy placement with rectangle collisions) has no DOM
 * and is what `__tests__/labels.test.ts` exercises directly; `LabelLayer` at
 * the bottom is the thin DOM wrapper `map.ts` drives on every camera settle.
 *
 * Placement is greedy by priority (Natural Earth's own `rank`, lower is more
 * major — a city label whose OWN marker is in view sorts first, ahead of
 * even a rank-0 capital merely visible nearby, so the article's own subject
 * is never crowded out of the area budget by unrelated top-rank names) and
 * never overlaps another label, a marker, or a reserved UI region. A city
 * label may sit beside a work marker only when that city is one of the
 * marker's own works' own declared places (`placeNameMatchesLabel`); a city
 * merely covered by an unrelated marker is dropped outright, never
 * re-anchored onto it — anything else would occasionally rename one real
 * place's marker after a different, merely-nearby one.
 */
import { worldToScreen } from "./camera";
import { CLUSTER_DISTANCE, clusterVisible } from "./clusters";
import { langBucket } from "../subscribe/i18n";
import { project } from "./projection";
import { worldCentroid, type WorkPoint } from "./markers";
import type { Camera, LabelPoint, LabelRiver, LabelsData, LanguageLabels, Place, Point, Rect, Viewport, Work } from "./types";

export type LabelKind = "city" | "range" | "peak" | "river";

/** One label per this many screen px², minimum 5 regardless of frame size — the design's own tuned budget, counted only against the visible frame so a small embed still orients a reader without needing its own separate constant. */
export const LABEL_AREA_BUDGET_PX2 = 45000;
export const LABEL_MIN_COUNT = 5;

const LABEL_GAP = 6;
/** Screen px within which a city label's own point counts as "on" a marker rather than merely near it. */
const MARKER_COINCIDENCE_PX = 20;
/** Gap from a marker's own edge once a city label has to clear it, wider than the plain `LABEL_GAP` a label in open water uses. */
const MARKER_CLEAR_GAP = 26;
const LABEL_KIND_ORDER: Record<LabelKind, number> = { city: 0, peak: 1, range: 2, river: 3 };

const EMPTY_LANGUAGE_LABELS: LanguageLabels = { cities: [], ranges: [], peaks: [], rivers: [] };

function isLanguageLabels(value: LanguageLabels | string[] | undefined): value is LanguageLabels {
  return value != null && !Array.isArray(value);
}

/**
 * Which of `data`'s language keys this page reads, bucketed off `<html
 * lang>` the same way `strings.ts`'s own runtime copy is: a `zh-hant` page
 * reads the pack's `zh-Hant` set when the build included it, every other
 * bucket (`en`, `zh-hans`) reads `en`. Falls back to whichever language the
 * build DID publish when the page's own preferred tag is not among them (a
 * `zh-Hans` page on a build that published only `zh-Hant`, say) rather than
 * showing no labels at all — `labels.json`'s own `languages` list is never
 * empty (`emit::place_map_labels::label_language_tags` always names at
 * least `en`), so this only returns the empty set when `data` itself is
 * absent (the handshake carried no `data-labels`, or the fetch failed).
 */
export function selectLanguageLabels(data: LabelsData | null | undefined, lang: string): LanguageLabels {
  if (!data || !Array.isArray(data.languages) || data.languages.length === 0) return EMPTY_LANGUAGE_LABELS;
  const preferred = langBucket(lang) === "zh-hant" ? "zh-Hant" : "en";
  const tag = data.languages.includes(preferred) ? preferred : data.languages[0];
  const picked = data[tag];
  return isLanguageLabels(picked) ? picked : EMPTY_LANGUAGE_LABELS;
}

/**
 * The course's own midpoint by arc length, and the local direction (degrees,
 * `atan2` range) of the segment it falls on — the point a river's name
 * anchors to and the angle it follows. `null` for a line with fewer than two
 * points (never emitted by the build — `place_map::labels::RiverLabel`
 * rejects one at decode time — but a defensive caller never assumes it).
 */
export function riverAnchor(line: Point[]): { point: Point; angleDeg: number } | null {
  if (line.length < 2) return null;
  const segmentLengths: number[] = [];
  let total = 0;
  for (let i = 1; i < line.length; i++) {
    const length = Math.hypot(line[i].x - line[i - 1].x, line[i].y - line[i - 1].y);
    segmentLengths.push(length);
    total += length;
  }
  if (total === 0) return { point: line[0], angleDeg: 0 };
  const target = total / 2;
  let covered = 0;
  for (let i = 0; i < segmentLengths.length; i++) {
    const length = segmentLengths[i];
    if (covered + length >= target || i === segmentLengths.length - 1) {
      const a = line[i];
      const b = line[i + 1];
      const t = length > 0 ? Math.min(1, Math.max(0, (target - covered) / length)) : 0;
      return {
        point: { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t },
        angleDeg: (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI,
      };
    }
    covered += length;
  }
  /* istanbul ignore next -- the loop above always returns on its final iteration */
  return null;
}

export type RiverOrientation = { mode: "vertical" | "horizontal"; angleDeg: 0 } | { mode: "rotate"; angleDeg: number };

/**
 * CJK cartographic convention: a river running close to vertical sets
 * upright characters stacked top-to-bottom (`writing-mode: vertical-rl`)
 * rather than rotating a horizontal run of glyphs onto its side, which reads
 * poorly for CJK the way it does not for Latin. `angleDeg` is first
 * normalized into [-90, 90] — a raw angle past 90 either way would render
 * rotated text upside down; flipping 180 degrees keeps the same on-screen
 * LINE direction, just read from the other end. Non-CJK text always
 * rotates, never switches to vertical writing-mode.
 */
export function riverOrientation(angleDeg: number, vertical: boolean): RiverOrientation {
  let angle = angleDeg;
  if (angle > 90) angle -= 180;
  else if (angle < -90) angle += 180;
  if (vertical) {
    const steepness = Math.abs(angle);
    if (steepness > 60) return { mode: "vertical", angleDeg: 0 };
    if (steepness > 30) return { mode: "horizontal", angleDeg: 0 };
  }
  return { mode: "rotate", angleDeg: angle };
}

/**
 * Whether `placeName` (one of a work's own declared places) names the SAME
 * real place `labelName` (a Natural Earth label) does — bidirectional
 * substring, so a locally-named sub-area ("Tokyo Shibuya") still counts as
 * its city ("Tokyo") without a false match between two merely-nearby,
 * differently-named places. Ported from the source prototype's own fix for
 * exactly that false match (a marker read a different nearby city's name
 * purely because the two happened to render close together at a given
 * zoom).
 */
export function placeNameMatchesLabel(placeName: string, labelName: string): boolean {
  if (!placeName || !labelName) return false;
  return placeName === labelName || placeName.includes(labelName) || labelName.includes(placeName);
}

/** One label per this many screen px², minimum `minCount`, counted only against the visible frame — shrinks automatically for a small embed with no separate constant. */
export function labelBudget(viewport: Viewport, areaBudgetPx2: number = LABEL_AREA_BUDGET_PX2, minCount: number = LABEL_MIN_COUNT): number {
  return Math.max(minCount, Math.round((viewport.width * viewport.height) / areaBudgetPx2));
}

function candidateBox(anchor: Point, width: number, height: number, position: LabelPosition, gap: number): Rect {
  if (position === "right") return { x: anchor.x + gap, y: anchor.y - height / 2, width, height };
  if (position === "left") return { x: anchor.x - gap - width, y: anchor.y - height / 2, width, height };
  if (position === "above") return { x: anchor.x - width / 2, y: anchor.y - gap - height, width, height };
  if (position === "below") return { x: anchor.x - width / 2, y: anchor.y + gap, width, height };
  return { x: anchor.x - width / 2, y: anchor.y - height / 2, width, height }; // "center": range/peak/river
}

function boxesOverlap(a: Rect, b: Rect, pad = 2): boolean {
  return a.x - pad < b.x + b.width && a.x + a.width + pad > b.x && a.y - pad < b.y + b.height && a.y + a.height + pad > b.y;
}

function withinStage(box: Rect, viewport: Viewport): boolean {
  return box.x >= 0 && box.y >= 0 && box.x + box.width <= viewport.width && box.y + box.height <= viewport.height;
}

export type LabelPosition = "right" | "left" | "above" | "below" | "center";

/** One label this settle might place — already projected and camera-transformed to a screen anchor, already measured. */
export interface LabelCandidate {
  id: string;
  kind: LabelKind;
  /** The display name this candidate was built with — the own-place rule's other half, alongside a marker's own works. */
  name: string;
  /** Natural Earth's own `rank` (0 = most major); lower sorts first. */
  priority: number;
  screen: Point;
  /** Measured, already axis-aligned box size in CSS px — a rotated or vertical-writing-mode river's own rendered footprint, not its unrotated layout box. */
  width: number;
  height: number;
}

/** A rendered marker (or cluster) the label layer must never cover, and whose own works decide the own-place rule for a coincident city label. */
export interface LabelMarkerBox {
  screen: Point;
  /** Work ids this marker (or cluster) represents. */
  workIds: string[];
}

export interface PlacedLabel {
  id: string;
  position: LabelPosition;
  box: Rect;
  /** A city label's own anchor dot — present only when the name had to move aside from empty space with no marker on the point; `null` for every non-city kind, and for a city whose marker already stands on the point (the marker IS the dot). */
  dot: Point | null;
}

export interface PlaceLabelsOptions {
  viewport: Viewport;
  markers: LabelMarkerBox[];
  /** Boxes a label must never cover beyond the markers themselves — the zoom controls, a breadcrumb/scope chip, the card row. */
  reserved: Rect[];
  areaBudgetPx2?: number;
  minCount?: number;
  /** True when `candidate` (a city label) is one of `marker`'s own works' own declared places — see `placeNameMatchesLabel`. */
  ownPlace: (candidate: LabelCandidate, marker: LabelMarkerBox) => boolean;
}

/**
 * Greedy by priority: walk candidates in order, keep a running list of
 * occupied boxes seeded with every marker and reserved region, and place a
 * label only in the first candidate position that clears all of them. A
 * city whose own point a marker stands on is a special case (see the module
 * doc's own-place rule); every other kind is center-anchored and simply
 * dropped on collision, never slid to a second position.
 */
export function placeLabels(candidates: LabelCandidate[], options: PlaceLabelsOptions): Map<string, PlacedLabel> {
  const { viewport, markers, reserved, ownPlace } = options;
  const budget = labelBudget(viewport, options.areaBudgetPx2, options.minCount);
  const markerRects: Rect[] = markers.map((marker) => ({ x: marker.screen.x - 22, y: marker.screen.y - 22, width: 44, height: 44 }));
  const placed: Rect[] = [...markerRects, ...reserved];

  const matchesOwnMarker = (candidate: LabelCandidate): boolean =>
    candidate.kind === "city" && markers.some((marker) => ownPlace(candidate, marker));
  const effectivePriority = (candidate: LabelCandidate): number => (matchesOwnMarker(candidate) ? -1 : candidate.priority);
  const ordered = [...candidates].sort(
    (a, b) => effectivePriority(a) - effectivePriority(b) || LABEL_KIND_ORDER[a.kind] - LABEL_KIND_ORDER[b.kind],
  );

  const results = new Map<string, PlacedLabel>();
  for (const candidate of ordered) {
    if (results.size >= budget) break;
    const anchor = candidate.screen;
    if (anchor.x < 0 || anchor.x > viewport.width || anchor.y < 0 || anchor.y > viewport.height) continue;

    if (candidate.kind === "city") {
      const covering = markers.find((marker) => Math.hypot(marker.screen.x - anchor.x, marker.screen.y - anchor.y) < MARKER_COINCIDENCE_PX);
      // A marker covering this city's own point always wins the exact spot;
      // if it's covering because this genuinely IS that marker's place, the
      // NAME still earns its place beside it (below). If a DIFFERENT marker
      // merely happens to render here, the city is dropped entirely — never
      // re-anchored onto a marker it has nothing to do with.
      if (covering && !ownPlace(candidate, covering)) continue;
      const boxAnchor = covering ? covering.screen : anchor;
      const gap = covering ? MARKER_CLEAR_GAP : LABEL_GAP;
      let chosen: { position: LabelPosition; box: Rect } | null = null;
      for (const position of ["right", "left", "above", "below"] as const) {
        const box = candidateBox(boxAnchor, candidate.width, candidate.height, position, gap);
        if (!withinStage(box, viewport)) continue;
        if (placed.some((existing) => boxesOverlap(box, existing))) continue;
        chosen = { position, box };
        break;
      }
      if (!chosen) continue;
      placed.push(chosen.box);
      results.set(candidate.id, { id: candidate.id, position: chosen.position, box: chosen.box, dot: covering ? null : anchor });
    } else {
      const box = candidateBox(anchor, candidate.width, candidate.height, "center", 0);
      if (!withinStage(box, viewport)) continue;
      if (placed.some((existing) => boxesOverlap(box, existing))) continue;
      placed.push(box);
      results.set(candidate.id, { id: candidate.id, position: "center", box, dot: null });
    }
  }
  return results;
}

// ---------------------------------------------------------------------------
// LabelLayer — the DOM half. Builds every label element once (off-screen,
// hidden) at construction, measuring each exactly once since neither its
// text nor its orientation ever changes; every `render()` call after that
// only recomputes screen positions and toggles which ones are shown, the
// same "measure once, reposition every settle" split `tiles.ts`'s own
// cached fetches use for a different resource.
// ---------------------------------------------------------------------------

interface BuiltLabel {
  id: string;
  kind: LabelKind;
  name: string;
  priority: number;
  /** World-space (projected) anchor point — a point label's own coordinate, or a river's own arc-length midpoint. */
  point: Point;
  el: HTMLElement;
  /** A city's own anchor dot, shown only when its text moved away from an empty point; `null` for every other kind. */
  dot: HTMLElement | null;
  width: number;
  height: number;
}

export class LabelLayer {
  private readonly container: HTMLElement;
  private readonly labels: BuiltLabel[] = [];

  constructor(container: HTMLElement, data: LabelsData | null | undefined, lang: string) {
    this.container = container;
    const picked = selectLanguageLabels(data, lang);
    const vertical = langBucket(lang) === "zh-hant";
    let nextId = 0;

    for (const city of picked.cities) this.labels.push(this.buildPoint(`city-${nextId++}`, "city", city));
    for (const range of picked.ranges) this.labels.push(this.buildPoint(`range-${nextId++}`, "range", range));
    for (const peak of picked.peaks) this.labels.push(this.buildPoint(`peak-${nextId++}`, "peak", peak));
    for (const river of picked.rivers) {
      const built = this.buildRiver(`river-${nextId++}`, river, vertical);
      if (built) this.labels.push(built);
    }
  }

  /** Reposition (and show/hide) every label for the current camera/viewport/markers. Never called mid-gesture — `places-explorer.css` hides this whole layer while `.moss-places-world` carries `[data-gesture]`, so a stale position is never visible, and skipping the work here is what "cheaply during a gesture" means in practice. */
  render(points: WorkPoint[], camera: Camera, viewport: Viewport, worksById: Map<string, Work>, placesById: Map<string, Place>, reserved: Rect[]): void {
    if (this.labels.length === 0 || viewport.width <= 0 || viewport.height <= 0) return;
    const markers = this.markerBoxesFor(points, camera, viewport);
    const candidates: LabelCandidate[] = this.labels.map((label) => ({
      id: label.id,
      kind: label.kind,
      name: label.name,
      priority: label.priority,
      screen: worldToScreen(label.point, camera, viewport),
      width: label.width,
      height: label.height,
    }));
    const ownPlace = (candidate: LabelCandidate, marker: LabelMarkerBox): boolean =>
      marker.workIds.some((id) => {
        const work = worksById.get(id);
        if (!work) return false;
        return work.places.some((placeId) => {
          const place = placesById.get(placeId);
          return place != null && placeNameMatchesLabel(place.name, candidate.name);
        });
      });

    const placed = placeLabels(candidates, { viewport, markers, reserved, ownPlace });
    for (const label of this.labels) {
      const result = placed.get(label.id);
      if (!result) {
        label.el.hidden = true;
        if (label.dot) label.dot.hidden = true;
        continue;
      }
      label.el.hidden = false;
      if (result.position === "center") {
        // Non-city kinds (range/peak/river) are always "center"-positioned
        // and carry their own `translate(-50%, -50%)` (composed with a
        // rotated river's own `rotate(...)`), set once at construction —
        // `left`/`top` here is the box's CENTER (which equals the
        // candidate's own screen anchor, since `candidateBox`'s "center"
        // branch always centers the box on it), never its literal top-left
        // corner. CSS `left`/`top` position the element's UNrotated layout
        // box; for a rotated river label that is NOT the same corner
        // `result.box` names (which is the ROTATED, measured bbox
        // `placeLabels` reserved for collision) — centering by the shared
        // anchor point sidesteps that mismatch entirely rather than
        // re-deriving an offset between the two boxes.
        label.el.style.left = `${result.box.x + result.box.width / 2}px`;
        label.el.style.top = `${result.box.y + result.box.height / 2}px`;
      } else {
        label.el.style.left = `${result.box.x}px`;
        label.el.style.top = `${result.box.y}px`;
      }
      if (label.dot) {
        if (result.dot) {
          label.dot.hidden = false;
          label.dot.style.left = `${result.dot.x}px`;
          label.dot.style.top = `${result.dot.y}px`;
        } else {
          label.dot.hidden = true;
        }
      }
    }
  }

  /** Marker boxes at their own ACTUAL rendered screen position — the same cluster-centroid-then-`worldToScreen` `markers.ts` draws a button at, not `clusters.ts`'s own zoom-scaled, pan-independent `ProximityCluster.screen` (meaningful only for distance comparisons between clusters, never for placing anything on the page). A ring's own fanned-out dots are left out of this reservation on purpose — a bloomed ring is a transient, already-attention-grabbing state, and the small cost of a label occasionally sitting near a ring leg is cheaper than threading the ring's own layout through this unrelated module. */
  private markerBoxesFor(points: WorkPoint[], camera: Camera, viewport: Viewport): LabelMarkerBox[] {
    const byId = new Map(points.map((point) => [point.id, point]));
    return clusterVisible(points, camera.zoom, CLUSTER_DISTANCE).map((cluster) => {
      const members = cluster.ids.map((id) => byId.get(id)).filter((point): point is WorkPoint => point != null);
      return { screen: worldToScreen(worldCentroid(members), camera, viewport), workIds: cluster.ids };
    });
  }

  private buildPoint(id: string, kind: "city" | "range" | "peak", label: LabelPoint): BuiltLabel {
    const el = document.createElement("span");
    el.className = "moss-places-label";
    el.dataset.kind = kind;
    el.setAttribute("aria-hidden", "true");
    el.textContent = kind === "peak" ? `▲ ${label.name}` : label.name;
    el.style.position = "absolute";
    // A city is positioned by its own box's literal top-left (one of
    // right/left/above/below the marker it may sit beside); range and peak
    // are always center-anchored, so their own element centers itself on
    // whatever point `render()` sets as left/top instead.
    if (kind !== "city") el.style.transform = "translate(-50%, -50%)";
    el.hidden = true;
    this.container.append(el);

    let dot: HTMLElement | null = null;
    if (kind === "city") {
      dot = document.createElement("i");
      dot.className = "moss-places-label-dot";
      dot.setAttribute("aria-hidden", "true");
      dot.style.position = "absolute";
      dot.hidden = true;
      this.container.append(dot);
    }

    const { width, height } = this.measure(el);
    return { id, kind, name: label.name, priority: label.rank, point: project(label.lat, label.lng), el, dot, width, height };
  }

  private buildRiver(id: string, river: LabelRiver, vertical: boolean): BuiltLabel | null {
    const worldLine = river.line.map(([lng, lat]) => project(lat, lng));
    const anchor = riverAnchor(worldLine);
    if (!anchor) return null;
    const orientation = riverOrientation(anchor.angleDeg, vertical);

    const el = document.createElement("span");
    el.className = "moss-places-label";
    el.dataset.kind = "river";
    el.setAttribute("aria-hidden", "true");
    el.textContent = river.name;
    el.style.position = "absolute";
    // Always center-anchored (like range/peak); a rotated river composes
    // the same centering translate with its own rotation so the rotation
    // pivots on the already-centered point, not the box's pre-rotation
    // corner.
    if (orientation.mode === "vertical") {
      el.style.writingMode = "vertical-rl";
      el.style.textOrientation = "upright";
      el.style.transform = "translate(-50%, -50%)";
    } else if (orientation.mode === "rotate") {
      el.style.transform = `translate(-50%, -50%) rotate(${orientation.angleDeg}deg)`;
    } else {
      el.style.transform = "translate(-50%, -50%)";
    }
    el.hidden = true;
    this.container.append(el);

    // `getBoundingClientRect` already reads the POST-transform box — a
    // rotated river label's real on-screen footprint — so, unlike measuring
    // `offsetWidth`/`offsetHeight` (the UNrotated layout box), nothing here
    // has to re-derive an axis-aligned bounding box from the rotation angle
    // by hand.
    const { width, height } = this.measure(el);
    return { id, kind: "river", name: river.name, priority: river.rank, point: anchor.point, el, dot: null, width, height };
  }

  /** Measure `el`'s real rendered box: briefly shown (`visibility: hidden`, so it takes layout space without flashing or affecting hit-testing) just long enough for `getBoundingClientRect`, then returned to `hidden` (removed from layout, the resting state every label starts and ends a render in unless `render()` chose it). */
  private measure(el: HTMLElement): { width: number; height: number } {
    el.hidden = false;
    el.style.visibility = "hidden";
    const rect = el.getBoundingClientRect();
    el.hidden = true;
    el.style.visibility = "";
    return { width: rect.width, height: rect.height };
  }
}
