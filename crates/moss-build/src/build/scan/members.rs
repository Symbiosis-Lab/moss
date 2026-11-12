//! The members policy for a reader that runs before the build's own scan,
//! from the same home election the page map uses.
use std::collections::HashSet;
use std::path::Path;
use moss_core::frontmatter::member_cascade::{declared_cascade, HomeCascade, MemberCascade};
use crate::build::scan::{page_map, path_collision::on_disk};
use crate::types::content::FileInfo;

/// Whether the site's settings name members (`[members]` in `.moss/config.toml`).
/// Settings that cannot be read count as naming them.
pub(crate) fn config_declares_members(root: &Path) -> bool {
    crate::build::site_config::read_project_config(&root.to_string_lossy()).map_or(true, |config| config.section(&["members"]).is_some())
}

/// The pages are not read here, so what the last build read of them stands
/// in as evidence that the site uses members.
pub(crate) fn load(files: &[FileInfo], root: &crate::vault::paths::VaultRoot, is_evicted: &dyn Fn(&Path) -> bool) -> MemberCascade {
    let locate = |p: &str| on_disk(Path::new(root.path()), p);
    // No reserved folders: a home they would displace still restricts here.
    let (overrides, winners) = page_map::elect_homes_with_evicted(files, root, &HashSet::new(), &locate, is_evicted);
    let scanned = page_map::FrontmatterScanCache::load(&crate::moss_paths::MossPaths::new(root.path()).cache_frontmatter_scan());
    let uses_members = config_declares_members(root.path()) || scanned.declares_members(files);
    MemberCascade::from_homes(page_map::folder_homes(&winners, &overrides, root.name()).into_iter().map(|home| {
        let path = locate(home);
        let cascade = if is_evicted(&path) { HomeCascade::Pending } else {
            std::fs::read_to_string(path).map_or(HomeCascade::Unreadable, |text| HomeCascade::Read(declared_cascade(&text)))
        };
        (home, cascade)
    }), uses_members)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::frontmatter::members::Members;

    /// A root given as `.` still elects its self-named home: the root's name
    /// comes from the resolved folder, not the path as typed.
    #[test]
    fn a_self_named_root_home_of_a_dot_root_declares_for_the_site() {
        let parent = tempfile::tempdir().unwrap();
        let site = parent.path().join("journal");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(site.join("journal.md"), "---\ncascade: {members: paid}\n---\nHome").unwrap();
        std::fs::write(site.join("post.md"), "Post").unwrap();
        let files: Vec<_> = ["journal.md", "post.md"].map(|path| FileInfo { path: path.into(), file_type: "md".into(), size: 0, modified: None }).into();
        let root = crate::vault::paths::VaultRoot::resolve_in(Path::new("."), &site);
        let policy = load(&files, &root, &|_| false);
        assert_eq!(policy.inherited("post.md"), Some(Members::Paid));
        assert_eq!(policy.inherited("journal.md"), None);
    }

    /// The pre-scan reader applies the same rule to a home it cannot read.
    #[test]
    fn an_unread_home_restricts_only_where_the_settings_or_pages_use_members() {
        let site = tempfile::tempdir().unwrap();
        std::fs::create_dir(site.path().join("notes")).unwrap();
        std::fs::write(site.path().join("notes/index.md"), "Notes").unwrap();
        std::fs::write(site.path().join("notes/article.md"), "Article").unwrap();
        let files: Vec<_> = ["notes/index.md", "notes/article.md"].map(|path| FileInfo { path: path.into(), file_type: "md".into(), size: 0, modified: None }).into();
        let root = crate::vault::paths::VaultRoot::resolve(site.path());
        let unread = |p: &Path| p.ends_with("notes/index.md");
        assert_eq!(load(&files, &root, &unread).inherited("notes/article.md"), None);
        std::fs::write(site.path().join("notes/index.md"), [0xff, 0xfe]).unwrap();
        assert_eq!(load(&files, &root, &|_| false).inherited("notes/article.md"), Some(Members::Subscribers), "an unreadable home restricts");
        std::fs::create_dir(site.path().join(".moss")).unwrap();
        std::fs::write(site.path().join(".moss/config.toml"), "[members]\n").unwrap();
        assert_eq!(load(&files, &root, &unread).inherited("notes/article.md"), Some(Members::Subscribers));
    }
}
