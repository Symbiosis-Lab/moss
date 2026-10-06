use super::*;
use std::collections::BTreeSet;

thread_local! {
    pub(super) static FEATURE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn scan_reference<'a>(selection: &TileSelection, pack: &'a Pack) -> Vec<(u8, &'a Feature)> {
    let ids: BTreeSet<u32> = pack.tiles.iter()
        .filter(|tile| selection.tiles.contains(&(tile.x, tile.y)))
        .flat_map(|tile| tile.features.iter().copied())
        .collect();
    let mut offset = 0u32;
    let mut expected = Vec::new();
    for layer in &pack.tiers[1].layers {
        for (index, feature) in layer.features.iter().enumerate() {
            if ids.contains(&(offset + index as u32)) {
                expected.push((layer.id, feature));
            }
        }
        offset += layer.feature_count;
    }
    expected
}

#[test]
fn selected_feature_order_matches_registry_scan_for_every_tile_and_frame_tier() {
    let pack = super::super::embedded().unwrap();
    let mut selections: Vec<_> = pack.tiles.iter()
        .map(|tile| TileSelection { tiles: vec![(tile.x, tile.y)] })
        .collect();
    for (points, precision) in [
        (vec![(35.5, 33.89)], Precision::City),
        (vec![(12.0, 41.0), (120.0, 30.0)], Precision::Exact),
        (vec![(0.0, 85.1)], Precision::Exact),
        (vec![(179.0, 0.0), (-179.0, 0.0)], Precision::Exact),
    ] {
        let points: Vec<_> = points.into_iter()
            .map(|(x, y)| ProjectedPoint::new(x, y).unwrap())
            .collect();
        let frame = Frame::from_points(
            &points, std::iter::repeat(precision).take(points.len()),
        ).unwrap();
        selections.push(TileSelection::for_frame(&pack, &frame));
    }
    selections.push(TileSelection {
        tiles: pack.tiles.iter().map(|tile| (tile.x, tile.y)).collect(),
    });
    selections.push(TileSelection { tiles: Vec::new() });
    for selection in selections {
        let actual = selection.features(&pack);
        let expected = scan_reference(&selection, &pack);
        assert_eq!(actual.len(), expected.len());
        for ((actual_layer, actual_feature), (expected_layer, expected_feature)) in
            actual.iter().zip(expected)
        {
            assert_eq!(*actual_layer, expected_layer);
            assert!(std::ptr::eq(*actual_feature, expected_feature));
        }
    }
}

#[test]
fn sparse_tile_selection_visits_only_selected_features() {
    let pack = super::super::embedded().unwrap();
    let tile = pack.tiles.iter().filter(|tile| !tile.features.is_empty())
        .min_by_key(|tile| tile.features.len())
        .unwrap();
    let selection = TileSelection { tiles: vec![(tile.x, tile.y)] };
    FEATURE_VISITS.with(|visits| visits.set(0));
    let features = selection.features(&pack);
    let visits = FEATURE_VISITS.with(|visits| visits.get());
    assert_eq!(
        visits, features.len(), "unselected features must not be inspected",
    );
    assert!(features.len() * 100 < pack.tiers[1].feature_count as usize);
}
