/**
 * types.ts — the shared shapes every places-explorer geometry module passes
 * around.
 *
 * Field names here match `places.<hash>.json` (the build's own
 * `crates/moss-build/src/build/place_map/places_data.rs`) exactly, so a
 * module in this directory can consume that file's parsed JSON with no
 * remapping step.
 */

/** A gazetteer entry's privacy tier — the one `places.toml` precision this project's Rust side serializes as a lowercase string. */
export type Precision = "exact" | "city" | "region" | "country";

/**
 * One resolved place. `parent`, when present, is another place's `id` —
 * one hop only, never a full ancestor chain.
 *
 * `precision`/`lat`/`lng` are absent on a GROUPING node:
 * `places_data.rs`'s `fold_ancestors` emits one for an ancestor it had to
 * invent (no gazetteer row at all, or a row with no coordinates) purely so
 * the chip menu can still dig through it — reachable only by `parent`
 * reference, never a point on the map. No reader in this directory ever
 * needs to tell the two no-coordinate shapes apart, only whether a point
 * exists at all (`lat`/`lng` present) before projecting one: `pointsForWorks`
 * and `selectWork` (markers.ts / map.ts) only ever read a WORK's own first
 * place, which `places_data.rs` only ever resolves to one with real
 * coordinates — a grouping node is never a work's own place — so neither
 * needs a guard of its own; `scope.ts`/`chip.ts` never read these three
 * fields at all, working purely off `id`/`name`/`parent`.
 */
export interface Place {
  id: string;
  name: string;
  parent?: string;
  precision?: Precision;
  lat?: number;
  lng?: number;
}

/** Narrows `place` to one with real coordinates (and therefore a real `precision` — `places_data.rs` never sets one without the other) to project — the ordinary case for anything a WORK resolves to; false only for a grouping node (see {@link Place}'s own doc), which every caller that reaches one only ever does through its `id`/`name`/`parent`, never a point. */
export function hasPoint(place: Place): place is Place & { lat: number; lng: number; precision: Precision } {
  return place.lat !== undefined && place.lng !== undefined;
}

/** A work's companion page: folded into the work's own card, never its own dot. */
export interface Companion {
  title: string;
  url: string;
}

/** One work (or standalone located page) — one dot on the map. Every reader in this directory can assume `byline` is an array, never `undefined`: {@link normalizePlacesData} is the one place that fills it in, so a reader never has to guard its own read of it. */
export interface Work {
  id: string;
  title: string;
  url: string;
  date?: string;
  byline: string[];
  /** The page's `author:` names, `[]` when it has none (see {@link normalizePlacesData}). */
  authors: string[];
  description?: string;
  cover?: string;
  /** Ids into the sibling `places` array. */
  places: string[];
  companions: Companion[];
}

/** `places.<hash>.json` itself: the build's `PlacesData` struct, serialized whole. */
export interface PlacesData {
  works: Work[];
  places: Place[];
}

/**
 * `places.<hash>.json` exactly as `places_data.rs`'s `WorkEntry` serializes
 * it over the wire, not as every reader in this directory wants to consume
 * it: `#[serde(skip_serializing_if = "Vec::is_empty")]` on `byline` means an
 * unauthored work's JSON omits the key outright, the same way
 * `Option::is_none` already does for `date`/`description`/`cover` — `byline`
 * was the one field this type declared required when the wire can truthfully
 * omit it, which crashed `cards.ts`'s first read of `work.byline.length` on
 * any such work before the whole explorer ever reached its ready state.
 * {@link normalizePlacesData} is the one place that reconciles the two
 * shapes; every other module in this directory should import {@link Work},
 * never this.
 */
export interface WorkWire {
  id: string;
  title: string;
  url: string;
  date?: string;
  byline?: string[];
  authors?: string[];
  description?: string;
  cover?: string;
  places: string[];
  companions: Companion[];
}

/** The wire shape of {@link PlacesData} — see {@link WorkWire}. */
export interface PlacesDataWire {
  works: WorkWire[];
  places: Place[];
}

/** Reconcile a fetched `places.<hash>.json` payload into the shape every reader in this directory actually relies on — defaulting an omitted `byline` and `authors` to `[]`. The one normalization boundary: call this once, right where the JSON is parsed ({@link import("./index").initPlacesExplorer}), not at each read site. */
export function normalizePlacesData(wire: PlacesDataWire): PlacesData {
  return {
    works: wire.works.map((work) => ({ ...work, byline: work.byline ?? [], authors: work.authors ?? [] })),
    places: wire.places,
  };
}

/** A plain 2D point, in whatever coordinate space the caller documents. */
export interface Point {
  x: number;
  y: number;
}

/** The on-screen box the map is drawn into, in CSS px. */
export interface Viewport {
  width: number;
  height: number;
}

/** An axis-aligned screen-space box, in CSS px from the viewport's own top-left — the shape every collision check in this directory (the label layer's own placement, a reserved UI region) compares. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The map's camera: `x`/`y` are the world-space (projected) point centred in the viewport; `zoom` is a multiplier over the cover baseline (1 = cover, never below it). */
export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

/** The current reading scope: everywhere, one work, or one place (and, per `inScope`, its direct children). */
export type Scope =
  | { kind: "all" }
  | { kind: "article"; id: string }
  | { kind: "place"; id: string };

/** A labeled point (city, mountain range or peak) in `labels.json` — see `emit::place_map_labels::PointLabelJson`. `rank` is Natural Earth's own `scalerank` (0 = most important), lower is more major. */
export interface LabelPoint {
  name: string;
  lat: number;
  lng: number;
  rank: number;
}

/** A labeled river course — see `emit::place_map_labels::RiverLabelJson`. `line` is `[lng, lat]` pairs (lng first) in the river's natural, open (not closed) order. */
export interface LabelRiver {
  name: string;
  rank: number;
  line: Array<[number, number]>;
}

/** One language's worth of `labels.json`. */
export interface LanguageLabels {
  cities: LabelPoint[];
  ranges: LabelPoint[];
  peaks: LabelPoint[];
  rivers: LabelRiver[];
}

/** `labels.<hash>.json` itself: `languages` names every other top-level key present — see `emit::place_map_labels::LabelsJson`. */
export interface LabelsData {
  languages: string[];
  [language: string]: LanguageLabels | string[];
}
