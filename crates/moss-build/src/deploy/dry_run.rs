//! `moss deploy --dry-run`: what a deploy would do, as far as can be known
//! without doing it.
//!
//! The build runs exactly as a deploy's build runs, plugins the flags allow
//! included, and the checks a deploy makes before it builds that are purely
//! local run too (is the folder set up to publish, is the signing key there,
//! is the plugin's setup complete), so a dry run fails where a deploy would.
//! Then it stops: no upload, no publish record, no acceptance, no site
//! registered, no key created. What needs the server is not checked, and the
//! report says so rather than promising the deploy goes ahead.

use std::path::Path;

use crate::build::manifest::change_set::{self, ChangeSet, PageVerb};
use crate::build::manifest::{backfill, published_record};
use crate::deploy::removal_gate::{capped_lines, capped_lines_cut, cause_line};
use crate::deploy::route::DeployRoute;
use crate::moss_paths::MossPaths;

/// What a deploy would send, and what that is measured against.
#[derive(Debug)]
pub enum Transfer {
    /// Files that differ from this folder's last publish record.
    SinceLastPublish { upload: u32, remove: u32 },
    /// No record of an earlier publish from this folder, so nothing to count
    /// against; only the server could say.
    NoRecord,
    /// A deploy plugin sends its own way and moss counts nothing.
    NotCounted,
}

#[derive(Debug)]
pub struct DryRun {
    pub change_set: ChangeSet,
    pub transfer: Transfer,
    /// The publish gate's refusal text, when a real deploy would stop at it.
    pub refusal: Option<String>,
    /// The site a first publish would register. Not registered.
    pub would_register: Option<String>,
    /// A deploy creates a signing key when the site has none. Not created.
    pub would_create_key: bool,
}

impl DryRun {
    pub fn render(&self) -> String {
        let set = &self.change_set;
        let mut out = String::from(
            "Dry run: the site was built exactly as a deploy builds it, including any plugin this command allows. The build may use the network as any build does: a plugin's own requests, and link previews fetched from third-party sites. \
             Nothing was uploaded, no publish record was written and no acceptance was recorded.\n",
        );
        if set.classified {
            out.push_str(&format!(
                "Pages: {} added, {} edited, {} deleted, {} restyled\n",
                set.added, set.edited, set.deleted, set.restyled
            ));
            out.push_str(&page_lines(&set.pages));
        } else {
            out.push_str("Pages: not classified, because there is no record of an earlier publish from this folder to this target.\n");
        }
        if !set.removed.is_empty() {
            out.push_str(&format!("Addresses going offline: {}\n", set.removed.len()));
            out.push_str(&capped_lines(&set.removed, cause_line));
        }
        match &self.transfer {
            Transfer::SinceLastPublish { upload, remove } => out.push_str(&format!(
                "Transfer, measured against this folder's last publish: would upload {} and remove {}.\n",
                files(*upload),
                files(*remove)
            )),
            Transfer::NoRecord => out.push_str(
                "Transfer: not known, because there is no record of an earlier publish from this folder.\n",
            ),
            Transfer::NotCounted => out.push_str("Transfer: not counted for a deploy plugin.\n"),
        }
        if let Some(site) = &self.would_register {
            out.push_str(&format!("A first publish would register the site \"{site}\" and create a signing key. Neither was done.\n"));
        } else if self.would_create_key {
            out.push_str("This folder's site has no signing key; a real deploy would create a new one. It was not created.\n");
        }
        match &self.refusal {
            Some(text) => out.push_str(&format!("Publish check: a real deploy would be refused:\n{text}\n")),
            None => out.push_str("Publish check: passes.\n"),
        }
        out.push_str(
            "Not run, because it needs the server: whether another copy of this folder published since this one, and the upload itself. \
             A real deploy can still stop there.\n",
        );
        out
    }
}

/// The pages, the ones the author changed first: added, edited, deleted, then
/// restyled (the order of the summary line), by path within a verb, so the cap
/// cuts restyled pages, whose source did not change, before any other.
fn page_lines(pages: &[change_set::ChangedPage]) -> String {
    let rank = |v: PageVerb| match v {
        PageVerb::Added => 0,
        PageVerb::Edited => 1,
        PageVerb::Deleted => 2,
        PageVerb::Restyled => 3,
    };
    let mut ordered: Vec<&change_set::ChangedPage> = pages.iter().collect();
    ordered.sort_by(|a, b| (rank(a.verb), &a.source_path).cmp(&(rank(b.verb), &b.source_path)));
    capped_lines_cut(
        &ordered,
        |p| format!("{:<8} {}", verb_word(p.verb), p.source_path),
        |rest| {
            // What was cut, per verb: "and 60 more restyled", or
            // "and 5 more: 2 edited, 3 deleted".
            let per_verb: Vec<String> = [PageVerb::Added, PageVerb::Edited, PageVerb::Deleted, PageVerb::Restyled]
                .into_iter()
                .filter_map(|v| {
                    let n = rest.iter().filter(|p| p.verb == v).count();
                    (n > 0).then(|| format!("{n} {}", verb_word(v)))
                })
                .collect();
            match per_verb.as_slice() {
                [only] => format!("and {} more {}", rest.len(), only.split_once(' ').map_or("", |(_, w)| w)),
                _ => format!("and {} more: {}", rest.len(), per_verb.join(", ")),
            }
        },
    )
}

