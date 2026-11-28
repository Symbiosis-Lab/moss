//! Source files that differ only by letter case, or by how an accented letter
//! is encoded (`é` as one character or as `e` plus a combining accent).
//!
//! `Photo.JPG` beside `photo.jpg`, or the two spellings of `café.png`, are two
//! files on a Linux disk and one on the default macOS volume, where saving the
//! second silently replaces the first. A site synced through a Linux machine
//! can carry both, and then a Mac and a server would publish different sites
//! from the same folder. So on every machine the build keeps one file of each
//! pair, the same one, and records a problem naming both, which refuses
//! publish. Detecting the pair is a comparison of the paths themselves, so it
//! does not depend on the disk the scan happens to run on.
//!
//! Two pictures or videos the build would publish under one name are handled
//! the same way: `photo.jpg` and `photo.png` both become `photo.webp`, and
//! which one a reader got would depend on which encode finished last.
//!
//! Two markdown pages are left to the passes that already name them: a clash
//! of page addresses is reported by `slug::resolve_duplicate_slugs_with_lang`
//! and a clash for the home page by the home-slot election. Reporting them
//! here too would print one problem twice.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

use moss_core::asset_paths::published_paths;

use crate::types::content::ProjectStructure;

pub(crate) use moss_core::slug::nfc_order;

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

/// Two spellings of one accented name get one [`logical_path`], so the walk
/// must pick one before it measures either. Decided before the walk: the
/// spelling [`on_disk`] reads is kept, so its size, date and picture
/// dimensions are the bytes the build renders.
#[derive(Default)]
pub(crate) struct Spellings {
    left_out: HashSet<PathBuf>,
    /// `(kept, left out)`, as the disk stores each name.
    pairs: Vec<(String, String)>,
}

impl Spellings {
    pub(crate) fn of<'a>(root: &Path, files: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut by_logical: HashMap<String, Vec<&Path>> = HashMap::new();
        for file in files {
            by_logical.entry(logical_path(file.strip_prefix(root).unwrap_or(file))).or_default().push(file);
        }
        let mut found = Self::default();
        let stored = |p: &Path| moss_core::slug::normalize_separators(&p.strip_prefix(root).unwrap_or(p).to_string_lossy());
        let mut groups: Vec<_> = by_logical.into_iter().filter(|(_, files)| files.len() > 1).collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        for (logical, mut files) in groups {
            // The walk's order is the file system's; `files[0]` must not depend on it.
            files.sort();
            let read = on_disk(root, &logical);
            let kept = files.iter().copied().find(|f| *f == read).unwrap_or(files[0]);
            for other in files.into_iter().filter(|f| *f != kept) {
                found.pairs.push((stored(kept), stored(other)));
                found.left_out.insert(other.to_path_buf());
            }
        }
        found
    }

    /// The logical path of `rel`, or `None` for a spelling left out.
    pub(crate) fn logical(&self, abs: &Path, rel: &Path) -> Option<String> {
        (!self.left_out.contains(abs)).then(|| logical_path(rel))
    }
}

/// Where the file a composed `logical` path names sits on this disk. A Mac
/// finds a file under either spelling of its name; Linux matches names byte
/// for byte, so a name a sync client wrote decomposed is found by composing
/// the names in each folder. Two folders whose names compose alike are both
/// searched and the first match wins, the composed spelling before the
/// decomposed one and then by bytes, whatever order the disk lists them in.
/// A path nothing matches comes back as
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
            let mut names: Vec<OsString> = std::fs::read_dir(dir)
                .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                .unwrap_or_default();
            names.sort_by_key(|name| (!name.to_str().is_some_and(unicode_normalization::is_nfc), name.clone()));
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
    moss_core::source_edit::policy::is_page_path(Path::new(path))
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

