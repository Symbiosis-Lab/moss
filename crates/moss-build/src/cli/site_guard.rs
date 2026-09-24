//! The nested-site decision table (rows 3–6) and its CLI refusal shapes.
//!
//! Detection lives in [`crate::nested_roots`], which never decides anything;
//! this module is the POLICY half both hosts share (crossed from the app's
//! nested-site guard at open-CLI slice 3).
//! The app's GUI guard consumes [`plan_unowned_open`] and renders a dialog;
//! [`guard_cli_open`] is the whole answer for a host with no surface to ask
//! on — every Confirm row becomes a refusal message naming the fix.

use crate::nested_roots::{
    find_nested_roots, owns_moss, NestedRootInfo, NestedRootsReport, RootClass, ScanLimits,
};
use std::path::Path;

/// The decision table's answer for an UNOWNED folder (no `.moss/` here or
/// above): proceed silently, or ask — and if asking, whether "make this
/// folder a site anyway" may be offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnownedPlan {
    /// Row 3: normal folder, nothing nested, scan complete.
    Proceed,
    /// Rows 4/5/6 and the truncated-Normal case: confirmation required.
    Confirm { offer_create_anyway: bool },
}

/// Decision-table rows 3–6, pure. `truncated` never degrades to "proceed":
/// only a CLEAN scan of a Normal folder with nothing nested builds silently.
///
/// "Create anyway" is offered ONLY when the folder is Normal-shaped AND
/// either nothing was found (the truncated-Normal escape) or the single
/// nested root is not published (`published != Some(true)` — orphaning a
/// published identity stays off the table, 2026-06-27 locked decision #4).
pub fn plan_unowned_open(report: &NestedRootsReport) -> UnownedPlan {
    let normal = report.root_class == RootClass::Normal;
    if report.nested.is_empty() && !report.truncated && normal {
        return UnownedPlan::Proceed;
    }
    let offer_create_anyway = normal
        && match report.nested.len() {
            0 => true, // truncated Normal: "anyway" IS the proceed path
            1 => report.nested[0].published != Some(true),
            _ => false,
        };
    UnownedPlan::Confirm { offer_create_anyway }
}

/// The site to offer first: shallowest, ties broken by relative path.
/// The report is BFS-ordered (shallowest-first) but sibling order is
/// filesystem order, so the tie-break keeps the choice deterministic.
pub fn pick_primary(sites: &[NestedRootInfo]) -> Option<&NestedRootInfo> {
    sites.iter().min_by(|a, b| {
        a.rel_path
            .split('/')
            .count()
            .cmp(&b.rel_path.split('/').count())
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    })
}

/// Human phrase for a suspicious root shape, used in dialog and CLI copy.
pub fn class_description(class: &RootClass) -> Option<String> {
    match class {
        RootClass::Normal => None,
        RootClass::CloudProviderRoot { provider } => Some(format!("your whole {provider} drive")),
        RootClass::HomeDir => Some("your home folder".to_string()),
        RootClass::SystemSpecial(name) => Some(format!("your {name} folder")),
        RootClass::FilesystemRoot => Some("the entire disk".to_string()),
    }
}

/// The one `ScanLimits` both guards scan with.
pub fn default_limits() -> ScanLimits {
    ScanLimits::default()
}

/// CLI guard. `Err(message)` refuses the open; the caller prints it and exits
/// non-zero. Refusals are parametrized by `subcommand` (`build` / `deploy`).
///
/// Three refusal shapes: the folder belongs to an existing vault (re-rooting
/// a script silently is worse than refusing), the folder contains nested
/// moss roots, or the folder is suspicious/uncheckable (provider root, home,
/// truncated scan). A folder that IS a root always proceeds.
pub fn guard_cli_open(folder_path: &str, subcommand: &str) -> Result<(), String> {
    let path = Path::new(folder_path);
    if owns_moss(path) {
        return Ok(());
    }
    if let Some(vault) = crate::vault::paths::VaultRoot::find_containing(path) {
        return Err(format!(
            "'{folder}' belongs to the site at '{root}'.\n       \
             Run moss on that folder instead:  moss {sub} \"{root}\"",
            folder = folder_path,
            root = vault.as_str(),
            sub = subcommand,
        ));
    }
    let home = dirs::home_dir();
    let report = find_nested_roots(path, home.as_deref(), &default_limits());
    match plan_unowned_open(&report) {
        UnownedPlan::Proceed => Ok(()),
        UnownedPlan::Confirm { .. } => Err(cli_refusal_message(&report, folder_path, subcommand)),
    }
}

/// The refusal text for each Confirm-shaped report. Pure, so every case is
/// testable without a filesystem.
pub fn cli_refusal_message(
    report: &NestedRootsReport,
    folder_path: &str,
    subcommand: &str,
) -> String {
    if let Some(primary) = pick_primary(&report.nested) {
        let id = primary
            .site_id
            .as_deref()
            .map(|id| format!(" (published site '{id}')"))
            .unwrap_or_default();
        let more = if report.nested.len() > 1 {
            format!(
                " and {} other moss site(s){}",
                report.nested.len() - 1,
                if report.truncated { ", possibly more" } else { "" }
            )
        } else if report.truncated {
            " (scan incomplete — there may be more)".to_string()
        } else {
            String::new()
        };
        return format!(
            "'{folder}' contains a moss site at {rel}{id}{more}.\n       \
             Run moss on that folder instead:  moss {sub} \"{folder}/{rel}\"",
            folder = folder_path,
            rel = primary.rel_path,
            sub = subcommand,
        );
    }
    if let Some(desc) = class_description(&report.root_class) {
        return format!(
            "'{folder}' looks like {desc} — moss will not turn it into a site.\n       \
             Run moss {sub} on the folder that holds your site instead.",
            folder = folder_path,
            sub = subcommand,
        );
    }
    // Truncated Normal, nothing found: the scan cannot vouch for the tree.
    format!(
        "'{folder}' is too large to check for nested moss sites ({visited} folders scanned).\n       \
         Run moss {sub} on the site folder itself.",
        folder = folder_path,
        visited = report.dirs_visited,
        sub = subcommand,
    )
}
