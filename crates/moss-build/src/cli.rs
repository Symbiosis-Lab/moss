//! The open-CLI features that ride the build itself: agent-guidance sync
//! (`agents/`), called mid-conductor by `build::run_pipeline`, and the
//! terminal verbs both binaries answer.
pub mod agents;
// `describe` prints the contract: tokens, slots, plugin hooks, the command
// table. Every input was already here or in moss-core; only the binary's
// version is not, so `run` takes it from the caller.
pub mod describe;
// The command table + `<cmd> --help`, read by both binaries.
pub mod commands;
// `doctor` and `list` read a vault and print — no window, no app state, no
// network. Both already called nothing but moss-build (`vault_root`,
// `build::emit::inventory`, `build::scan::classify`) through the app crate's
// re-export shims, so crossing them resolves the same items by a shorter path.
pub mod doctor;
pub mod list;
// `map bbox` reads the folder's config, gazetteer and page frontmatter and prints a frame.
pub mod map;
// `env` reads and writes the environment through `vault::config`, the one
// config writer both binaries share.
pub mod env;
pub mod tools;
// `rename` is the editor's rename-with-refs from the terminal; its whole body
// is `editor::ref_scan`, which crossed with it.
pub mod rename;
// `moss new` plans in moss_core::template and writes through crate::templates;
// it sits beside `rename`, the other command that reads the site from the cwd.
pub mod new;
pub mod render;
// Read-only project ownership lookup for hosts opening an arbitrary path.
pub mod root;
// `guide` reads nothing but the binary — no project, no network, no window —
// and its whole body was already `agents::skill_package`, so it belongs beside
// what it prints rather than one crate up behind a shim.
pub mod guide;
// The nested-site decision table + CLI refusal shapes, shared by both hosts
// (open-CLI slice 3). The GUI surfaces stay app-side.
pub mod site_guard;
// `domain list` / `domain link` talk to seta with the vault's own identity.
// The client crossed into this crate, and every other door this file opens
// (VaultRoot, site_config, Identity::load_for_signing) was already here.
pub mod domain;
pub mod site;
// Argument parsing for `moss import`, over the engine in
// `vault::import::scrape::run`.
pub mod import;
// `moss import <folder>` / `--status`: show a folder's files as pages and report
// where each folder stands, over `vault::derive_*`.
pub mod import_folder;
// `moss expand`: write embeds out as text.
pub mod expand;
// `moss deploy --prebuilt` over the shared driver in `deploy::prebuilt`; the
// app's own deploy mode calls the same one with a Tauri sink instead of a
// printing one (track C4c).
pub mod deploy;
// The `HostPorts` a terminal build runs with, moved from `moss-cli` at C4f so
// that `deploy`'s build route — which is in this crate — can reach it.
pub mod host;

// The hosting-service verbs (`domain`, `site`) share how they sign in as the
// folder and how they print a result.
use std::path::Path;

use crate::build::site_config::resolve_environment;
use crate::identity::keypair::Identity;
use crate::seta::client::MossSetaClient;

/// Build a MossSetaClient for `folder`, loading the project's identity.
pub(crate) fn client_for_folder(folder: &Path) -> Result<MossSetaClient, String> {
    if !folder.exists() {
        return Err(format!("folder does not exist: {}", folder.display()));
    }
    // Loads the signing key now, so a missing key fails here rather than at
    // the first signed request. It never regenerates one: a new pubkey would
    // make seta stop recognizing this account as the owner of the site.
    let identity = Identity::load_for_signing(folder).map_err(|e| {
        format!(
            "signing key unavailable: {} (expected identity at {}/.moss/identity/secret-key)",
            e,
            folder.display()
        )
    })?;
    let env = resolve_environment(&folder.to_string_lossy());
    Ok(MossSetaClient::for_environment(&identity, &env))
}

pub(crate) fn run_async<F: std::future::Future<Output = i32>>(fut: F) -> i32 {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(fut),
        Err(e) => {
            eprintln!("failed to start tokio runtime: {}", e);
            2
        }
    }
}

/// What a command prints and how it exits, so the flows are testable without
/// capturing the process's own streams.
pub(crate) struct Outcome {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Outcome {
    /// With `--json` an error is an object on stdout, like every other result.
    pub(crate) fn fail(json: bool, code: i32, msg: impl std::fmt::Display) -> Self {
        if json {
            let v = serde_json::json!({ "error": msg.to_string(), "exit": code });
            Outcome { code, out: format!("{}\n", v), err: String::new() }
        } else {
            Outcome { code, out: String::new(), err: format!("error: {}\n", msg) }
        }
    }
}

pub(crate) fn emit(o: Outcome) -> i32 {
    print!("{}", o.out);
    eprint!("{}", o.err);
    o.code
}
