//! `.moss-email/welcome.<edition>.json`: the welcome email an author writes as
//! `welcome-email.md` at the root of a language tree.
//!
//! The file is parsed but is never a page (see `footer::is_excluded_from_pages`).
//! The hosting server reads the JSON, renders `body` and sends it once to each
//! new subscriber of that edition, so the body ships as the author's markdown,
//! unrendered. A file without `subject:` cannot be sent, so it emits nothing.

use std::path::Path;

use crate::build::context::BuildContext;
use crate::build::footer::{language_tree_split, WELCOME_EMAIL_FILE};
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;

/// Edition name for the root language tree.
const DEFAULT_EDITION: &str = "default";

/// Emit one JSON file per `welcome-email.md` in `documents`.
pub fn emit(
    documents: &[ParsedDocument],
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    for doc in documents {
        let path = doc.source_path.as_deref().unwrap_or("");
        let (tree, within_tree) = language_tree_split(path);
        if within_tree != WELCOME_EMAIL_FILE {
            continue;
        }
        let field = |key: &str| {
            doc.raw_frontmatter
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let Some(subject) = field("subject") else {
            if doc.raw_frontmatter.get("subject").is_some_and(|v| !v.is_string()) {
                log::warn!("'{path}': `subject:` must be text, so no welcome email is sent for this edition");
            } else {
                log::warn!("'{path}' has no `subject:` in its frontmatter, so no welcome email is sent for this edition");
            }
            continue;
        };
        let json = serde_json::json!({
            "subject": subject,
            "preview": field("preview"),
            "from": field("from"),
            "reply_to": field("reply_to"),
            "body": doc.content,
        });
        let served = ServedPath::for_welcome_email(tree.as_deref().unwrap_or(DEFAULT_EDITION))
            .map_err(|e| format!("welcome email '{path}': {e:?}"))?;
        BuildContext::for_render(output_dir, pending)
            .emit_held(&served, json.to_string().into_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to emit {}: {}", served.as_str(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::build::{HostPorts, PluginMode};
    use crate::types::services::BuildServices;

    #[tokio::test(flavor = "multi_thread")]
    async fn the_template_is_in_the_manifest_a_deploy_uploads() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let write = |rel: &str, body: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };
        write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
        write(".moss/state.toml", "[deployment]\nsite_id = \"harbor-notes\"\ndeploy_method = \"moss\"\n");
        write("index.md", "---\ntitle: Harbor Notes\n---\n\nHello.\n");
        write("welcome-email.md", "---\nsubject: Welcome aboard\n---\n\nHi {{email}}.\n");

        let host = HostPorts { services: BuildServices::headless(), ..crate::build::ports::host::test_host_ports() };
        let sealed = crate::deploy::one_shot::build_and_seal(
            &crate::vault::paths::VaultRoot::resolve(&root),
            host,
            PluginMode::Skip,
        )
        .await
        .unwrap()
        .expect("the build seals a generation");

        assert!(
            sealed.files().contains_key(".moss-email/welcome.default.json"),
            "{:?}",
            sealed.files().keys().collect::<Vec<_>>()
        );
    }
}
