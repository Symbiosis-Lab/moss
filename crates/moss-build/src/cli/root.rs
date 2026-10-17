//! Read-only lookup of the moss site that owns a path.

use std::path::Path;

/// `moss root <path> --json` prints the canonical owning site or JSON null.
pub fn run(args: &[String]) -> i32 {
    let mut path = None;
    let mut json = false;
    for arg in args {
        if arg == "--json" && !json {
            json = true;
        } else if arg.starts_with('-') || path.is_some() {
            eprintln!("Usage: moss root <path> --json");
            return 2;
        } else {
            path = Some(arg);
        }
    }
    let Some(path) = path.filter(|_| json) else {
        eprintln!("Usage: moss root <path> --json");
        return 2;
    };

    let root = crate::vault_root::VaultRoot::find_containing(Path::new(path));
    let root = root.map(|root| root.as_str().to_owned());
    println!("{}", serde_json::json!({"root": root}));
    0
}