/// Leaves the second file of each colliding pair, the decomposed spelling or
/// else the later in [`nfc_order`], out of
/// `found`, so the folder builds alike on every machine, and returns the
/// problems naming both files of each pair. A recorded problem refuses
/// publish. The accent pairs `spellings` saw were left out by the walk.
pub(crate) fn leave_out_colliding_files(found: &mut ProjectStructure, spellings: &Spellings) -> Vec<String> {
    let files = [&found.markdown_files, &found.html_files, &found.notebook_files, &found.other_files];
    let others = files.into_iter().flatten().map(|f| (f.path.as_str(), false));
    let media = found.image_files.iter().chain(&found.video_files).map(|f| (f.path.as_str(), true));
    // The composed spelling is kept, as `rename_advice` names the other.
    let order = |a: &str, b: &str| unicode_normalization::is_nfc(b).cmp(&unicode_normalization::is_nfc(a)).then_with(|| nfc_order(a, b));
    let kept_first = |(a, b, name): (String, String, String)| if order(&a, &b).is_gt() { (b, a, name) } else { (a, b, name) };
    let (mut same_name, shared): (Vec<_>, Vec<_>) =
        colliding_pairs(others.chain(media)).into_iter().map(kept_first).partition(|(a, b, _)| fold(a) == fold(b));
    let left_out: HashMap<String, String> = same_name.iter().chain(&shared).map(|(kept, out, _)| (out.clone(), kept.clone())).collect();
    same_name.extend(spellings.pairs.iter().map(|(a, b)| (a.clone(), b.clone(), String::new())));
    let outs = |pairs: &[(String, String, String)]| pairs.iter().map(|(_, out, _)| format!("`{out}`")).collect::<Vec<_>>().join(", ");
    let mut problems = Vec::new();
    if !same_name.is_empty() {
        let fixes: Vec<String> = same_name.iter().map(|(a, b, _)| rename_advice(a, b)).collect();
        problems.push(format!(
            "Some files differ only by letter case or by how an accented letter is encoded, so a Mac or Windows disk keeps only one of each pair. This build leaves out {}. To publish, {}.",
            outs(&same_name),
            fixes.join("; ")
        ));
    }
    if !shared.is_empty() {
        let fixes: Vec<String> =
            shared.iter().map(|(a, b, name)| format!("rename `{a}` or `{b}`, which are both published as `{name}`")).collect();
        problems.push(format!(
            "Some pictures or videos would be published under one name. This build leaves out {}. To publish, {}.",
            outs(&shared),
            fixes.join("; ")
        ));
    }
    let keep = |path: &str| !left_out.contains_key(path);
    for list in [&mut found.markdown_files, &mut found.html_files, &mut found.notebook_files, &mut found.other_files] {
        list.retain(|f| keep(&f.path));
    }
    found.image_files.retain(|f| keep(&f.path));
    found.video_files.retain(|f| keep(&f.path));
    found.passthrough_media.retain(|p| keep(p));
    found.total_files -= left_out.len();
    if let Some(kept) = found.homepage_file.as_ref().and_then(|home| left_out.get(home)) {
        found.homepage_file = Some(kept.clone());
    }
    problems
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

    /// Of a colliding pair the build keeps the file the advice does not name
    /// and records a problem naming both; any other folder is left whole.
    #[test]
    fn a_colliding_pair_builds_with_one_file_and_a_problem() {
        let image = |path: &str| crate::types::content::MediaMetadata { path: path.into(), ..Default::default() };
        let paths = |found: &ProjectStructure| found.image_files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        let mut found = ProjectStructure { image_files: vec![image("images/caf\u{e9}.png"), image("images/plain.png")], total_files: 2, ..Default::default() };
        assert!(leave_out_colliding_files(&mut found, &Spellings::default()).is_empty());
        assert_eq!(paths(&found).len(), 2);
        found.image_files.insert(0, image("images/Cafe\u{301}.png"));
        found.total_files = 3;
        let problems = leave_out_colliding_files(&mut found, &Spellings::default()).join(" ");
        assert!(problems.contains("leaves out `images/Cafe\u{301}.png`"), "{problems}");
        assert!(problems.contains("rename `images/Cafe\u{301}.png`, whose accent is stored as two characters, not `images/caf\u{e9}.png`"), "{problems}");
        assert_eq!(paths(&found), ["images/caf\u{e9}.png", "images/plain.png"]);
        assert_eq!(found.total_files, 2);
    }

    /// Two stored spellings of one name share a logical path, so the walk
    /// keeps the one `on_disk` reads, whichever it lists first, and the
    /// problem names both. A Mac volume cannot hold the pair.
    #[test]
    fn of_two_stored_spellings_the_walk_keeps_the_one_it_reads() {
        let root = Path::new("/no-such-site");
        let [index, decomposed, composed] = ["index.md", "images/cafe\u{301}.png", "images/caf\u{e9}.png"].map(|n| root.join(n));
        let spellings = Spellings::of(root, [&index, &decomposed, &composed].map(PathBuf::as_path));
        assert_eq!(spellings.logical(&decomposed, Path::new("images/cafe\u{301}.png")), None);
        assert_eq!(spellings.logical(&composed, Path::new("images/caf\u{e9}.png")).as_deref(), Some("images/caf\u{e9}.png"));
        assert_eq!(spellings.logical(&index, Path::new("index.md")).as_deref(), Some("index.md"));
        let problems = leave_out_colliding_files(&mut ProjectStructure::default(), &spellings).join(" ");
        assert!(problems.contains("leaves out `images/cafe\u{301}.png`"), "{problems}");
        assert!(problems.contains("rename `images/cafe\u{301}.png`, whose accent is stored as two characters, not `images/caf\u{e9}.png`"), "{problems}");
        assert_eq!(problems.matches("rename").count(), 1, "{problems}");
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

    /// A case pair and two sources for one name are named in one pass, so
    /// one rename pass fixes the folder. The scan test cannot make the case
    /// pair on a Mac disk.
    #[test]
    fn both_kinds_of_pair_are_named_at_once() {
        let image = |path: &str| crate::types::content::MediaMetadata { path: path.into(), ..Default::default() };
        let names = ["photo.jpg", "photo.png", "logo.gif", "Logo.gif"];
        let mut found = ProjectStructure { image_files: names.map(image).to_vec(), total_files: 4, ..Default::default() };
        let problems = leave_out_colliding_files(&mut found, &Spellings::default()).join(" ");
        assert!(problems.contains("rename `Logo.gif` or `logo.gif`") && problems.contains("which are both published as `photo.webp`"), "{problems}");
        let kept: Vec<_> = found.image_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(kept, ["photo.jpg", "Logo.gif"]);
    }
}
