use super::{Frame, PattersonProjection, TILE_BLEED, TILE_K};

/// A tile's canvas on the world's lattice of `1 / TILE_K` world units, in
/// whole canvas units. A vertex is rounded to an integer in its own
/// canvas, so two tiles agree on where a shared coastline falls only if
/// both canvases start on the same lattice: the origin is the padded
/// cell's top-left corner snapped down to a whole unit, and the size is
/// grown to a whole number of units so the canvas still covers the
/// padded cell's far corner.
pub(crate) struct TileGrid {
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

impl TileGrid {
    /// `frame`'s cell edges come from the full-extent projection (the
    /// same values `tiles.ts`'s `tileCellBounds` computes), padded by
    /// `TILE_BLEED` on every edge, then snapped.
    pub(crate) fn of(frame: &Frame) -> Self {
        let full = PattersonProjection::full_extent();
        let west = frame.center_longitude - frame.longitude_span / 2.0;
        let east = frame.center_longitude + frame.longitude_span / 2.0;
        let north = frame.center_latitude + frame.latitude_span / 2.0;
        let south = frame.center_latitude - frame.latitude_span / 2.0;
        let (min_x, _) = full.project_unwrapped(west, 0.0);
        let (max_x, _) = full.project_unwrapped(east, 0.0);
        // Higher latitude (north) projects to a SMALLER y (north is up), so
        // the cell's own north edge gives min_y and south gives max_y —
        // mirrors `map.ts`'s `tileCellBounds` exactly.
        let (_, min_y) = full.project_unwrapped(0.0, north);
        let (_, max_y) = full.project_unwrapped(0.0, south);
        let left = ((min_x - TILE_BLEED) * TILE_K).floor();
        let top = ((min_y - TILE_BLEED) * TILE_K).floor();
        let right = ((max_x + TILE_BLEED) * TILE_K).ceil();
        let bottom = ((max_y + TILE_BLEED) * TILE_K).ceil();
        Self { left, top, width: right - left, height: bottom - top }
    }

    /// Where the canvas starts on the world's lattice of `1 / TILE_K`
    /// units, as whole canvas units from the world's own origin. Written
    /// to `tiles.json` so the runtime places the tile exactly there.
    pub(crate) fn origin_units(&self) -> (i64, i64) {
        (self.left as i64, self.top as i64)
    }
}