fn verb_word(verb: PageVerb) -> &'static str {
    match verb {
        PageVerb::Added => "added",
        PageVerb::Edited => "edited",
        PageVerb::Deleted => "deleted",
        PageVerb::Restyled => "restyled",
    }
}

fn files(n: u32) -> String {
    if n == 1 { "1 file".to_string() } else { format!("{n} files") }
}

/// What the local checks found.
struct Local {
    would_register: Option<String>,
    would_create_key: bool,
}

/// The checks a deploy makes before it sends anything that need nothing from
/// the network, in the order and with the answers a deploy gives them:
/// the plugin's setup (plugin route), the config version, whether the folder
/// has a site (or may register one, under a valid name), and the identity
/// files. A missing signing key is not a refusal: a deploy creates one, so the
/// dry run says so and creates nothing.
async fn check_locally(
    folder: &Path,
    route: &DeployRoute,
    requested_site_id: Option<&str>,
) -> Result<Local, String> {
    let folder_str = folder.to_string_lossy().to_string();
    if matches!(route, DeployRoute::Plugin { .. }) {
        crate::deploy::publish_setup::refuse_publish(&folder_str)?;
    }
    crate::build::site_config::ensure_config_current(&folder_str)?;
    if matches!(route, DeployRoute::Plugin { .. }) {
        return Ok(Local { would_register: None, would_create_key: false });
    }
    let config = crate::build::site_config::get_domain_config(&folder_str)?;
    let would_register = match config.site_id {
        Some(_) => None,
        None => {
            if !crate::deploy::first_publish_is_permitted(&folder_str)? {
                return Err(format!(
                    "this folder has no site yet. Run `moss env <staging|production|local> {}` first, then deploy again.",
                    folder.display()
                ));
            }
            let chosen = requested_site_id.map(str::to_string).unwrap_or_else(|| {
                crate::deploy::derive_site_id_from_root(&crate::vault::paths::VaultRoot::resolve(folder))
            });
            crate::deploy::validate_site_id(&chosen)?;
            Some(chosen)
        }
    };
    // A deploy waits for a cloud-synced key to arrive before reading it; reads only.
    crate::deploy::preflight_publish_inputs(folder).await?;
    // Read-only stand-in for `ensure_signing_key`, which creates the identity
    // when there is none and regenerates the key when its file is missing.
    let would_create_key = !crate::identity::Identity::exists(folder)
        || !crate::identity::Identity::has_usable_signing_key(folder);
    Ok(Local { would_register, would_create_key })
}

