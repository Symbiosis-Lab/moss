//! The members policy for a reader that runs before the build's own scan,
//! from the same home election the page map uses.
use std::collections::HashSet;
use std::path::Path;
use moss_core::frontmatter::member_cascade::{declared_cascade, HomeCascade, MemberCascade};
use moss_core::frontmatter::members::{public_source, Members};
use crate::build::scan::{classify, page_map, path_collision::{logical_path, nfc_name_order, on_disk}};
use crate::types::content::FileInfo;

/// The page's own `members:`, read from its public view, where a declaration
/// that cannot be read with certainty already reads as restricted.
pub(crate) fn own_access(public_view: &str) -> Option<Members> {
    moss_core::frontmatter::frontmatter_map(public_view).get("members").map(Members::from_value)
}

/// The build's verdict for one page: its own access wins, inherited access
/// otherwise, and only `public` (or no declaration) leaves the body whole.
pub(crate) fn restricted(own: Option<Members>, inherited: impl FnOnce() -> Option<Members>) -> bool {
    own.or_else(inherited).is_some_and(|access| access != Members::Public)
}

/// The access a folder's cascade gives the file at `rel_path` that is not a
/// page: `None` for a file anyone may fetch. Pictures, video and audio stay
/// public, since a members page's public part (its cover, its preview) may
/// show them and their encoders write beside them; so do a notebook, which
/// its own viewer serves, and the stylesheets, scripts and fonts a public
/// version draws with. Every other file beneath the cascade is gated by
/// where it sits, so renaming it cannot open it.
pub(crate) fn file_access(policy: &MemberCascade, rel_path: &str) -> Option<Members> {
    use moss_core::resolve::ext_kind::{reference_kind_for_ext, ExtKind};
    let ext = Path::new(rel_path).extension().map_or(String::new(), |e| e.to_string_lossy().to_lowercase());
    if matches!(reference_kind_for_ext(&ext), ExtKind::Image | ExtKind::Video | ExtKind::Audio | ExtKind::Transclusion | ExtKind::Notebook)
        || matches!(ext.as_str(), "css" | "js" | "mjs" | "woff" | "woff2" | "ttf" | "otf")
        || classify::is_page_path(Path::new(rel_path))
    {
        return None;
    }
    policy.inherited(rel_path).filter(|access| *access != Members::Public)
}

/// Whether a members page gets a members-only version: its public view holds
/// less than its source, and subscribers are who may read it. No host can
/// check a paid reader yet, so a paid page has its public version only.
pub(crate) fn has_gated_version(public_view: &str, source: &str) -> bool {
    public_view != source && own_access(public_view) == Some(Members::Subscribers)
}

/// The embeds a page may not show, one line each naming the page and the
/// embed as the author wrote it: in its public
/// source, any file only members may read; in the `whole` source of a page
/// with a members-only version, a file its own readers may not read.
pub(crate) fn gated_embeds(
    policy: &MemberCascade,
    source_path: &str,
    public_source: &str,
    whole: Option<&str>,
    parse_config: &moss_core::ast::ParseConfig,
    graph: &moss_core::content_graph::ContentGraph,
) -> Vec<String> {
    let mut embeds = embeds_of(policy, source_path, public_source, None, parse_config, graph);
    if let Some(whole) = whole {
        embeds.extend(embeds_of(policy, source_path, whole, Some(Members::Subscribers), parse_config, graph));
    }
    embeds.into_iter().map(|embed| format!("  {source_path} embeds {embed}")).collect()
}

/// The embeds in `source` of a gated file whose access is not `allowed`.
fn embeds_of(
    policy: &MemberCascade,
    source_path: &str,
    source: &str,
    allowed: Option<Members>,
    parse_config: &moss_core::ast::ParseConfig,
    graph: &moss_core::content_graph::ContentGraph,
) -> Vec<String> {
    use moss_core::resolve::asset_class::{resolve_file_target, AssetResolution};
    moss_core::resolve::authored_assets::extract_authored_asset_refs(source, parse_config)
        .into_iter()
        .filter(|reference| reference.is_embed)
        .filter(|reference| {
            let (target, _) = moss_core::resolve::embeds::split_embed_target(&reference.raw.text);
            let target = target.split('|').next().unwrap_or(target).trim();
            let file = match resolve_file_target(target, source_path, graph) {
                AssetResolution::Resolved { root_rel, .. } => root_rel,
                AssetResolution::Ambiguous { chosen, .. } => chosen,
                AssetResolution::NotFound => return false,
            };
            file_access(policy, &file).is_some_and(|access| Some(access) != allowed)
        })
        .map(|reference| reference.raw.text)
        .collect()
}

