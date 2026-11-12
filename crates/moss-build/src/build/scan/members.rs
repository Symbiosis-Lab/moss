//! The members policy for a reader that runs before the build's own scan,
//! from the same home election the page map uses.
use std::collections::HashSet;
use std::path::Path;
use moss_core::frontmatter::member_cascade::{declared_cascade, MemberCascade};
use moss_core::frontmatter::members::Members;
use crate::build::scan::{page_map, path_collision::on_disk};
use crate::types::content::FileInfo;

pub(crate) fn load(files: &[FileInfo], root: &crate::vault::paths::VaultRoot, is_evicted: &dyn Fn(&Path) -> bool) -> MemberCascade {
    let locate = |p: &str| on_disk(Path::new(root.path()), p);
    // No reserved folders: a home they would displace still restricts here.
    let (overrides, winners) = page_map::elect_homes_with_evicted(files, root, &HashSet::new(), &locate, is_evicted);
    MemberCascade::from_homes(page_map::folder_homes(&winners, &overrides, root.name()).into_iter().map(|home| {
        let path = locate(home);
        let text = if is_evicted(&path) { None } else { std::fs::read_to_string(path).ok() };
        (home, text.map_or(Some(Members::Subscribers), |text| declared_cascade(&text)))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
