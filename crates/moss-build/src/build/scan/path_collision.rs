//! Source files that differ only by letter case, or by how an accented letter
//! is encoded (`é` as one character or as `e` plus a combining accent).
//!
//! `Photo.JPG` beside `photo.jpg`, or the two spellings of `café.png`, are two
//! files on a Linux disk and one on the default macOS volume, where saving the
//! second silently replaces the first. A site synced through a Linux machine
//! can carry both, and then a Mac and a server would publish different sites
//! from the same folder. So the build refuses the folder on every machine.
//! Detecting the pair is a comparison of the paths themselves, so it does not
//! depend on the disk the scan happens to run on.
//!
//! Two pictures or videos the build would publish under one name are refused
//! the same way: `photo.jpg` and `photo.png` both become `photo.webp`, and
//! which one a reader gets would depend on which encode finished last.
//!
//! Two markdown pages are left to the passes that already name them: a clash
//! of page addresses is reported by `slug::resolve_duplicate_slugs_with_lang`
//! and a clash for the home page by the home-slot election. Reporting them
//! here too would print one problem twice.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

use moss_core::asset_paths::published_paths;

use crate::types::content::ProjectStructure;

/// Two names in composed (NFC) order, falling back to their bytes when they
/// compose alike. Every walk and list whose order reaches the output sorts by
/// this, so a folder reads in one order on every machine whatever its disk
/// lists first and however its sync client spelled the names.
pub(crate) fn nfc_order(a: &str, b: &str) -> Ordering {
    a.nfc().cmp(b.nfc()).then_with(|| a.cmp(b))
}

/// [`nfc_order`] for a directory walk: `WalkDir::new(dir).sort_by(nfc_name_order)`.
pub(crate) fn nfc_name_order(a: &walkdir::DirEntry, b: &walkdir::DirEntry) -> Ordering {
    nfc_order(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy())
}

/// The logical path of `rel`, a path under the site folder: `/`-separated
/// (Windows lists `\\`) and composed (NFC), so every machine names a file
/// alike however its disk spells an accented name. The inverse of [`on_disk`].
pub(crate) fn logical_path(rel: impl AsRef<Path>) -> String {
    moss_core::slug::normalize_separators(&rel.as_ref().to_string_lossy()).nfc().collect()
}

/// Where the file a composed `logical` path names sits on this disk. A Mac
/// finds a file under either spelling of its name; Linux matches names byte
/// for byte, so a name a sync client wrote decomposed is found by composing
/// the names in each folder. Two folders whose names compose alike are both
/// searched and the first match wins: a folder holding both spellings of one
/// name resolves to whichever its listing returns first (a site holding such a
/// pair does not build). A path nothing matches comes back as
/// `root.join(logical)`, so the read fails as it would have.
pub(crate) fn on_disk(root: &Path, logical: &str) -> PathBuf {
    let direct = root.join(logical);
    if logical.is_ascii() || direct.symlink_metadata().is_ok() {
        return direct;
    }
    let mut found = vec![root.to_path_buf()];
    for segment in logical.split('/').filter(|s| !s.is_empty()) {
        let mut next = Vec::new();
        for dir in &found {
            let names: Vec<OsString> = std::fs::read_dir(dir)
                .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                .unwrap_or_default();
            next.extend(spellings_of(segment, &names).map(|name| dir.join(name)));
        }
        found = next;
    }
    found.into_iter().next().unwrap_or(direct)
}

/// The names among `names` that compose to the name `segment` composes to.
fn spellings_of<'a>(segment: &'a str, names: &'a [OsString]) -> impl Iterator<Item = &'a OsString> + 'a {
    names.iter().filter(move |name| name.to_str().is_some_and(|name| name.nfc().eq(segment.nfc())))
}