/// Stop a build that would show a file to readers who may not read it.
pub(crate) fn refuse_gated_embeds(embeds: &[String]) -> Result<(), String> {
    if embeds.is_empty() { return Ok(()); }
    Err(format!(
        "Nothing built: a page embeds a file its readers may not read:\n{}\nMove the embed below \
         `<!--members-only-->` on a page for the same members, or move the file out of the members folder.",
        embeds.join("\n")
    ))
}

/// Whether the body of the page at `rel_path`, under the site folder `root`,
/// is members-only by the rule a build of the folder applies. A page that is
/// in the cloud or cannot be read counts as restricted.
pub fn page_restricted(root: &Path, rel_path: &str) -> bool {
    let rel = logical_path(rel_path.trim_start_matches("./"));
    let path = on_disk(root, &rel);
    if crate::build::icloud::is_evicted(&path) { return true; }
    let Ok(content) = std::fs::read_to_string(path) else { return true };
    restricted(own_access(&public_source(&content)), || {
        let root = crate::vault::paths::VaultRoot::resolve(root);
        load(&site_pages(root.path()), &root, &crate::build::icloud::is_evicted).inherited(&rel)
    })
}

/// The pages a build of `root` scans, in its walk's order.
fn site_pages(root: &Path) -> Vec<FileInfo> {
    let layer_sources = crate::config::map_layers::declared_sources(root);
    walkdir::WalkDir::new(root).sort_by(nfc_name_order).into_iter()
        .filter_entry(|e| classify::left_out_with_layer_sources(e, root, &layer_sources).is_none())
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && classify::is_page_path(e.path()))
        .map(|e| {
            let path = logical_path(e.path().strip_prefix(root).unwrap_or(e.path()));
            let file_type = e.path().extension().map_or(String::new(), |ext| ext.to_string_lossy().to_lowercase());
            FileInfo { path, file_type, size: 0, modified: None }
        })
        .collect()
}

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

    fn site(pages: &[(&str, &[u8])]) -> tempfile::TempDir {
        let site = tempfile::tempdir().unwrap();
        for (path, bytes) in pages {
            let path = site.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        site
    }

    #[test]
    fn a_page_declaring_members_is_restricted() {
        let site = site(&[("index.md", b"Home"), ("post.md", b"---\nmembers: paid\n---\nBody")]);
        assert!(page_restricted(site.path(), "post.md"));
    }

    #[test]
    fn a_page_under_a_folder_cascade_is_restricted() {
        let site = site(&[("notes/index.md", b"---\ncascade: {members: paid}\n---\nNotes"), ("notes/article.md", b"Article")]);
        assert!(page_restricted(site.path(), "notes/article.md"));
        assert!(page_restricted(site.path(), "./notes/article.md"), "a path spelled from the folder still meets the cascade");
        assert!(!page_restricted(site.path(), "notes/index.md"), "a home's cascade covers its folder, not itself");
    }

    #[test]
    fn a_public_page_and_a_public_override_are_not_restricted() {
        let site = site(&[
            ("index.md", b"Home"),
            ("post.md", b"Post"),
            ("notes/index.md", b"---\ncascade: {members: subscribers}\n---\nNotes"),
            ("notes/open.md", b"---\nmembers: public\n---\nOpen"),
        ]);
        assert!(!page_restricted(site.path(), "post.md"));
        assert!(!page_restricted(site.path(), "notes/open.md"));
    }

    #[test]
    fn a_page_that_cannot_be_read_is_restricted() {
        let site = site(&[("index.md", b"Home"), ("post.md", &[0xff, 0xfe])]);
        assert!(page_restricted(site.path(), "post.md"));
        assert!(page_restricted(site.path(), "missing.md"));
    }
}
