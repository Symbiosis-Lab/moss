/**
 * markers.ts — one marker per in-scope work, clustered by proximity, and
 * the coincident-cluster ring.
 *
 * Placement: a work's FIRST place (`work.places[0]`) only — the same
 * one-dot-per-work choice `places.<hash>.json` itself already made at build
 * time (`place_map/places_data.rs`'s module doc), so this module never has
 * to decide among a work's several places on its own. A work with no
 * resolvable first place is skipped (not merely placeless: `places_data.rs`
 * never emits a work whose declared locations resolved to no coordinates at
 * all, so an id a `places` lookup misses here means stale/malformed data,
 * not a legitimately placeless work).
 *
 * Clustering and the ring reuse `clusters.ts`'s pan-independent geometry
 * directly; this module's own job is turning that geometry into DOM: one
 * `.moss-places-marker` button per cluster, or — once a cluster is proven
 * unable to ever separate by zooming (`staysMergedAtMaxZoom`) — a bloomed
 * ring of one `.moss-places-ring-dot` per member, exactly the design's case
 * 2/3 split.
 */
import { worldToScreen } from "./camera";
import { byDateDescThenTitle } from "./cards";
import {
  CLUSTER_DISTANCE,
  RING_MAX,
  RING_ZOOM_STEP,
  clusterVisible,
  ringLayout,
  ringZoom,
  staysMergedAtMaxZoom,
  type IdPoint,
  type ProximityCluster,
} from "./clusters";
import { project } from "./projection";
import { inScope } from "./scope";
import { hasPoint, type Camera, type Place, type Point, type Precision, type Scope, type Viewport, type Work } from "./types";
import type { PlacesStrings } from "./strings";
import { worksHereLabel } from "./strings";

/** One in-scope work, projected to its own first place. */
export interface WorkPoint extends IdPoint {
  precision: Precision;
}

const PRECISION_COARSENESS: Record<Precision, number> = { exact: 0, city: 1, region: 2, country: 3 };

/** The least precise (most privacy-coarse) of a cluster's own members — never show a merged marker at a precision finer than its vaguest member implies. */
function coarsestPrecision(precisions: Precision[]): Precision {
  return precisions.reduce((coarsest, next) =>
    PRECISION_COARSENESS[next] > PRECISION_COARSENESS[coarsest] ? next : coarsest,
  "exact" as Precision);
}

/** In-scope works, each projected to its own first resolvable place. Pure — no DOM, no camera. */
export function pointsForWorks(works: Work[], places: Place[], scope: Scope): WorkPoint[] {
  const byId = new Map(places.map((place) => [place.id, place]));
  const points: WorkPoint[] = [];
  for (const work of works) {
    if (!inScope(work, places, scope)) continue;
    const place = byId.get(work.places[0] ?? "");
    // A grouping node (no coordinates) is never a work's own resolved
    // place — `places_data.rs` only ever puts one of those in `places`,
    // never in a `WorkEntry`'s own `places` ids — but the type can't say
    // that on its own, so this still narrows rather than asserting past it.
    if (!place || !hasPoint(place)) continue;
    const projected = project(place.lat, place.lng);
    points.push({ id: work.id, x: projected.x, y: projected.y, precision: place.precision });
  }
  return points;
}

export interface MarkerCallbacks {
  /** Zoom to separate a cluster that CAN separate (case 1: zoom-to-fit). */
  fitPoints(points: Point[]): void;
  /** Centre on a single point at `zoom`, without necessarily fitting past it (a ring's own bloom). */
  focusPoint(point: Point, zoom: number): void;
  /** Select (and expand) a work; `null` clears the selection. */
  selectWork(id: string | null): void;
  /** Restrict the card row to exactly these work ids, or `null` to lift the restriction. */
  scopeRow(ids: Set<string> | null): void;
  announce(message: string): void;
  /** The current detail ceiling (zoom units) — rises once the camera has crossed into tile detail. */
  maxZoom(): number;
}

interface RingState {
  key: string;
  members: WorkPoint[];
}

/** A plain marker's own persistent DOM, reused across renders by `clusterKey`. `cluster`/`members` are mutated in place on every render so the button's own `click` closure (bound once, at creation) always reads the current data instead of the snapshot from whenever the button was first built. */
interface MarkerEntry {
  button: HTMLButtonElement;
  cluster: ProximityCluster;
  members: WorkPoint[];
}