fn is_markdown(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// A path composed and lowercased: two paths with one fold are one file on a
/// Mac or Windows disk.
fn fold(path: &str) -> String {
    path.nfc().collect::<String>().to_lowercase()
}

/// Every pair of files the site would publish under one name, folded as
/// above, as `(first, second, name)` in input order. Every file claims its
/// own path, and a picture or video (`media`) also the names
/// [`published_paths`] gives it. Two markdown pages are left to the passes
/// that already name them.
fn colliding_pairs<'a>(files: impl IntoIterator<Item = (&'a str, bool)>) -> Vec<(String, String, String)> {
    let mut owner: HashMap<String, &str> = HashMap::new();
    let mut pairs: Vec<(String, String, String)> = Vec::new();
    for (path, media) in files {
        let published = if media { published_paths(path) } else { Vec::new() };
        let names = std::iter::once(path.to_string()).chain(published);
        let start = pairs.len();
        for name in names {
            let first = *owner.entry(fold(&name)).or_insert(path);
            let both_pages = is_markdown(first) && is_markdown(path);
            if first != path && !both_pages && !pairs[start..].iter().any(|(a, _, _)| a == first) {
                pairs.push((first.to_string(), path.to_string(), name));
            }
        }
    }
    pairs
}

/// What to rename for one colliding pair. When the two differ by how an
/// accent is stored, the first folder or file name where they part decides:
/// rename the decomposed spelling, since that keeps the name a reader sees.
/// A pair that differs by case has no right spelling, so either will do.
fn rename_advice(a: &str, b: &str) -> String {
    let (parts_a, parts_b): (Vec<&str>, Vec<&str>) = (a.split('/').collect(), b.split('/').collect());
    let Some(at) = parts_a.iter().zip(&parts_b).position(|(x, y)| x != y) else {
        return format!("rename `{a}` or `{b}`");
    };
    let named = |parts: &[&str]| parts[..=at].join("/");
    let what = if at + 1 == parts_a.len() { "" } else { "the folder " };
    let decomposed = |part: &str| !unicode_normalization::is_nfc(part);
    match (decomposed(parts_a[at]), decomposed(parts_b[at])) {
        (true, false) => format!("rename {what}`{}`, whose accent is stored as two characters, not `{}`", named(&parts_a), named(&parts_b)),
        (false, true) => format!("rename {what}`{}`, whose accent is stored as two characters, not `{}`", named(&parts_b), named(&parts_a)),
        _ => format!("rename `{a}` or `{b}`"),
    }
}