/// Build the folder as a deploy would and describe the publish that would
/// follow. Skips what a deploy does that writes or sends: registering a site,
/// creating a key, asking the server whether this copy is behind, uploading.
pub async fn run_dry_run(
    folder: &Path,
    route: &DeployRoute,
    requested_site_id: Option<&str>,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
) -> Result<DryRun, String> {
    let local = check_locally(folder, route, requested_site_id).await?;

    let folder_str = folder.to_string_lossy().to_string();
    let _session = crate::system::folder_session::register_session(&folder_str);
    let root = crate::vault::paths::VaultRoot::resolve(folder);
    // The seal already classified the build against the last publish; asking
    // again would walk the tree twice.
    let (sealed, change_set) =
        super::one_shot::build_and_describe(&root, host_ports(&folder_str), plugins).await?;
    let sealed = super::one_shot::require_sealed(sealed)?;
    let change_set = change_set.ok_or("the build produced no change set to describe")?;

    let transfer = if matches!(route, DeployRoute::Plugin { .. }) {
        Transfer::NotCounted
    } else {
        let mp = MossPaths::new(folder);
        let target = backfill::publish_target(&mp);
        match published_record::load_for(&mp, target.as_deref()) {
            Some(record) => {
                let flat = change_set::flat_against(&record.files, &sealed);
                Transfer::SinceLastPublish { upload: flat.flat_upload, remove: flat.flat_remove }
            }
            None => Transfer::NoRecord,
        }
    };
    let refusal = crate::deploy::refuse_publish(&folder_str)
        .and_then(|_| crate::deploy::refuse_foreign_inputs(&sealed, folder))
        .and_then(|_| crate::deploy::refuse_unresolved_inputs(&sealed)).err();
    Ok(DryRun {
        change_set,
        transfer,
        refusal,
        would_register: local.would_register,
        would_create_key: local.would_create_key && !matches!(route, DeployRoute::Plugin { .. }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::change_set::{ChangedPage, RemovalReason, RemovedAddress};

    fn small() -> DryRun {
        DryRun {
            change_set: ChangeSet {
                classified: true,
                added: 1,
                pages: vec![ChangedPage { source_path: "new.md".into(), verb: PageVerb::Added }],
                ..ChangeSet::default()
            }
            .with_removed(vec![RemovedAddress {
                path: "legacy-feed.xml".into(),
                reason: RemovalReason::Unexplained,
                moved_to: None,
                source: None,
            }]),
            transfer: Transfer::SinceLastPublish { upload: 2, remove: 1 },
            refusal: Some("Nothing published — REFUSED".into()),
            would_register: None,
            would_create_key: false,
        }
    }

    /// The one page the author changed must be in the list however many pages
    /// were only restyled: verbs first (added, edited, deleted, restyled), then
    /// path, and the cap cuts restyled pages before any other.
    #[test]
    fn the_one_edited_page_is_listed_ahead_of_seventy_nine_restyled_ones() {
        let mut pages: Vec<ChangedPage> = (0..79)
            .map(|i| ChangedPage { source_path: format!("a-{i:02}.md"), verb: PageVerb::Restyled })
            .collect();
        pages.push(ChangedPage { source_path: "z-edited.md".into(), verb: PageVerb::Edited });
        pages.sort_by(|a, b| a.source_path.cmp(&b.source_path));
        let dry = DryRun {
            change_set: ChangeSet { classified: true, edited: 1, restyled: 79, pages, ..ChangeSet::default() },
            transfer: Transfer::NoRecord,
            refusal: None,
            would_register: None,
            would_create_key: false,
        };

        let text = dry.render();

        assert!(text.contains("Pages: 0 added, 1 edited, 0 deleted, 79 restyled\n  edited   z-edited.md\n  restyled a-00.md\n"), "{text}");
        assert!(text.contains("  and 60 more restyled\n"), "{text}");
    }

    fn listing(pages: &[(PageVerb, usize)]) -> String {
        let pages = pages
            .iter()
            .flat_map(|(verb, n)| {
                (0..*n).map(move |i| ChangedPage { source_path: format!("{verb:?}-{i:02}.md"), verb: *verb })
            })
            .collect::<Vec<_>>();
        page_lines(&pages)
    }

    /// More than the cap of pages the author changed: the cut line says what
    /// was cut, per verb, and no restyled page is listed before an added one.
    #[test]
    fn more_than_twenty_changed_pages_say_what_was_cut_per_verb() {
        let text = listing(&[(PageVerb::Added, 18), (PageVerb::Edited, 4), (PageVerb::Deleted, 3), (PageVerb::Restyled, 5)]);
        assert!(text.contains("  edited   Edited-01.md\n"), "{text}");
        assert!(!text.contains("Edited-02") && !text.contains("  restyled "), "{text}");
        assert!(text.ends_with("  and 10 more: 2 edited, 3 deleted, 5 restyled\n"), "{text}");
    }

    #[test]
    fn a_cut_of_one_verb_names_just_that_verb() {
        assert!(listing(&[(PageVerb::Added, 25)]).ends_with("  and 5 more added\n"));
    }

    #[test]
    fn a_refused_dry_run_reads_as_a_report_and_names_what_it_did_not_run() {
        assert_eq!(
            small().render(),
            "Dry run: the site was built exactly as a deploy builds it, including any plugin this command allows. The build may use the network as any build does: a plugin's own requests, and link previews fetched from third-party sites. \
             Nothing was uploaded, no publish record was written and no acceptance was recorded.\n\
             Pages: 1 added, 0 edited, 0 deleted, 0 restyled\n\
             \x20 added    new.md\n\
             Addresses going offline: 1\n\
             \x20 /legacy-feed.xml is no longer produced by your site.\n\
             Transfer, measured against this folder's last publish: would upload 2 files and remove 1 file.\n\
             Publish check: a real deploy would be refused:\n\
             Nothing published — REFUSED\n\
             Not run, because it needs the server: whether another copy of this folder published since this one, and the upload itself. \
             A real deploy can still stop there.\n"
        );
    }

    /// Never a promise that the deploy goes ahead.
    #[test]
    fn a_passing_dry_run_says_the_check_passes_and_not_that_the_deploy_goes_ahead() {
        let dry = DryRun {
            change_set: ChangeSet::default(),
            transfer: Transfer::NoRecord,
            refusal: None,
            would_register: Some("my-site".into()),
            would_create_key: true,
        };
        let text = dry.render();
        assert!(text.contains("Pages: not classified"), "{text}");
        assert!(text.contains("Transfer: not known, because there is no record of an earlier publish"), "{text}");
        assert!(text.contains("would register the site \"my-site\""), "{text}");
        assert!(text.contains("Publish check: passes.\n"), "{text}");
        assert!(text.contains("Not run, because it needs the server"), "{text}");
        assert!(!text.contains("go ahead"), "{text}");
    }
}
