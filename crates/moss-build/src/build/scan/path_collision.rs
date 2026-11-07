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
//! Two markdown pages are left to the passes that already name them: a clash
//! of page addresses is reported by `slug::resolve_duplicate_slugs_with_lang`
//! and a clash for the home page by the home-slot election. Reporting them
//! here too would print one problem twice.

use std::cmp::Ordering;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

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

fn is_markdown(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// Every pair of paths equal when composed and compared case-insensitively
/// but not exactly, as `(first, second)` in input order, except pairs where
/// both are markdown. A group of three yields two pairs, each against the
/// first path.
pub(crate) fn case_colliding_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut first_seen: HashMap<String, &str> = HashMap::new();
    let mut pairs = Vec::new();
    for path in paths {
        let key = path.nfc().collect::<String>().to_lowercase();
        match first_seen.get(&key) {
            Some(first) if *first != path && !(is_markdown(first) && is_markdown(path)) => {
                pairs.push((first.to_string(), path.to_string()))
            }
            Some(_) => {}
            None => {
                first_seen.insert(key, path);
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
    let others = found.image_files.iter().chain(&found.video_files).map(|f| f.path.as_str());
    let files = [&found.markdown_files, &found.html_files, &found.notebook_files, &found.other_files];
    let paths = files.into_iter().flatten().map(|f| f.path.as_str()).chain(others);
    let fixes: Vec<String> = case_colliding_paths(paths).iter().map(|(a, b)| rename_advice(a, b)).collect();
    if fixes.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Some files differ only by letter case or by how an accented letter is encoded, so a Mac or Windows disk keeps only one of each pair and the site would build differently there. To build, {}.",
        fixes.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Composed order first, and two spellings of one name never tie, so a
    /// sort never falls back to the order the disk listed them in.
    #[test]
    fn names_sort_composed_and_two_spellings_never_tie() {
        assert_eq!(nfc_order("f", "e\u{301}clair"), Ordering::Less);
        assert_eq!(nfc_order("e\u{301}", "\u{e9}"), Ordering::Less);
        assert_eq!(nfc_order("\u{e9}", "e\u{301}"), Ordering::Greater);
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
}
