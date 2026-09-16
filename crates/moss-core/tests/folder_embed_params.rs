//! Golden-vector test for the folder-embed pothole grammar.
//!
//! `tests/fixtures/folder-embed-params.vectors.json` is the cross-language
//! contract for `![[/folder/|style:grid,sort:weight]]`. This test holds the
//! Rust side of it; the editor's read-side twin `parseFolderParams`
//! (`frontend/app/editor/cm-image-extract.ts`) reads the same file from
//! `cm-image-extract.test.ts`, so the chips the editor draws cannot claim a
//! param the build would drop, or drop one the build would keep.
//!
//! Every expectation in the fixture was derived by RUNNING `parse_params`.
//! Add a vector before you add a key: a param added on the Rust side alone
//! produces a silently missing chip, not a red test, unless it is vectored.
//!
//! The fixture carries only the five params the LISTING branch acts on.
//! `size` is deliberately absent — the listing branch ignores it — and its
//! own behaviour stays pinned by the `folder_list_tests.rs` size tests.

use moss_core::resolve::embed_renderer::folder_list::parse_params;
use moss_core::sort::SortAxis;
use serde::Deserialize;

#[derive(Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    name: String,
    input: String,
    expect: Expect,
    note: String,
}

#[derive(Deserialize, PartialEq, Debug)]
struct Expect {
    style: Option<String>,
    sort: Option<String>,
    depth: Option<String>,
    group: Option<String>,
    limit: Option<u64>,
}

fn axis_name(a: SortAxis) -> String {
    match a {
        SortAxis::Date => "date",
        SortAxis::Weight => "weight",
        SortAxis::Title => "title",
    }
    .to_string()
}

#[test]
fn parse_params_matches_the_shared_vectors() {
    let raw = include_str!("fixtures/folder-embed-params.vectors.json");
    let Vectors { vectors } = serde_json::from_str(raw).unwrap_or_else(|e| {
        panic!("folder-embed-params.vectors.json is not valid JSON: {e}");
    });
    assert!(!vectors.is_empty(), "fixture must carry vectors");

    for v in &vectors {
        let p = parse_params(&v.input);
        let got = Expect {
            style: p.style.clone(),
            sort: p.sort.map(axis_name),
            depth: p.depth.clone(),
            group: p.group.clone(),
            limit: p.limit.map(|n| n as u64),
        };
        assert_eq!(
            got, v.expect,
            "vector `{}` ({:?}) — {}",
            v.name, v.input, v.note
        );
    }
}
