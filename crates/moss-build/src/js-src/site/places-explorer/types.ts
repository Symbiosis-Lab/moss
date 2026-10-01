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

/** One resolved place. `parent`, when present, is another place's `id` — one hop only, never a full ancestor chain. */
export interface Place {
  id: string;
  name: string;
  parent?: string;
  precision: Precision;
  lat: number;
  lng: number;
}

/** A work's companion page: folded into the work's own card, never its own dot. */
export interface Companion {
  title: string;
  url: string;
}

/** One work (or standalone located page) — one dot on the map. */
export interface Work {
  id: string;
  title: string;
  url: string;
  date?: string;
  byline: string[];
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
