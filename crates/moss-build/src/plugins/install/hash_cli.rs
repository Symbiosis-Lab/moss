//! `moss plugin hash <plugin-dir> [--json]` — the code hash the loader keys
//! consent on, for a directory that is not part of any site.
//!
//! The registry pins this value per plugin version, so its scripts ask the
//! pinned moss for it instead of keeping a second implementation of the hash.

use std::path::Path;

const USAGE: &str = "Usage: moss plugin hash <plugin-dir> [--json]\n       moss plugin install <id> [<folder>]";

/// The `moss plugin` verbs: `hash` here, `install` in [`super::install_cli`].
pub fn run(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("install") {
        return super::install_cli::run(args);
    }
    let mut dir = None;
    let mut json = false;
    let mut words = args.iter();
    if words.next().map(String::as_str) != Some("hash") {
        eprintln!("{USAGE}");
        return 2;
    }
    for arg in words {
        if arg == "--json" && !json {
            json = true;
        } else if arg.starts_with('-') || dir.is_some() {
            eprintln!("{USAGE}");
            return 2;
        } else {
            dir = Some(arg);
        }
    }
    let Some(dir) = dir else {
        eprintln!("{USAGE}");
        return 2;
    };
    match hash(Path::new(dir)) {
        Ok((id, version, hash)) if json => {
            println!("{}", serde_json::json!({"id": id, "version": version, "code_sha256": hash}));
            0
        }
        Ok((_, _, hash)) => {
            println!("{hash}");
            0
        }
        Err(message) => {
            eprintln!("moss plugin hash: {message}");
            1
        }
    }
}

fn hash(dir: &Path) -> Result<(String, String, String), String> {
    let manifest = crate::plugins::bundled::read_installed_manifest(dir)
        .ok_or_else(|| format!("{} has no readable manifest.json", dir.display()))?;
    let hash = crate::plugins::bundled::code_hash(dir, &manifest)
        .ok_or_else(|| format!("{} has no readable entry file", dir.display()))?;
    Ok((manifest.name, manifest.version, hash))
}
