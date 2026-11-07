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
use crate::deploy::removal_gate::{capped_lines_cut, cause_line, more_in_json, JSON_LISTS_ALL};
use crate::deploy::route::DeployRoute;
use crate::moss_paths::MossPaths;

/// What a deploy would send, and what that is measured against.
#[derive(Debug)]
pub enum Transfer {
    /// Files that differ from this folder's last publish record.
    /// `removed` is every output path the last publish had and this build
    /// does not; its length is the count of files a deploy would remove.
    SinceLastPublish { upload: u32, removed: Vec<String> },
    /// No record of an earlier publish from this folder, so nothing to count
    /// against; only the server could say.
    NoRecord,
    /// A deploy plugin sends its own way and moss counts nothing.
    NotCounted,
}

/// An old address kept answering: `from` forwards to, or is a copy of, `to`.
#[derive(Debug, PartialEq, Eq)]
pub struct Kept {
    pub from: String,
    pub to: String,
}

#[derive(Debug)]
pub struct DryRun {
    pub change_set: ChangeSet,
    pub transfer: Transfer,
    /// The refusal a real deploy would stop at, with the rule that made it.
    pub refusal: Option<crate::deploy::Refusal>,
    /// The site a first publish would register. Not registered.
    pub would_register: Option<String>,
    /// A deploy creates a signing key when the site has none. Not created.
    pub would_create_key: bool,
    /// A first publish would register a site, and this folder's key has no
    /// email recorded, so the server will most likely refuse until the key is
    /// verified. Nothing was sent to find out.
    pub needs_verification: bool,
    /// The folder was read as it is, with no build.
    pub prebuilt: bool,
    /// Old addresses kept answering without a line of their own: an image
    /// derivative the build stopped producing (from its picture), and each
    /// file under a moved folder: `(address, served address)`.
    pub kept_as_redirect: Vec<Kept>,
    /// The removals that refuse this publish, as the publish gate holds them.
    pub pending_removals: Vec<change_set::PendingRemoval>,
    /// The copies a publish would post to other networks, each shaped.
    pub copies: Vec<crate::syndication::preview::PreviewCopy>,
}

impl DryRun {
    /// What `moss deploy --dry-run --json` prints: every list in full. A field
    /// named `output_path` is a file of the build output (`a/index.html`); one
    /// named `address`, `moved_to` or a move's `to` is what a visitor asks for
    /// (`/a/`); one named `source_path` is a file in the site folder.
    pub fn to_json(&self) -> serde_json::Value {
        use crate::build::served_path::served_address;
        use serde_json::json;
        // Every field, so a new one is a build error here until it is printed.
        let DryRun {
            change_set: set,
            transfer,
            refusal,
            would_register,
            would_create_key,
            needs_verification,
            prebuilt,
            kept_as_redirect,
            pending_removals,
            copies,
        } = self;
        let mut change_set = serde_json::to_value(set).expect("a change set serializes");
        if let Some(fields) = change_set.as_object_mut() {
            fields.remove("removed");
        }
        let offline: Vec<_> = set.removed.iter().map(|r| json!({
            "output_path": r.path,
            "address": served_address(&r.path),
            "reason": r.reason,
            "moved_to": r.moved_to.as_deref().map(served_address),
            "source_path": r.source,
        })).collect();
        let pending: Vec<_> = pending_removals.iter()
            .map(|p| json!({ "output_path": p.path, "address": p.address, "cause": p.cause }))
            .collect();
        let transfer = match transfer {
            Transfer::SinceLastPublish { upload, removed } => json!({ "kind": "since_last_publish", "upload": upload, "removed_output_paths": removed }),
            Transfer::NoRecord => json!({ "kind": "no_record" }),
            Transfer::NotCounted => json!({ "kind": "not_counted" }),
        };
        json!({
            "change_set": change_set,
            "addresses_going_offline": offline,
            "refusal": refusal,
            "pending_removals": pending,
            "redirects_to_add": crate::deploy::removal_gate::redirect_lines(pending_removals),
            "kept": kept_as_redirect.iter().map(|k| json!({ "address": k.from, "to": k.to })).collect::<Vec<_>>(),
            "transfer": transfer,
            "would_register": would_register,
            "would_create_key": would_create_key,
            "needs_verification": needs_verification,
            "prebuilt": prebuilt,
            "copies": copies,
        })
    }

