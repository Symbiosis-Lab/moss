//! A tiny stand-in deriver and the declaration that points at it.

use std::path::{Path, PathBuf};

use crate::plugins::contributions::derivation::DerivationContribution;
use crate::system::derive::registry::DerivationEntry;

/// Write an executable `sh` script that has read the request into `$REQ`.
pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    // allow:raw_write a fixture script in a test's temp dir, not build output
    std::fs::write(&path, format!("#!/bin/sh\nREQ=$(cat)\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub const DERIVED: &str = r#"printf '%s' '{"kind":"derived","fragments":{"text":"hello"},"facts":{"title":"T","extra":1}}'"#;

pub fn entry(binary: &Path, version: &str) -> DerivationEntry {
    let contribution: DerivationContribution = serde_json::from_value(serde_json::json!({
        "accepts": ["xyz"],
        "noun": {"one": "doc", "other": "docs"},
        "fragments": {"text": "markdown"},
        "stub": ["#text"],
        "run": {"binary": "unpack", "artifact": [{"kind": "path", "binary": binary}]}
    }))
    .unwrap();
    DerivationEntry {
        plugin_id: "docs".into(),
        derivation_id: "pages".into(),
        plugin_version: version.into(),
        plugin_dir: PathBuf::new(),
        contribution,
    }
}