/** An open ring's own persistent DOM. `dots` is keyed by work id (stable across a ring's own lifetime, unlike array index) so a mid-gesture settle reuses the SAME dot element a pointer sequence may already be resolving a click against. Legs carry no listener, so they are simplest rebuilt each render rather than diffed. */
interface RingEntry {
  key: string;
  anchor: HTMLButtonElement;
  legs: HTMLElement[];
  dots: Map<string, HTMLButtonElement>;
}

export class MarkerLayer {
  private readonly container: HTMLElement;
  private readonly callbacks: MarkerCallbacks;
  private strings: PlacesStrings;
  private lang: string;
  private ring: RingState | null = null;
  private lastPoints: WorkPoint[] = [];
  private lastCamera: Camera = { x: 0, y: 0, zoom: 1 };
  /** Work ids exempted from the ring's own dimming attribute while the scope chip's menu has an item hovered/focused — `null` lifts the exemption. Independent of `ring`: the chip never shows a dig-down menu while a ring is open (its terminal crumb is the ring itself), so the two are never both active. */
  private highlightIds: Set<string> | null = null;
  // Persistent DOM, reused across `render()` calls — see `MarkerEntry`/
  // `RingEntry`. `container.replaceChildren()` every render used to mean
  // ANY settle (a resize, a tile arriving, not just a gesture's own end)
  // could replace a marker or ring-dot button mid-click: the browser's own
  // click synthesis resolves against the element `pointerdown` landed on,
  // and a node swapped out between `pointerdown` and `pointerup` leaves it
  // nothing to fire `click` on. Reusing the same node when a cluster's own
  // identity (its member ids) hasn't changed closes that window.
  private markerEntries = new Map<string, MarkerEntry>();
  private ringEntry: RingEntry | null = null;