/// The refusal for a folder holding a colliding pair among every file the
/// scan found, saying which file of each pair to rename.
pub(crate) fn refuse_colliding_files(found: &ProjectStructure) -> Result<(), String> {
    let files = [&found.markdown_files, &found.html_files, &found.notebook_files, &found.other_files];
    let others = files.into_iter().flatten().map(|f| (f.path.as_str(), false));
    let media = found.image_files.iter().chain(&found.video_files).map(|f| (f.path.as_str(), true));
    let (same_name, shared): (Vec<_>, Vec<_>) =
        colliding_pairs(others.chain(media)).into_iter().partition(|(a, b, _)| fold(a) == fold(b));
    let mut problems = Vec::new();
    if !same_name.is_empty() {
        let fixes: Vec<String> = same_name.iter().map(|(a, b, _)| rename_advice(a, b)).collect();
        problems.push(format!(
            "Some files differ only by letter case or by how an accented letter is encoded, so a Mac or Windows disk keeps only one of each pair and the site would build differently there. To build, {}.",
            fixes.join("; ")
        ));
    }
    if !shared.is_empty() {
        let fixes: Vec<String> =
            shared.iter().map(|(a, b, name)| format!("rename `{a}` or `{b}`, which are both published as `{name}`")).collect();
        problems.push(format!(
            "Some pictures or videos would be published under one name, so which one a reader gets would depend on which the build finished writing last. To build, {}.",
            fixes.join("; ")
        ));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join(" ")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs among files that are not pictures or videos, as `(first, second)`.
    fn case_colliding_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
        colliding_pairs(paths.into_iter().map(|p| (p, false))).into_iter().map(|(a, b, _)| (a, b)).collect()
    }

    fn shared(sources: &[&str]) -> Vec<(String, String, String)> {
        colliding_pairs(sources.iter().map(|p| (*p, true)))
    }

    /// Composed order first, and two spellings of one name never tie, so a
    /// sort never falls back to the order the disk listed them in.
    #[test]
    fn names_sort_composed_and_two_spellings_never_tie() {
        assert_eq!(nfc_order("f", "e\u{301}clair"), Ordering::Less);
        assert_eq!(nfc_order("e\u{301}", "\u{e9}"), Ordering::Less);
        assert_eq!(nfc_order("\u{e9}", "e\u{301}"), Ordering::Greater);
    }

    /// Either spelling of a name finds the other; nothing else does.
    #[test]
    fn a_name_matches_its_other_spelling_only() {
        let names: Vec<OsString> = ["cafe\u{301}", "caf\u{e9}s", "Caf\u{e9}", "cafe"].map(OsString::from).to_vec();
        let found: Vec<&OsString> = spellings_of("caf\u{e9}", &names).collect();
        assert_eq!(found, vec![&names[0]]);
    }

    /// A file stored under decomposed folder and file names is found by its
    /// composed path, and a missing one comes back as the plain join. On a
    /// Mac the plain join already finds it; the folder search is what Linux
    /// needs.
    #[test]
    fn a_decomposed_file_is_found_by_its_composed_path() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("photos/cafe\u{301}")).unwrap();
        std::fs::write(root.path().join("photos/cafe\u{301}/cre\u{300}me.png"), b"x").unwrap();
        let file = on_disk(root.path(), "photos/caf\u{e9}/cr\u{e8}me.png");
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
        assert_eq!(on_disk(root.path(), "photos/caf\u{e9}/gone.png"), root.path().join("photos/caf\u{e9}/gone.png"));
    }

    #[test]
    fn names_both_files_when_only_the_case_differs() {
        let pairs = case_colliding_paths(["photo.jpg", "about.txt", "photo.JPG"]);
        assert_eq!(pairs, vec![("photo.jpg".to_string(), "photo.JPG".to_string())]);
    }

    #[test]
    fn two_encodings_of_one_accented_name_collide() {
        let (composed, decomposed) = ("images/caf\u{e9}.png", "images/cafe\u{301}.png");
        assert_eq!(case_colliding_paths([composed, "about.txt", decomposed]).len(), 1);
        assert_eq!(case_colliding_paths(["images/Cafe\u{301}.PNG", composed]).len(), 1);
    }

    /// A colliding pair refuses the folder, naming both files; any other
    /// folder passes.
    #[test]
    fn a_folder_with_a_colliding_pair_is_refused() {
        let image = |path: &str| crate::types::content::MediaMetadata { path: path.into(), ..Default::default() };
        let mut found = ProjectStructure { image_files: vec![image("images/caf\u{e9}.png"), image("images/plain.png")], ..Default::default() };
        assert!(refuse_colliding_files(&found).is_ok());
        found.image_files.push(image("images/Cafe\u{301}.png"));
        let refusal = refuse_colliding_files(&found).unwrap_err();
        assert!(refusal.contains("rename `images/Cafe\u{301}.png`, whose accent is stored as two characters, not `images/caf\u{e9}.png`"), "{refusal}");
    }

    /// The advice names the decomposed spelling when there is one, and
    /// leaves the choice open for a pair that differs only by case.
    #[test]
    fn the_refusal_says_which_file_to_rename() {
        let advice = "rename `cafe\u{301}.png`, whose accent is stored as two characters, not `caf\u{e9}.png`";
        assert_eq!(rename_advice("cafe\u{301}.png", "caf\u{e9}.png"), advice);
        assert_eq!(rename_advice("caf\u{e9}.png", "cafe\u{301}.png"), advice);
        assert_eq!(rename_advice("Photo.png", "photo.png"), "rename `Photo.png` or `photo.png`");
    }

    /// The name where the two spellings part is the one to rename: a file in
    /// a folder both share, or the folder itself.
    #[test]
    fn the_advice_names_the_folder_or_file_where_the_spellings_part() {
        assert_eq!(
            rename_advice("Re\u{301}seau/\u{e9}.png", "Re\u{301}seau/e\u{301}.png"),
            "rename `Re\u{301}seau/e\u{301}.png`, whose accent is stored as two characters, not `Re\u{301}seau/\u{e9}.png`"
        );
        assert_eq!(
            rename_advice("Cafe\u{301}/a.png", "Caf\u{e9}/a.png"),
            "rename the folder `Cafe\u{301}`, whose accent is stored as two characters, not `Caf\u{e9}`"
        );
    }

    #[test]
    fn a_static_asset_pair_collides() {
        assert_eq!(case_colliding_paths(["data/Foo.csv", "data/foo.csv"]).len(), 1);
    }

    #[test]
    fn compares_whole_paths_so_a_folder_name_counts() {
        assert_eq!(case_colliding_paths(["Docs/a.png", "docs/A.png"]).len(), 1);
    }

    #[test]
    fn two_markdown_pages_are_left_to_the_url_and_home_passes() {
        assert!(case_colliding_paths(["index.md", "Index.md"]).is_empty());
    }

    #[test]
    fn distinct_names_and_a_repeated_identical_path_are_not_collisions() {
        assert!(case_colliding_paths(["a.png", "b.png", "a.png"]).is_empty());
    }

    #[test]
    fn two_pictures_with_one_webp_or_two_videos_with_one_ladder_collide() {
        let pair = |a: &str, b: &str, name: &str| vec![(a.to_string(), b.to_string(), name.to_string())];
        assert_eq!(shared(&["photo.jpg", "photo.png"]), pair("photo.jpg", "photo.png", "photo.webp"));
        assert_eq!(shared(&["photo.png", "photo.webp"]), pair("photo.png", "photo.webp", "photo.webp"));
        assert_eq!(shared(&["Photo.JPEG", "photo.png"]), pair("Photo.JPEG", "photo.png", "photo.webp"));
        assert_eq!(shared(&["clip.mov", "clip.mp4"]), pair("clip.mov", "clip.mp4", "clip.mp4"));
        assert_eq!(shared(&["clip.webm", "clip.mkv"]), pair("clip.webm", "clip.mkv", "clip.thumb.jpg"));
        assert_eq!(shared(&["clip.mov", "clip.thumb.jpg"]), pair("clip.mov", "clip.thumb.jpg", "clip.thumb.jpg"));
    }

    #[test]
    fn media_published_under_different_names_do_not_collide() {
        assert!(shared(&["photo.jpg", "photo.mov", "logo.gif", "logo.png", "a/photo.png"]).is_empty());
        assert_eq!(shared(&["photo.jpg", "Photo.jpg"]).len(), 1, "a case pair is one pair, however many names it shares");
        assert_eq!(shared(&["clip.Mov", "clip.mov"]).len(), 1, "a video's own name counts, not only what it is published as");
    }

    /// A case pair and two sources for one name are named in one refusal, so
    /// one rename pass fixes the folder. The scan test cannot make the case
    /// pair on a Mac disk.
    #[test]
    fn both_kinds_of_pair_are_named_in_one_refusal() {
        let image = |path: &str| crate::types::content::MediaMetadata { path: path.into(), ..Default::default() };
        let names = ["photo.jpg", "photo.png", "logo.gif", "Logo.gif"];
        let found = ProjectStructure { image_files: names.map(image).to_vec(), ..Default::default() };
        let refusal = refuse_colliding_files(&found).unwrap_err();
        assert!(refusal.contains("rename `logo.gif` or `Logo.gif`") && refusal.contains("which are both published as `photo.webp`"), "{refusal}");
    }
}