    pub fn render(&self) -> String {
        let set = &self.change_set;
        let mut out = if self.prebuilt {
            String::from("Dry run: nothing was built; the folder was read as it is. ")
        } else {
            String::from(
                "Dry run: the site was built exactly as a deploy builds it, including any plugin this command allows. The build may use the network as any build does: a plugin's own requests, and link previews fetched from third-party sites. ",
            )
        };
        out.push_str("Nothing was uploaded, no publish record was written and no acceptance was recorded.\n");
        if self.prebuilt {
            out.push_str("Pages: not classified, because a prebuilt folder has no sources, so counts are in files.\n");
        } else if set.classified {
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
            out.push_str(&capped_lines_cut(&set.removed, cause_line, more_in_json));
        }
        if !self.kept_as_redirect.is_empty() {
            out.push_str("Kept answering at the old address (a picture still served, or a folder redirect):\n");
            out.push_str(&capped_lines_cut(&self.kept_as_redirect, |k| format!("{} -> {}", k.from, k.to), more_in_json));
        }
        match &self.transfer {
            Transfer::SinceLastPublish { upload, removed } => {
                out.push_str(&format!(
                    "Transfer, measured against this folder's last publish: would upload {} and remove {}.\n",
                    files(*upload),
                    files(removed.len() as u32)
                ));
                // A removed public address is already listed above; this is the rest.
                let rest: Vec<String> = removed
                    .iter()
                    .filter(|p| !set.removed.iter().any(|a| &a.path == *p))
                    .map(|p| crate::build::served_path::served_address(p))
                    .collect();
                if !rest.is_empty() {
                    out.push_str(if set.removed.is_empty() { "Removed:\n" } else { "Also removed:\n" });
                    out.push_str(&capped_lines_cut(&rest, String::clone, more_in_json));
                }
            }
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
        if self.needs_verification {
            out.push_str("This folder's key is not linked to an email yet, so registering would need verification: a real deploy needs `--email <address>` (the account the site should belong to) to be sent a sign-in link. Nothing was sent.\n");
        }
        match &self.refusal {
            Some(r) => out.push_str(&format!("Publish check: a real deploy would be refused:\n{}\n", r.text)),
            None => out.push_str("Publish check: passes.\n"),
        }
        out.push_str(
            "Not run, because it needs the server: whether another copy of this folder published since this one, and the upload itself. \
             A real deploy can still stop there.\n",
        );
        for c in &self.copies {
            let what = match (&c.action, &c.error, c.form) {
                (_, Some(e), _) => format!("not postable: {e}"),
                (crate::syndication::plan::Action::Skip { reason }, ..) => format!("not posted: {reason}"),
                (crate::syndication::plan::Action::Adopt { url }, ..) => format!("already there: {url}"),
                (action, None, form) => {
                    let verb = if matches!(action, crate::syndication::plan::Action::Update) { "would update" } else { "would post" };
                    let sizes: Vec<String> = c.parts.iter().map(|p| match p.max_chars {
                        Some(max) => format!("{}/{max}", p.chars),
                        None => p.chars.to_string(),
                    }).collect();
                    format!("{verb} as {} ({} characters)", form.map(|f| format!("{f:?}").to_lowercase()).unwrap_or_default(), sizes.join(", "))
                }
            };
            out.push_str(&format!("Copy to {}: {} — {what}\n", c.channel, c.source_path));
        }
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
                [only] => format!("and {} more {}; {JSON_LISTS_ALL}", rest.len(), only.split_once(' ').map_or("", |(_, w)| w)),
                _ => format!("and {} more: {}; {JSON_LISTS_ALL}", rest.len(), per_verb.join(", ")),
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

/// The refusal for a folder with no site and no environment. A real deploy
/// and a dry run both give exactly this, so they cannot drift apart.
pub fn no_site_message(folder: &str) -> String {
    format!("this folder has no site yet. Run `moss env <staging|production|local> {folder}` first, then deploy again.")
}

/// What the local checks found.
struct Local {
    would_register: Option<String>,
    would_create_key: bool,
    needs_verification: bool,
    /// Set when the folder has no site and no environment: the refusal a real
    /// deploy gives, which a dry run reports rather than stops at.
    no_environment: Option<String>,
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
        return Ok(Local { would_register: None, would_create_key: false, needs_verification: false, no_environment: None });
    }
    let config = crate::build::site_config::get_domain_config(&folder_str)?;
    let mut no_environment = None;
    let would_register = match config.site_id {
        Some(_) => None,
        None if !crate::deploy::first_publish_is_permitted(&folder_str)? => {
            no_environment = Some(no_site_message(&folder.display().to_string()));
            None
        }
        None => {
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
    let needs_verification =
        would_register.is_some() && crate::identity::Identity::read_email(folder).is_none();
    Ok(Local { would_register, would_create_key, needs_verification, no_environment })
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
                Transfer::SinceLastPublish {
                    upload: change_set::upload_count(&record.files, &sealed) as u32,
                    removed: change_set::removed_files(&record.files, &sealed),
                }
            }
            None => Transfer::NoRecord,
        }
    };
    use crate::deploy::Refusal;
    let refusal = match &local.no_environment {
        Some(text) => Some(Refusal::new(crate::deploy::RefusalKind::NoEnvironment, text.clone())),
        None => crate::deploy::publish_refusal(&folder_str)
            .or_else(|| crate::deploy::removal_gate::refuse_stale_generation(folder, &sealed).err().map(|t| Refusal::new(crate::deploy::RefusalKind::StaleContent, t)))
            .or_else(|| crate::deploy::refuse_foreign_inputs(&sealed, folder).err().map(|t| Refusal::new(crate::deploy::RefusalKind::ForeignInputs, t)))
            .or_else(|| crate::deploy::refuse_unresolved_inputs(&sealed).err().map(|t| Refusal::new(crate::deploy::RefusalKind::UnresolvedInputs, t))),
    };
    Ok(DryRun {
        change_set,
        transfer,
        refusal,
        would_register: local.would_register,
        would_create_key: local.would_create_key && !matches!(route, DeployRoute::Plugin { .. }),
        needs_verification: local.needs_verification,
        prebuilt: false,
        kept_as_redirect: kept_as_redirect(&folder_str),
        pending_removals: crate::system::build_records::records().pending_removals(&folder_str),
        copies: crate::syndication::preview::for_dry_run(folder, &sealed),
    })
}

fn kept_as_redirect(folder: &str) -> Vec<Kept> {
    use crate::build::served_path::served_address;
    crate::system::build_records::records()
        .kept(folder)
        .into_iter()
        .map(|(from, to)| Kept { from: served_address(&from), to: served_address(&to) })
        .collect()
}

/// Describe the publish of a prebuilt folder: the folder is read as it is,
/// with the same manifest the upload hashes and the same record and removal
/// check the upload's gate uses. Sends and writes nothing.
pub async fn run_prebuilt_dry_run(
    folder: &Path,
    dir: &Path,
    requested_site_id: Option<&str>,
    accept_removals: bool,
) -> Result<DryRun, String> {
    let route = DeployRoute::Prebuilt { dir: dir.to_path_buf() };
    let local = check_locally(folder, &route, requested_site_id).await?;
    if let Some(text) = local.no_environment {
        return Err(text);
    }
    let folder_str = folder.to_string_lossy().to_string();
    let site_id = match &local.would_register {
        Some(id) => id.clone(),
        None => crate::build::site_config::get_domain_config(&folder_str)?
            .site_id
            .ok_or("this folder has no site yet")?,
    };

    let manifest = super::prebuilt::build_manifest_from_dir(dir)?;
    let (transfer, refusal) = match super::prebuilt::removed_against_last_publish(folder, &site_id, &manifest) {
        Some((baseline, removed)) => {
            let refusal = (!accept_removals && !removed.is_empty())
                .then(|| crate::deploy::Refusal::new(crate::deploy::RefusalKind::Removals, crate::deploy::removal_gate::prebuilt_refusal(&removed)));
            let upload = change_set::upload_count_of(&baseline.files, &manifest) as u32;
            (Transfer::SinceLastPublish { upload, removed }, refusal)
        }
        None => (Transfer::NoRecord, None),
    };
    Ok(DryRun {
        change_set: ChangeSet::default(),
        transfer,
        refusal,
        would_register: local.would_register,
        would_create_key: local.would_create_key,
        needs_verification: local.needs_verification,
        prebuilt: true,
        kept_as_redirect: Vec::new(),
        pending_removals: Vec::new(),
        copies: Vec::new(),
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
                withdrawn_by: None,
            }]),
            transfer: Transfer::SinceLastPublish { upload: 2, removed: vec!["legacy-feed.xml".into(), "style.1a2b3c.css".into()] },
            refusal: Some(crate::deploy::Refusal::new(crate::deploy::RefusalKind::Removals, "Nothing published — REFUSED".into())),
            would_register: None,
            would_create_key: false,
            needs_verification: false,
            prebuilt: false,
            kept_as_redirect: Vec::new(),
            pending_removals: Vec::new(),
            copies: Vec::new(),
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
            needs_verification: false,
            prebuilt: false,
            kept_as_redirect: Vec::new(),
            pending_removals: Vec::new(),
            copies: Vec::new(),
        };

        let text = dry.render();

        assert!(text.contains("Pages: 0 added, 1 edited, 0 deleted, 79 restyled\n  edited   z-edited.md\n  restyled a-00.md\n"), "{text}");
        assert!(text.contains("  and 60 more restyled; `moss deploy --dry-run --json` lists every one\n"), "{text}");
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
        assert!(text.ends_with("  and 10 more: 2 edited, 3 deleted, 5 restyled; `moss deploy --dry-run --json` lists every one\n"), "{text}");
    }

    #[test]
    fn a_cut_of_one_verb_names_just_that_verb() {
        assert!(listing(&[(PageVerb::Added, 25)]).ends_with("  and 5 more added; `moss deploy --dry-run --json` lists every one\n"));
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
             Transfer, measured against this folder's last publish: would upload 2 files and remove 2 files.\n\
             Also removed:\n\
             \x20 /style.1a2b3c.css\n\
             Publish check: a real deploy would be refused:\n\
             Nothing published — REFUSED\n\
             Not run, because it needs the server: whether another copy of this folder published since this one, and the upload itself. \
             A real deploy can still stop there.\n"
        );
    }

    #[test]
    fn a_derivative_kept_as_a_redirect_is_named_and_the_check_still_passes() {
        let dry = DryRun {
            kept_as_redirect: vec![Kept { from: "/photo-2.webp".into(), to: "/photo-2.jpg".into() }],
            pending_removals: Vec::new(),
            refusal: None,
            ..small()
        };
        let text = dry.render();
        assert!(
            text.contains("Kept answering at the old address (a picture still served, or a folder redirect):\n  /photo-2.webp -> /photo-2.jpg\n"),
            "{text}"
        );
        assert!(text.contains("Publish check: passes.\n"), "{text}");
    }

    #[test]
    fn removed_files_not_already_offline_addresses_are_listed_and_capped() {
        let removed: Vec<String> = (0..23).map(|i| format!("old-{i:02}.css")).collect();
        let dry = DryRun { transfer: Transfer::SinceLastPublish { upload: 0, removed }, ..small() };
        let text = dry.render();
        assert!(text.contains("remove 23 files.\nAlso removed:\n  /old-00.css\n  /old-01.css\n"), "{text}");
        assert!(text.contains("  /old-19.css\n  and 3 more; `moss deploy --dry-run --json` lists every one\n"), "{text}");
        assert!(!text.contains("old-20"), "{text}");
    }

    #[test]
    fn a_removed_page_is_listed_once_as_its_address_and_hashed_assets_as_served() {
        let mut dry = small();
        dry.change_set.removed.push(RemovedAddress {
            path: "a/index.html".into(),
            reason: RemovalReason::Unexplained,
            moved_to: None,
            source: None,
            withdrawn_by: None,
        });
        dry.transfer = Transfer::SinceLastPublish {
            upload: 0,
            removed: vec!["a/index.html".into(), "legacy-feed.xml".into(), "style.1a2b3c.css".into()],
        };
        let text = dry.render();
        assert_eq!(text.matches("/a/").count(), 1, "{text}");
        assert!(text.contains("Also removed:\n  /style.1a2b3c.css\n"), "{text}");
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
            needs_verification: false,
            prebuilt: false,
            kept_as_redirect: Vec::new(),
            pending_removals: Vec::new(),
            copies: Vec::new(),
        };
        let text = dry.render();
        assert!(text.contains("Pages: not classified"), "{text}");
        assert!(text.contains("Transfer: not known, because there is no record of an earlier publish"), "{text}");
        assert!(text.contains("would register the site \"my-site\""), "{text}");
        assert!(text.contains("Publish check: passes.\n"), "{text}");
        assert!(text.contains("Not run, because it needs the server"), "{text}");
        assert!(!text.contains("go ahead"), "{text}");
    }

    mod prebuilt {
        use super::*;
        use crate::build::manifest::change_set::PublishedSnapshot;

        fn entry(bytes: &str) -> String {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("f"), bytes).unwrap();
            super::super::super::prebuilt::build_manifest_from_dir(dir.path()).unwrap()["f"].clone()
        }

        /// A registered folder and a prebuilt `_site` holding `files`; the
        /// last publish served `served`, if given.
        fn folder(files: &[(&str, &str)], served: Option<&[(&str, &str)]>) -> tempfile::TempDir {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/prebuilt-dry");
            std::fs::create_dir_all(&base).unwrap();
            let dir = tempfile::Builder::new().prefix("pd").tempdir_in(&base).unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root.join(".moss/identity")).unwrap();
            std::fs::write(root.join(".moss/state.toml"), "[deployment]\nsite_id = \"pd-test\"\n").unwrap();
            std::fs::write(root.join(".moss/identity/secret-key"), "k").unwrap();
            std::fs::write(root.join(".moss/identity/public.json"), "{}").unwrap();
            for (path, body) in files {
                let full = root.join("_site").join(path);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, body).unwrap();
            }
            if let Some(served) = served {
                published_record::save(
                    &MossPaths::new(root),
                    &PublishedSnapshot {
                        generation_id: "ours".into(),
                        target: "moss:pd-test".into(),
                        published_at: "2026-09-20T09:12:30+00:00".into(),
                        files: served.iter().map(|(p, b)| (p.to_string(), entry(b))).collect(),
                        asset_source_to_output: Some(Default::default()),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
            dir
        }

        async fn run(dir: &tempfile::TempDir, accept: bool) -> DryRun {
            let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var("MOSS_SETA_URL", "http://127.0.0.1:9");
            run_prebuilt_dry_run(dir.path(), &dir.path().join("_site"), None, accept).await.unwrap()
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn an_address_the_folder_no_longer_holds_is_listed_and_refused() {
            let dir = folder(&[("index.html", "a")], Some(&[("index.html", "a"), ("old/index.html", "o")]));
            let dry = run(&dir, false).await;
            let Transfer::SinceLastPublish { removed, .. } = &dry.transfer else { panic!("{dry:?}") };
            assert_eq!(removed, &vec!["old/index.html".to_string()]);
            assert!(dry.refusal.as_ref().map(|r| r.text.as_str()).is_some_and(|t| t.contains("/old/")), "{dry:?}");
            let text = dry.render();
            assert!(text.contains("nothing was built"), "{text}");
            assert!(text.contains("\nRemoved:\n  /old/\n") && !text.contains("Also removed"), "{text}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn accepting_removals_lifts_the_refusal_but_still_lists_them() {
            let dir = folder(&[("index.html", "a")], Some(&[("index.html", "a"), ("old/index.html", "o")]));
            let dry = run(&dir, true).await;
            assert!(dry.refusal.is_none(), "{dry:?}");
            assert!(matches!(&dry.transfer, Transfer::SinceLastPublish { removed, .. } if removed.len() == 1), "{dry:?}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_changed_and_a_new_file_upload_and_an_unchanged_one_does_not() {
            let dir = folder(
                &[("same.css", "s"), ("changed.css", "new"), ("added.css", "n")],
                Some(&[("same.css", "s"), ("changed.css", "old")]),
            );
            let dry = run(&dir, false).await;
            assert!(matches!(&dry.transfer, Transfer::SinceLastPublish { upload: 2, .. }), "{dry:?}");
            assert!(dry.refusal.is_none(), "{dry:?}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn no_record_is_not_counted_against_anything() {
            let dir = folder(&[("index.html", "a")], None);
            let dry = run(&dir, false).await;
            assert!(matches!(dry.transfer, Transfer::NoRecord), "{dry:?}");
        }

        fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
            let mut all: Vec<_> = walkdir::WalkDir::new(root.join(".moss"))
                .into_iter()
                .flatten()
                .map(|e| (e.path().to_path_buf(), e.metadata().unwrap().modified().unwrap()))
                .collect();
            all.sort();
            all
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn nothing_is_written_under_the_moss_directory() {
            let dir = folder(&[("index.html", "a")], Some(&[("old/index.html", "o")]));
            let before = snapshot(dir.path());
            run(&dir, false).await;
            assert_eq!(snapshot(dir.path()), before);
        }
    }
}
