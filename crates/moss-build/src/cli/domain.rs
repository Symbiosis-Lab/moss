//! CLI subcommand: `moss domain [link|list]`
//!
//! Exposes the same seta endpoints the Settings UI will use, authenticated with
//! the project's own identity (`.moss/identity/public.json` + signing key).
//! No admin key. Lets us fix stray manual DB inserts (e.g. `mosspub.com`) via
//! the same codepath users will take.
//!
//! Subcommands:
//!   moss domain list <folder>            List owned + assigned domains.
//!   moss domain link <folder> <domain>   Link a domain to the project's site.
//!
//! Exit codes:
//!   0  success
//!   1  usage / precondition failure (bad folder, no identity, no deployment)
//!   2  seta error (network, 4xx, 5xx)

use std::path::Path;

use crate::build::site_config::{resolve_environment, site_id_for_folder};
use crate::identity::keypair::Identity;
use crate::seta::client::MossSetaClient;
use crate::vault_root::VaultRoot;

/// Entry point for `moss domain`. `args` is everything AFTER the verb.
///
/// Returns the process exit code. The caller (main.rs) calls `std::process::exit`
/// so that the async runtime and any background tasks don't keep the process
/// alive.
pub fn run(args: &[String]) -> i32 {
    let subcommand = match args.first() {
        Some(s) => s.as_str(),
        None => {
            print_help();
            return 1;
        }
    };

    match subcommand {
        "list" => {
            let folder = match args.get(1) {
                Some(f) => f,
                None => {
                    eprintln!("Usage: moss domain list <folder>");
                    return 1;
                }
            };
            run_async(cmd_list(folder))
        }
        "link" => {
            let folder = match args.get(1) {
                Some(f) => f,
                None => {
                    eprintln!("Usage: moss domain link <folder> <domain>");
                    return 1;
                }
            };
            let domain = match args.get(2) {
                Some(d) => d,
                None => {
                    eprintln!("Usage: moss domain link <folder> <domain>");
                    return 1;
                }
            };
            run_async(cmd_link(folder, domain))
        }
        "--help" | "-h" | "help" => {
            print_help();
            0
        }
        other => {
            eprintln!("Unknown domain subcommand: {}", other);
            print_help();
            1
        }
    }
}

fn print_help() {
    println!(
        "moss domain — manage custom domains for a project

USAGE:
    moss domain list <folder>
    moss domain link <folder> <domain>

DESCRIPTION:
    Authenticates with the project's identity (.moss/identity) and talks to
    moss-seta. Same codepath the Settings UI will use.

EXIT CODES:
    0  success
    1  usage or precondition failure (bad folder, missing identity, no site_id)
    2  seta error (network, 4xx, 5xx) — retryable

EXAMPLES:
    moss domain list ~/Sites/landing
    moss domain link ~/Sites/landing mosspub.com
"
    );
}

/// Build a MossSetaClient for `folder`, loading the project's identity.
fn client_for_folder(folder: &Path) -> Result<MossSetaClient, String> {
    if !folder.exists() {
        return Err(format!("folder does not exist: {}", folder.display()));
    }
    // load_for_signing forces the signing key into memory now, so later
    // sign_request_payload calls don't fail opaquely. It intentionally does NOT
    // regenerate on a missing key — a new pubkey would make seta stop
    // recognizing this account as the owner of the site.
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

fn run_async<F: std::future::Future<Output = i32>>(fut: F) -> i32 {
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

async fn cmd_list(folder: &str) -> i32 {
    // Was a raw `Path::new(folder)` — no resolution at all, so `moss domain list .`
    // read config from a folder path whose name was empty.
    let root = VaultRoot::resolve(folder);
    let folder_path = root.path();
    let client = match client_for_folder(folder_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    // Best-effort: read site_id so we can mark which row is "this folder".
    let current_site = site_id_for_folder(folder_path).ok();

    let response = match client.get_assignable_domains().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            return 2;
        }
    };

    if response.domains.is_empty() {
        println!("(no domains in your account)");
        return 0;
    }

    // Columns: HOST, SOURCE, ASSIGNED, FLAG-IF-THIS-SITE.
    // Using tab separators keeps the output grep-friendly without pulling in
    // a formatter dep.
    println!("HOST\tSOURCE\tASSIGNED_TO\tTHIS_SITE");
    for d in response.domains {
        // ASCII `-` rather than em-dash so downstream awk/cut pipelines
        // don't need to handle multi-byte UTF-8 in the "empty" column.
        let assigned = d
            .assigned_to
            .as_ref()
            .map(|r| r.id.as_str())
            .unwrap_or("-");
        let is_this = current_site
            .as_deref()
            .map(|s| d.assigned_to.as_ref().map_or(false, |r| r.id == s))
            .unwrap_or(false);
        let flag = if is_this { "*" } else { "" };
        println!("{}\t{}\t{}\t{}", d.host, d.source, assigned, flag);
    }
    0
}

async fn cmd_link(folder: &str, domain: &str) -> i32 {
    let root = VaultRoot::resolve(folder);
    let folder_path = root.path();
    let client = match client_for_folder(folder_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let site_id = match site_id_for_folder(folder_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    println!("Linking {} to site {}...", domain, site_id);
    if let Err(e) = client.link_custom_domain(&site_id, domain).await {
        eprintln!("error: {}", e);
        return 2;
    }

    // Record what we just told seta — the fix this command exists for.
    // `resolve_site_url` falls back to `[site].domain`; without this write a
    // CLI-linked site keeps resolving og:url/canonical/sitemap to
    // <site_id>.mosspub.com even though seta now thinks the domain belongs
    // to this site.
    let folder_str = folder_path.to_string_lossy();
    if let Err(e) = crate::vault::config::set_domain_in_config(&folder_str, domain) {
        eprintln!("error: domain linked, but failed to record it locally: {}", e);
        return 2;
    }

    // Best-effort: seed [deployment].observed.cdn_status so the build knows
    // whether to canonicalize to `www.` right away instead of waiting for
    // the next `get_domain_cdn`/`moss domain link` call. A seta outage here
    // must never fail a link that already succeeded — see
    // `observe_and_record_cdn`.
    crate::vault::deployment_state::observe_and_record_cdn(&client, domain, &folder_str).await;

    println!("ok.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exit code each argv shape answers with. `cli_binary_parity_test`
    /// proves the two binaries agree on these; this pins what they agree on.
    #[test]
    fn argv_shapes_map_to_exit_codes() {
        let cases: &[(&[&str], i32)] = &[
            (&[], 1),
            (&["--help"], 0),
            (&["help"], 0),
            (&["bogus"], 1),
            (&["list"], 1),
            (&["link"], 1),
            (&["link", "/tmp/x"], 1),
        ];
        for (args, want) in cases {
            let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            assert_eq!(run(&owned), *want, "argv {:?}", args);
        }
    }

    #[test]
    fn client_for_folder_rejects_nonexistent() {
        let missing = Path::new("/tmp/moss-cli-test-nonexistent-path-xxx");
        match client_for_folder(missing) {
            Ok(_) => panic!("expected error for nonexistent path"),
            Err(e) => assert!(e.contains("does not exist"), "got: {}", e),
        }
    }
}
