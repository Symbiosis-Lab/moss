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
import type { Camera, Place, Point, Precision, Scope, Viewport, Work } from "./types";
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
    if (!place) continue;
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

export class MarkerLayer {
  private readonly container: HTMLElement;
  private readonly callbacks: MarkerCallbacks;
  private strings: PlacesStrings;
  private lang: string;
  private ring: RingState | null = null;
  private lastPoints: WorkPoint[] = [];
  private lastCamera: Camera = { x: 0, y: 0, zoom: 1 };

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

    this.container.replaceChildren();
    const byId = new Map(points.map((point) => [point.id, point]));

    for (const cluster of clusters) {
      if (this.ring && clusterKey(cluster.ids) === this.ring.key) {
        this.renderRing(cluster, byId, camera, viewport, selectedId, works);
        continue;
      }
      this.renderMarker(cluster, byId, camera, viewport, selectedId, works);
    }
  }

  private renderMarker(
    cluster: ProximityCluster,
    byId: Map<string, WorkPoint>,
    camera: Camera,
    viewport: Viewport,
    selectedId: string | null,
    works: Map<string, Work>,
  ): void {
    const members = cluster.ids.map((id) => byId.get(id)).filter((point): point is WorkPoint => point != null);
    const screen = worldToScreen(worldCentroid(members), camera, viewport);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-places-marker";
    button.style.left = `${screen.x}px`;
    button.style.top = `${screen.y}px`;
    button.dataset.precision = coarsestPrecision(members.map((member) => member.precision));
    if (cluster.ids.length > 1) button.dataset.count = String(cluster.ids.length);
    if (cluster.ids.some((id) => id === selectedId)) button.dataset.selected = "true";
    if (this.ring) button.dataset.dimmed = "";
    button.setAttribute("aria-label", this.labelFor(cluster.ids, works));
    button.addEventListener("click", () => this.activate(cluster, members));
    this.container.append(button);
  }

  private renderRing(
    cluster: ProximityCluster,
    byId: Map<string, WorkPoint>,
    camera: Camera,
    viewport: Viewport,
    selectedId: string | null,
    works: Map<string, Work>,
  ): void {
    const members = cluster.ids.map((id) => byId.get(id)).filter((point): point is WorkPoint => point != null);
    const anchorScreen = worldToScreen(worldCentroid(members), camera, viewport);
    const anchor = document.createElement("button");
    anchor.type = "button";
    anchor.className = "moss-places-marker";
    anchor.dataset.ringAnchor = "";
    anchor.style.left = `${anchorScreen.x}px`;
    anchor.style.top = `${anchorScreen.y}px`;
    anchor.setAttribute("aria-label", this.labelFor(cluster.ids, works));
    anchor.addEventListener("click", () => this.closeRing());
    this.container.append(anchor);

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

      const work = works.get(id);
      const dot = document.createElement("button");
      dot.type = "button";
      dot.className = "moss-places-ring-dot";
      dot.style.left = `${position.x}px`;
      dot.style.top = `${position.y}px`;
      if (id === selectedId) dot.dataset.selected = "true";
      dot.setAttribute("aria-label", work ? (work.title || this.strings.untitled) : id);
      dot.addEventListener("click", () => this.callbacks.selectWork(id));
      this.container.append(dot);
    });
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

/** A cluster's own world-space centroid, from its members' WORLD (unscaled) coordinates — never `ProximityCluster.screen`, which is `clusters.ts`'s zoom-scaled, pan-independent position, meaningful only for distance comparisons, not for placing anything on the actual page. */
function worldCentroid(members: WorkPoint[]): Point {
  if (members.length === 0) return { x: 0, y: 0 };
  const sum = members.reduce((acc, member) => ({ x: acc.x + member.x, y: acc.y + member.y }), { x: 0, y: 0 });
  return { x: sum.x / members.length, y: sum.y / members.length };
}
