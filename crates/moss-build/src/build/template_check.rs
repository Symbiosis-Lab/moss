//! Build problems in the site's templates: a template whose folder is missing,
//! a language version for a language the site has no tree for, and template
//! files the store could not list. Each is a problem `--strict` counts, because
//! a page made from such a template would land nowhere useful, or a language
//! version would never be used.

use crate::build::cli_output::log_warn_problem;
use crate::templates::{list_templates, site_languages};
use moss_core::template::{SiteLanguages, Template};
use std::path::Path;

/// The problem lines for a site's templates, given the listing's own problems
/// and whether a folder (site-relative, `/`-terminated) exists.
pub(crate) fn problems(
    templates: &[Template],
    listed_problems: &[String],
    langs: &SiteLanguages,
    folder_exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut out = listed_problems.to_vec();
    for t in templates {
        if let Some(folder) = t.folder.as_deref().filter(|f| !folder_exists(f)) {
            out.push(format!(
                "Template '{}' ({}.md) names the folder {folder}, which does not exist. Pages made from it have nowhere to go: create the folder, or clear the template's folder.",
                t.name, t.id
            ));
        }
        for file in &t.files {
            let Some(lang) = file.lang.as_deref() else { continue };
            if lang != langs.default_lang && !langs.trees.iter().any(|tree| tree == lang) {
                out.push(format!(
                    "Template '{}' has a {lang} version ({}.{lang}.md), but the site has no {lang} folder, so that version is never used. Create the {lang} folder, or remove the file.",
                    t.name, t.id
                ));
            }
        }
    }
    out
}

/// Reports the site's template problems as build problems.
pub(crate) fn warn_template_problems(site_root: &Path) {
    let listing = match list_templates(site_root) {
        Ok(listing) => listing,
        Err(e) => {
            log_warn_problem!("Could not check the site's templates: {e}");
            return;
        }
    };
    if listing.templates.is_empty() && listing.problems.is_empty() {
        return;
    }
    let langs = match site_languages(site_root) {
        Ok(langs) => langs,
        Err(e) => {
            log_warn_problem!("Could not check the site's templates: {e}");
            return;
        }
    };
    for line in problems(&listing.templates, &listing.problems, &langs, |f| site_root.join(f).is_dir()) {
        log_warn_problem!("{line}");
    }
}
