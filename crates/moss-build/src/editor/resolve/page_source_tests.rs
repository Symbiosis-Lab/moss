//! Tests for [`super`] — the disk-truth half of page-source resolution.

use super::*;
use std::path::Path;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// A marker prober that reports every named file's bytes as still in the
/// cloud — the shape `home_marker_of` returns for a dataless file, which no
/// test can create for real.
fn evicted(names: &[&str]) -> impl Fn(&Path) -> Option<bool> {
    let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    move |p: &Path| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if owned.iter().any(|o| o == name) {
            None
        } else {
            super::home_marker_of(p)
        }
    }
}

/// moss#1062. A `home: true` home that is neither index-named nor self-named
/// is provable ONLY by reading it. While its bytes are in the cloud that read
/// fails, and folding the failure into `false` turns "I could not check" into
/// "it is not the home" — so the site root resolved to nothing and the editor
/// offered to create a home file over the one that exists.
#[test]
fn an_unreadable_marker_is_unknown_never_a_proven_negative() {
    let dir = tmp();
    let root = dir.path().join("My Site");
    std::fs::create_dir_all(&root).unwrap();
    // Not `index.md`, not `My Site.md` — only the marker makes it the home.
    std::fs::write(root.join("welcome.md"), "---\nhome: true\n---\nhi").unwrap();

    let vault = crate::vault_root::VaultRoot::resolve(&root);
    assert!(
        matches!(detect_home_source_with(vault.path(), vault.name(), true, &home_marker_of), HomeVerdict::File(f) if f == "welcome.md"),
        "sanity: readable, the marker proves the home"
    );

    assert!(
        matches!(
            detect_home_source_with(vault.path(), vault.name(), true, &evicted(&["welcome.md"])),
            HomeVerdict::StillArriving
        ),
        "an unreadable marker must leave the root undecided, never home-less"
    );
}

/// The other half of the rule: an unreadable neighbour must not *override* a
/// home the filesystem already proves. A root with a real `index.md` stays
/// editable while some unrelated top-level note is still downloading.
#[test]
fn a_provable_home_outranks_an_unreadable_neighbour() {
    let dir = tmp();
    let root = dir.path().join("My Site");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.md"), "# Home").unwrap();
    std::fs::write(root.join("notes.md"), "# Notes").unwrap();

    let vault = crate::vault_root::VaultRoot::resolve(&root);
    assert!(
        matches!(
            detect_home_source_with(vault.path(), vault.name(), true, &evicted(&["notes.md"])),
            HomeVerdict::File(f) if f == "index.md"
        ),
        "a proven home wins; only the absence of one is undecidable"
    );
}

/// And a root that genuinely has no intentional home must still say so — the
/// "Create home file" CTA is correct there, and turning every such root into a
/// waiting pane would strand the onboarding flow.
#[test]
fn a_root_with_no_intentional_home_is_still_a_proven_none() {
    let dir = tmp();
    let root = dir.path().join("My Site");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("first-post.md"), "# Post").unwrap();

    let vault = crate::vault_root::VaultRoot::resolve(&root);
    assert!(matches!(
        detect_home_source_with(vault.path(), vault.name(), true, &home_marker_of),
        HomeVerdict::None
    ));
}

/// The three-valued marker read itself: a readable file answers, and the
/// answer for an ordinary local file is never `None`.
#[test]
fn a_readable_file_always_answers_the_marker_question() {
    let dir = tmp();
    let marked = dir.path().join("a.md");
    let plain = dir.path().join("b.md");
    std::fs::write(&marked, "---\nhome: true\n---\n").unwrap();
    std::fs::write(&plain, "no frontmatter").unwrap();

    assert_eq!(home_marker_of(&marked), Some(true));
    assert_eq!(home_marker_of(&plain), Some(false));
}

/// The verdict has to survive the trip to the frontend, or deliverable 4 stops
/// at the Rust boundary: an undecidable root must resolve to a page the editor
/// can recognise as "waiting", not to the same `(source_path: None)` shape a
/// root with genuinely no home produces — those two render different surfaces.
#[test]
fn an_undecidable_root_resolves_to_a_pending_page_not_an_absent_one() {
    let dir = tmp();

    let waiting = root_page_source(HomeVerdict::StillArriving, dir.path())
        .expect("a waiting root must resolve, not fall through to the CTA path");
    assert!(waiting.source_pending, "the editor needs to know why it is empty");
    assert!(waiting.source_path.is_none());
    assert!(waiting.is_dir, "the root is still the root");

    assert!(
        root_page_source(HomeVerdict::None, dir.path()).is_none(),
        "a proven-home-less root falls through to the ordinary resolve path"
    );

    std::fs::write(dir.path().join("index.md"), "# Home").unwrap();
    let found = root_page_source(HomeVerdict::File("index.md".into()), dir.path())
        .expect("a real home resolves");
    assert!(!found.source_pending);
    assert!(found.source_path.unwrap().ends_with("index.md"));
}

/// A folder template seeds `Updates/Updates.md` and the editor opens it before
/// any build registers the folder; the preview's echo for `/updates/` must
/// resolve to it from disk. Subfolders take the FULL election, fallback
/// included — the tree and `compute_home_file_winners` both serve a lone
/// article as its folder's home, and a resolver that dissented showed the
/// create-home CTA over it (2026-09-05). The root keeps its gate: see
/// `a_root_with_no_intentional_home_is_still_a_proven_none` above.
#[test]
fn a_subfolder_home_is_found_on_disk_before_the_build_registers_it() {
    let dir = tmp();
    let sub = dir.path().join("Updates");
    std::fs::create_dir_all(&sub).unwrap();
    let elect = |sub: &Path| match detect_home_source_with(sub, "Updates", false, &home_marker_of) {
        HomeVerdict::File(f) => Some(f),
        HomeVerdict::None => None,
        HomeVerdict::StillArriving => panic!("nothing here is in the cloud"),
    };

    assert_eq!(elect(&sub), None, "an empty folder has no home");

    std::fs::write(sub.join("hello.md"), "---\ntitle: Hi\n---\n").unwrap();
    assert_eq!(elect(&sub).as_deref(), Some("hello.md"), "a lone article IS the folder's home, as built");

    std::fs::write(sub.join("Updates.md"), "---\nnav: true\n---\n").unwrap();
    assert_eq!(elect(&sub).as_deref(), Some("Updates.md"), "self-named outranks the fallback");

    std::fs::write(sub.join("index.md"), "# Index").unwrap();
    assert_eq!(elect(&sub).as_deref(), Some("index.md"), "an index stem outranks the self-named note");

    std::fs::write(sub.join("welcome.md"), "---\nhome: true\n---\n").unwrap();
    assert_eq!(elect(&sub).as_deref(), Some("welcome.md"), "a `home: true` marker outranks every filename rule");
}