  constructor(container: HTMLElement, callbacks: MarkerCallbacks, strings: PlacesStrings, lang: string) {
    this.container = container;
    this.callbacks = callbacks;
    this.strings = strings;
    this.lang = lang;
    // Document-level, not just the markers layer: a clicked marker does not
    // reliably take focus in every engine (WebKit historically does not
    // focus a button on click), so scoping this to the container alone
    // would miss Escape whenever focus never actually moved there.
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.ring) {
        event.preventDefault();
        this.closeRing();
      }
    });
    document.addEventListener("pointerdown", (event) => {
      if (!this.ring) return;
      if (event.target instanceof Node && this.container.contains(event.target)) return;
      this.closeRing();
    });
  }

  hasOpenRing(): boolean {
    return this.ring != null;
  }

  /** The open ring's own member count, or `null` when no ring is open — the scope chip's own trailing "N here" crumb reads this. */
  ringCount(): number | null {
    return this.ring?.members.length ?? null;
  }

  /** Exempt exactly these work ids from dimming, `null` to lift the exemption — the chip menu's hover/focus highlight, reusing the ring's own `data-dimmed` attribute rather than a second one. Does not itself re-render; the caller re-renders (`map.ts`'s `applyCamera`). */
  setHighlight(ids: Set<string> | null): void {
    this.highlightIds = ids;
  }

  closeRing(): void {
    if (!this.ring) return;
    this.ring = null;
    this.callbacks.scopeRow(null);
    this.callbacks.announce(this.strings.scopeCleared);
  }

  /** Repaint every marker (and the ring, if one is open) for the current camera/viewport/scope/selection. */
  render(points: WorkPoint[], camera: Camera, viewport: Viewport, selectedId: string | null, works: Map<string, Work>): void {
    this.lastPoints = points;
    this.lastCamera = camera;
    const clusters = clusterVisible(points, camera.zoom, CLUSTER_DISTANCE);

    // A pan/zoom can re-cluster the bloomed group's own coordinate into (or
    // out of) another cluster — fold the ring back first, same guard the
    // prototype's own positionMarkers() applies, so the render below never
    // has to special-case a ring whose cluster no longer exists.
    if (this.ring && !clusters.some((cluster) => clusterKey(cluster.ids) === this.ring!.key)) {
      this.ring = null;
      this.callbacks.scopeRow(null);
    }

    const byId = new Map(points.map((point) => [point.id, point]));
    const seenMarkerKeys = new Set<string>();

    for (const cluster of clusters) {
      const key = clusterKey(cluster.ids);
      if (this.ring && key === this.ring.key) {
        this.renderRing(key, cluster, byId, camera, viewport, selectedId, works);
        continue;
      }
      seenMarkerKeys.add(key);
      this.renderMarker(key, cluster, byId, camera, viewport, selectedId, works);
    }

    // A cluster from a previous render that no longer exists this one
    // (re-clustered away, or scrolled out of scope) — only now is its
    // button actually removed.
    for (const [key, entry] of this.markerEntries) {
      if (!seenMarkerKeys.has(key)) {
        entry.button.remove();
        this.markerEntries.delete(key);
      }
    }
    // The ring itself closed, or re-clustered into a different key, since
    // the last render that had one open.
    if (this.ringEntry && (!this.ring || this.ringEntry.key !== this.ring.key)) {
      this.ringEntry.anchor.remove();
      this.ringEntry.legs.forEach((leg) => leg.remove());
      this.ringEntry.dots.forEach((dot) => dot.remove());
      this.ringEntry = null;
    }
  }

  private renderMarker(
    key: string,
    cluster: ProximityCluster,
    byId: Map<string, WorkPoint>,
    camera: Camera,
    viewport: Viewport,
    selectedId: string | null,
    works: Map<string, Work>,
  ): void {
    const members = cluster.ids.map((id) => byId.get(id)).filter((point): point is WorkPoint => point != null);
    const screen = worldToScreen(worldCentroid(members), camera, viewport);

    let entry = this.markerEntries.get(key);
    if (!entry) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "moss-places-marker";
      this.container.append(button);
      entry = { button, cluster, members };
      this.markerEntries.set(key, entry);
      // Reads `entry.cluster`/`entry.members`, not the `cluster`/`members`
      // captured here — those two fields are updated in place below on
      // every later render that reuses this same button, so a click long
      // after creation still activates with current data.
      button.addEventListener("click", () => this.activate(entry!.cluster, entry!.members));
    } else {
      entry.cluster = cluster;
      entry.members = members;
    }

    const button = entry.button;
    button.style.left = `${screen.x}px`;
    button.style.top = `${screen.y}px`;
    button.dataset.precision = coarsestPrecision(members.map((member) => member.precision));
    if (cluster.ids.length > 1) button.dataset.count = String(cluster.ids.length);
    else delete button.dataset.count;
    if (cluster.ids.some((id) => id === selectedId)) button.dataset.selected = "true";
    else delete button.dataset.selected;
    const dimmedByHighlight = this.highlightIds != null && !cluster.ids.some((id) => this.highlightIds!.has(id));
    if (this.ring || dimmedByHighlight) button.dataset.dimmed = "";
    else delete button.dataset.dimmed;
    button.setAttribute("aria-label", this.labelFor(cluster.ids, works));
  }

  private renderRing(
    key: string,
    cluster: ProximityCluster,
    byId: Map<string, WorkPoint>,
    camera: Camera,
    viewport: Viewport,
    selectedId: string | null,
    works: Map<string, Work>,
  ): void {
    const members = cluster.ids.map((id) => byId.get(id)).filter((point): point is WorkPoint => point != null);
    const anchorScreen = worldToScreen(worldCentroid(members), camera, viewport);

    let ringEntry = this.ringEntry;
    if (!ringEntry || ringEntry.key !== key) {
      // Switching rings within one render is not a real case today (a ring
      // only ever opens from a fresh click, never while another is still
      // open), but drop any stale one defensively rather than leak it.
      if (ringEntry) {
        ringEntry.anchor.remove();
        ringEntry.legs.forEach((leg) => leg.remove());
        ringEntry.dots.forEach((dot) => dot.remove());
      }
      const anchor = document.createElement("button");
      anchor.type = "button";
      anchor.className = "moss-places-marker";
      anchor.dataset.ringAnchor = "";
      this.container.append(anchor);
      anchor.addEventListener("click", () => this.closeRing());
      ringEntry = { key, anchor, legs: [], dots: new Map() };
      this.ringEntry = ringEntry;
    }
    ringEntry.anchor.style.left = `${anchorScreen.x}px`;
    ringEntry.anchor.style.top = `${anchorScreen.y}px`;
    ringEntry.anchor.setAttribute("aria-label", this.labelFor(cluster.ids, works));

    // Legs carry no listener and are cheap to rebuild — unlike the anchor
    // and the dots below, there is no click race to protect here.
    ringEntry.legs.forEach((leg) => leg.remove());
    ringEntry.legs = [];

    // Row order, not cluster.ids' own id-sorted order (clusters.ts sorts by
    // id purely for deterministic merging, see its own doc) — the ring's
    // dots must read in the SAME order the scoped card row below shows the
    // same members in, both derived from the one date-desc-then-title rule.
    const orderedIds = [...cluster.ids].sort((a, b) => {
      const workA = works.get(a);
      const workB = works.get(b);
      return workA && workB ? byDateDescThenTitle(workA, workB) : 0;
    });
    const layout = ringLayout(orderedIds.length, anchorScreen);
    const positions = layout.positions ?? orderedIds.map(() => anchorScreen);
    const seenDotIds = new Set<string>();
    orderedIds.forEach((id, index) => {
      const position = positions[index];
      const leg = document.createElement("div");
      leg.className = "moss-places-ring-leg";
      const dx = position.x - anchorScreen.x;
      const dy = position.y - anchorScreen.y;
      const length = Math.hypot(dx, dy);
      const angle = (Math.atan2(dy, dx) * 180) / Math.PI;
      leg.style.left = `${anchorScreen.x}px`;
      leg.style.top = `${anchorScreen.y}px`;
      leg.style.width = `${length}px`;
      leg.style.transform = `rotate(${angle}deg)`;
      this.container.append(leg);
      ringEntry!.legs.push(leg);

      seenDotIds.add(id);
      let dot = ringEntry!.dots.get(id);
      if (!dot) {
        dot = document.createElement("button");
        dot.type = "button";
        dot.className = "moss-places-ring-dot";
        this.container.append(dot);
        dot.addEventListener("click", () => this.callbacks.selectWork(id));
        ringEntry!.dots.set(id, dot);
      }
      const work = works.get(id);
      dot.style.left = `${position.x}px`;
      dot.style.top = `${position.y}px`;
      if (id === selectedId) dot.dataset.selected = "true";
      else delete dot.dataset.selected;
      dot.setAttribute("aria-label", work ? (work.title || this.strings.untitled) : id);
    });
    for (const [id, dot] of ringEntry.dots) {
      if (!seenDotIds.has(id)) {
        dot.remove();
        ringEntry.dots.delete(id);
      }
    }
  }

  private labelFor(ids: string[], works: Map<string, Work>): string {
    if (ids.length === 1) {
      return works.get(ids[0])?.title || this.strings.untitled;
    }
    return worksHereLabel(this.strings, this.lang, ids.length);
  }

  private activate(cluster: ProximityCluster, members: WorkPoint[]): void {
    if (members.length <= 1) {
      this.callbacks.selectWork(cluster.ids[0] ?? null);
      return;
    }
    const ceiling = this.callbacks.maxZoom();
    if (!staysMergedAtMaxZoom(members, ceiling, CLUSTER_DISTANCE)) {
      // Case 1: can separate by zooming — let re-clustering pull them apart.
      this.callbacks.fitPoints(members.map((member) => ({ x: member.x, y: member.y })));
      return;
    }
    // Case 2/3: coincident in practice. Always scope the row; ring only
    // when the count stays legible (RING_MAX), same split the design made.
    const key = clusterKey(cluster.ids);
    this.callbacks.scopeRow(new Set(cluster.ids));
    this.callbacks.announce(this.strings.ringOpened);
    if (cluster.ids.length <= RING_MAX) {
      this.ring = { key, members };
      // Smallest zoom, starting from the current one (zooming out never
      // helps separation), at which this group stands apart from every
      // OTHER point on the map — never just a function of the group's own
      // members, which is what `staysMergedAtMaxZoom` above already ruled
      // out on its own.
      const zoom = ringZoom(members, this.lastPoints, this.lastCamera.zoom, ceiling, CLUSTER_DISTANCE, RING_ZOOM_STEP);
      this.callbacks.focusPoint(worldCentroid(members), zoom);
    } else {
      this.ring = null;
    }
  }
}

function clusterKey(ids: string[]): string {
  return [...ids].sort().join("|");
}

/** A cluster's own world-space centroid, from its members' WORLD (unscaled) coordinates — never `ProximityCluster.screen`, which is `clusters.ts`'s zoom-scaled, pan-independent position, meaningful only for distance comparisons, not for placing anything on the actual page. Exported for `labels.ts`'s own marker-box reservation, which needs the SAME on-screen cluster position this module draws a marker at, not a second computation of it. */
export function worldCentroid(members: WorkPoint[]): Point {
  if (members.length === 0) return { x: 0, y: 0 };
  const sum = members.reduce((acc, member) => ({ x: acc.x + member.x, y: acc.y + member.y }), { x: 0, y: 0 });
  return { x: sum.x / members.length, y: sum.y / members.length };
}
