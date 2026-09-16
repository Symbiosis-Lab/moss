use super::*;

// ── the rule, pinned across languages ───────────────────────────────────────
//
// Where an image lands is decided twice: here (the real placement, read at
// store time) and in TypeScript (`frontend/app/settings/attachment-placement.ts`,
// which draws the live "posts/on-gardens.md → posts/assets/photo.png" line the
// user reads while typing the folder name). A preview that disagreed with the
// placement would be worse than no preview, so both sides are checked against
// ONE file — tests/fixtures/attachment-placement.vectors.json — read directly,
// with no generated copy in between. Add a case there and both languages get it.
//
// The `exclusion` array is Rust-only (the settings preview shows placement, not
// exclusion) but lives in the same file so the two questions the encoding
// answers cannot drift apart.

#[derive(serde::Deserialize)]
struct Vectors {
    placement: Vec<PlacementVector>,
    validation: Vec<ValidationVector>,
    exclusion: Vec<ExclusionVector>,
}

#[derive(serde::Deserialize)]
struct PlacementVector {
    raw: String,
    page: String,
    dir: String,
}

#[derive(serde::Deserialize)]
struct ValidationVector {
    raw: String,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct ExclusionVector {
    raw: String,
    dir: String,
    excluded: bool,
}

fn vectors() -> Vectors {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/attachment-placement.vectors.json"
    ))
    .expect("attachment-placement.vectors.json parses")
}

#[test]
fn placement_matches_the_shared_vectors() {
    for v in vectors().placement {
        assert_eq!(
            attachment_dir_for_page(&v.raw, &v.page),
            v.dir,
            "attachment_folder {:?} with page {:?}",
            v.raw,
            v.page
        );
    }
}

#[test]
fn validation_matches_the_shared_vectors() {
    for v in vectors().validation {
        // Map the message back to the vector's kind: the two rejections are
        // distinct sentences, so a swapped one fails here instead of shipping
        // a reason that names the wrong problem.
        let got = match validate_attachment_folder(&v.raw) {
            Ok(()) => None,
            Err(msg) if msg.contains("'..'") => Some("dotdot"),
            Err(_) => Some("absolute"),
        };
        assert_eq!(got, v.error.as_deref(), "attachment_folder {:?}", v.raw);
    }
}

#[test]
fn exclusion_matches_the_shared_vectors() {
    for v in vectors().exclusion {
        assert_eq!(
            is_attachment_dir(&v.raw, &v.dir),
            v.excluded,
            "attachment_folder {:?} with dir {:?}",
            v.raw,
            v.dir
        );
    }
}

// ── the two questions agree ─────────────────────────────────────────────────

#[test]
fn the_directory_a_page_writes_to_is_always_storage() {
    // The pair is only coherent if every folder `attachment_dir_for_page`
    // would create is one `is_attachment_dir` then recognizes. Otherwise moss
    // would drop an image into a folder it goes on to publish as a section.
    for raw in ["assets", "./assets", "media/img", "./a/b", "assets/", "  assets  "] {
        for page in ["index.md", "posts/p.md", "a/b/c/deep.md"] {
            let dir = attachment_dir_for_page(raw, page);
            assert!(
                is_attachment_dir(raw, &dir),
                "attachment_folder {raw:?} stores {page:?}'s images in {dir:?}, \
                 but that directory is not recognized as storage"
            );
        }
    }
}

#[test]
fn the_default_never_excludes_anything() {
    // The no-op case, stated as a property rather than a handful of vectors:
    // with attachments beside the page there is no asset folder, so no vault
    // loses a listing page by upgrading into this change.
    for raw in ["", ".", "./", "  "] {
        for dir in ["assets", "posts", "posts/assets", "images/2024", "static"] {
            assert!(!is_attachment_dir(raw, dir), "raw {raw:?} excluded {dir:?}");
        }
    }
}
