//! CLI subcommand: `moss domain [link|status|list]`
//!
//! Exposes the same seta endpoints the Settings UI will use, authenticated with
//! the project's own identity (`.moss/identity/public.json` + signing key).
//! No admin key. Lets us fix stray manual DB inserts (e.g. `mosspub.com`) via
//! the same codepath users will take.
//!
//! Subcommands:
//!   moss domain list <folder>            List owned + assigned domains.
//!   moss domain link <folder> <domain>   Link a domain to the project's site and
//!                                        print the DNS records to set.
//!   moss domain status <folder> <domain> Read live DNS and certificate state.
//!   All three take `--json` for a stable object.
//!
//! Exit codes:
//!   0  success (`status`: the domain is live)
//!   1  usage / precondition failure (bad folder, no identity, no deployment)
//!   2  seta error (network, 4xx, 5xx)
//!   3  `status`: not live yet, waiting for DNS or the certificate
//!   4  `status`: failed, the hosting service gave up and the link must be redone

use std::path::Path;

use crate::build::site_config::{resolve_environment, site_id_for_folder};
use crate::identity::keypair::Identity;
use crate::seta::cdn::{CdnStatus, DomainCdn};
use crate::seta::client::MossSetaClient;
use crate::vault_root::VaultRoot;

mod records;

/// Entry point for `moss domain`. `args` is everything AFTER the verb.
///
/// Returns the process exit code. The caller (main.rs) calls `std::process::exit`
/// so that the async runtime and any background tasks don't keep the process
/// alive.
pub fn run(args: &[String]) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_help();
        return 1;
    };
    let usage = |text: &str| emit(Outcome::fail(json, 1, format!("usage: moss domain {} [--json]", text)));

    match subcommand {
        "list" => match args.get(1) {
            Some(folder) => run_async(cmd_list(folder, json)),
            None => usage("list <folder>"),
        },
        "link" | "status" => {
            let (Some(folder), Some(domain)) = (args.get(1), args.get(2)) else {
                return usage(&format!("{} <folder> <domain>", subcommand));
            };
            if subcommand == "link" {
                run_async(cmd_link(folder, domain, json))
            } else {
                run_async(cmd_status(folder, domain, json))
            }
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
    moss domain link <folder> <domain> [--json]
    moss domain status <folder> <domain> [--json]

DESCRIPTION:
    Authenticates with the project's identity (.moss/identity) and talks to
    the hosting service. Same codepath the Settings UI will use.

    link     Links <domain> (and www.<domain>) to the site. For a domain bought
             elsewhere it prints the DNS records to set at your registrar; the
             bare domain redirects to www. For a domain bought through moss,
             moss manages DNS and there is nothing to set.
    status   Looks up the records live, reports the certificate, and says
             whether the domain is live or what it is waiting for.
    --json   Print one stable JSON object instead of text; an error is
             {{\"error\": \"...\", \"exit\": N}} on stdout.

    Only a registered domain of two labels (example.com, or www.example.com
    for the same site) is supported; a subdomain such as blog.example.com is
    refused.

EXIT CODES:
    0  success (status: the domain is live)
    1  usage or precondition failure (bad folder, missing identity, no site_id)
    2  hosting service error (network, 4xx, 5xx) — retryable
    3  status: not live yet, waiting for DNS or the certificate
    4  status: failed, run `moss domain link` again

EXAMPLES:
    moss domain list ~/Sites/landing
    moss domain link ~/Sites/landing example.com
    moss domain status ~/Sites/landing example.com --json
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

/// What a command prints and how it exits, so the flows are testable without
/// capturing the process's own streams.
pub(crate) struct Outcome {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Outcome {
    /// With `--json` an error is an object on stdout, like every other result.
    fn fail(json: bool, code: i32, msg: impl std::fmt::Display) -> Self {
        if json {
            let v = serde_json::json!({ "error": msg.to_string(), "exit": code });
            Outcome { code, out: format!("{}\n", v), err: String::new() }
        } else {
            Outcome { code, out: String::new(), err: format!("error: {}\n", msg) }
        }
    }
}

fn emit(o: Outcome) -> i32 {
    print!("{}", o.out);
    eprint!("{}", o.err);
    o.code
}

const RERUN: &str = "Re-running the same command is safe: it reuses what already exists.";

/// Whether moss or the owner runs a domain's DNS decides what is printed and
/// which host visitors see.
struct Hosting {
    apex: String,
    managed: bool,
}

impl Hosting {
    fn source(&self) -> &'static str {
        if self.managed { "moss" } else { "external" }
    }

    fn www(&self) -> String {
        format!("www.{}", self.apex)
    }

    /// Moss-run domains are served at the apex; owner-run ones redirect the
    /// apex to www.
    fn canonical(&self) -> String {
        if self.managed { self.apex.clone() } else { self.www() }
    }

    async fn of(client: &MossSetaClient, apex: &str) -> Result<Option<Self>, crate::seta::client::SetaError> {
        let owned = client.get_assignable_domains().await?;
        Ok(owned
            .domains
            .iter()
            .find(|d| d.host == apex)
            .map(|d| Hosting { apex: apex.to_string(), managed: d.source == "moss" }))
    }
}

async fn cmd_list(folder: &str, json: bool) -> i32 {
    let root = VaultRoot::resolve(folder);
    let folder_path = root.path();
    let client = match client_for_folder(folder_path) {
        Ok(c) => c,
        Err(e) => return emit(Outcome::fail(json, 1, e)),
    };
    // Best-effort: read site_id so we can mark which row is "this folder".
    let current_site = site_id_for_folder(folder_path).ok();
    emit(list_flow(&client, current_site.as_deref(), json).await)
}

async fn list_flow(client: &MossSetaClient, current_site: Option<&str>, json: bool) -> Outcome {
    let response = match client.get_assignable_domains().await {
        Ok(r) => r,
        Err(e) => return Outcome::fail(json, 2, e),
    };
    let this_site = |d: &crate::seta::domains::AssignableDomain| {
        current_site.is_some_and(|s| d.assigned_to.as_ref().is_some_and(|r| r.id == s))
    };
    if json {
        let rows: Vec<_> = response
            .domains
            .iter()
            .map(|d| serde_json::json!({
                "host": d.host,
                "source": d.source,
                "assigned_to": d.assigned_to.as_ref().map(|r| r.id.as_str()),
                "this_site": this_site(d),
            }))
            .collect();
        return Outcome { code: 0, out: format!("{}\n", serde_json::json!({ "domains": rows })), err: String::new() };
    }
    if response.domains.is_empty() {
        return Outcome { code: 0, out: "(no domains in your account)\n".into(), err: String::new() };
    }
    // Tab separators keep the output grep-friendly; ASCII `-` marks "empty".
    let mut out = String::from("HOST\tSOURCE\tASSIGNED_TO\tTHIS_SITE\n");
    for d in &response.domains {
        let assigned = d.assigned_to.as_ref().map(|r| r.id.as_str()).unwrap_or("-");
        out.push_str(&format!("{}\t{}\t{}\t{}\n", d.host, d.source, assigned, if this_site(d) { "*" } else { "" }));
    }
    Outcome { code: 0, out, err: String::new() }
}

async fn cmd_link(folder: &str, domain: &str, json: bool) -> i32 {
    let root = VaultRoot::resolve(folder);
    let folder_path = root.path();
    let client = match client_for_folder(folder_path) {
        Ok(c) => c,
        Err(e) => return emit(Outcome::fail(json, 1, e)),
    };
    let site_id = match site_id_for_folder(folder_path) {
        Ok(s) => s,
        Err(e) => return emit(Outcome::fail(json, 1, e)),
    };
    let target = LinkTarget { client: &client, config_folder: &folder_path.to_string_lossy(), shown_folder: folder, site_id: &site_id };
    emit(link_flow(&target, domain, json).await)
}

async fn cmd_status(folder: &str, domain: &str, json: bool) -> i32 {
    let root = VaultRoot::resolve(folder);
    let client = match client_for_folder(root.path()) {
        Ok(c) => c,
        Err(e) => return emit(Outcome::fail(json, 1, e)),
    };
    emit(status_flow(&client, folder, domain, json, &records::SystemLookup).await)
}

/// Where a link goes and how the folder is named back to the user.
pub(crate) struct LinkTarget<'a> {
    pub client: &'a MossSetaClient,
    /// The resolved folder, for reading and writing its config.
    pub config_folder: &'a str,
    /// The folder as the user typed it, for the next command we print.
    pub shown_folder: &'a str,
    pub site_id: &'a str,
}

/// Link the apex, learn whether moss or the owner runs its DNS, and for an
/// owner-run domain start edge acceleration once and link `www.` too: the
/// origin redirects a bare domain to www only when a `www.` link exists, and
/// serves the edge's requests for www only under that same link. Every step
/// is idempotent, so a failed run is repaired by running it again.
pub(crate) async fn link_flow(t: &LinkTarget<'_>, input: &str, json: bool) -> Outcome {
    let apex = match records::normalize_domain(input) {
        Ok(d) => d,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let fail = |code: i32, e: &dyn std::fmt::Display| Outcome::fail(json, code, format!("{} {}", e, RERUN));
    let (client, folder) = (t.client, t.config_folder);

    if let Err(e) = client.link_custom_domain(t.site_id, &apex).await {
        return fail(2, &e);
    }
    // `resolve_site_url` falls back to `[site].domain`; without this write a
    // linked site keeps resolving og:url/canonical/sitemap to its subdomain.
    if let Err(e) = crate::vault::config::set_domain_in_config(folder, &apex) {
        return fail(2, &format!("domain linked, but failed to record it locally: {}", e));
    }
    let host = match Hosting::of(client, &apex).await {
        Ok(Some(h)) => h,
        Ok(None) => return fail(2, &"the hosting service linked the domain but does not list it"),
        Err(e) => return fail(2, &e),
    };

    let mut linked = vec![apex.clone()];
    let mut cdn: Option<DomainCdn> = None;
    if host.managed {
        crate::vault::deployment_state::observe_and_record_cdn(client, &apex, folder).await;
    } else {
        // Starting acceleration spends a quota shared by every user and mints
        // fresh validation values, so a re-run reuses what is already there.
        let current = match client.get_domain_cdn(&apex).await {
            Ok(c) => c,
            Err(e) => return fail(2, &e),
        };
        let started = if current.records.is_empty() || matches!(current.cdn_status, CdnStatus::Disabled | CdnStatus::Failed) {
            match client.start_domain_cdn(&apex).await {
                Ok(c) => c,
                Err(e) => return fail(2, &e),
            }
        } else {
            current
        };
        if let Err(e) = client.link_custom_domain(t.site_id, &host.www()).await {
            return fail(2, &e);
        }
        linked.push(host.www());
        if let Some(word) = started.cdn_status.wire_word() {
            let _ = crate::vault::deployment_state::record_cdn_status(folder, word);
        }
        cdn = Some(started);
    }
    let rows = cdn.as_ref().map(|c| records::required_records(&c.records)).unwrap_or_default();
    let (apex, site_id, www, canonical) = (&host.apex, t.site_id, host.www(), host.canonical());
    let next = format!("moss domain status {} {}", t.shown_folder, apex);

    let out = if json {
        let v = serde_json::json!({
            "domain": apex,
            "site_id": site_id,
            "source": host.source(),
            "canonical_host": canonical,
            "linked": linked,
            "cdn_status": cdn.as_ref().and_then(|c| c.cdn_status.wire_word()),
            "records": rows.iter().map(|r| serde_json::json!({
                "type": r.record_type, "name": r.name, "value": r.value,
            })).collect::<Vec<_>>(),
            "next": next,
        });
        format!("{}\n", v)
    } else if host.managed {
        format!(
            "Linked {apex} to site {site_id}.\n\nThis domain was bought through moss, so moss manages its DNS. There is nothing to add.\n"
        )
    } else {
        format!(
            "Linked {apex} and {www} to site {site_id}.\n\nAt the registrar where {apex} is managed, add these records:\n\n{table}\n\
             Visitors see {www}; {apex} redirects to it.\n\
             Leave the MX records and any other TXT records alone: email keeps working.\n\
             Records can take up to an hour to show. Run `{next}` to see what is left.\n",
            table = records::render_records(&rows),
        )
    };
    Outcome { code: 0, out, err: String::new() }
}

pub(crate) async fn status_flow(
    client: &MossSetaClient,
    folder: &str,
    input: &str,
    json: bool,
    lookup: &dyn records::Lookup,
) -> Outcome {
    let apex = match records::normalize_domain(input) {
        Ok(d) => d,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let fail = |e: crate::seta::client::SetaError| Outcome::fail(json, 2, e);
    let host = match Hosting::of(client, &apex).await {
        Ok(Some(h)) => h,
        Ok(None) => return Outcome::fail(json, 1, format!("{} is not linked to your account; run `moss domain link {} {}`", apex, folder, apex)),
        Err(e) => return fail(e),
    };
    let cdn = match client.get_domain_cdn(&apex).await {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let retry = format!("moss domain link {} {}", folder, apex);
    let (checks, verdict) = if host.managed {
        // Only a moss-run domain's verdict leans on the service's DNS probe.
        let dns_ok = match client.get_domain_status_for_host(&apex).await {
            Ok(s) => s.dns_ok,
            Err(e) => return fail(e),
        };
        (Vec::new(), records::managed_verdict(dns_ok, cdn.cdn_status, &retry))
    } else {
        let required = records::required_records(&cdn.records);
        let checks = records::check_records(&apex, &required, cdn.cdn_status, lookup);
        let verdict = records::verdict(&checks, cdn.records.len(), cdn.cdn_status, &retry);
        (checks, verdict)
    };

    let canonical = host.canonical();
    let out = if json {
        let v = serde_json::json!({
            "domain": apex,
            "source": host.source(),
            "canonical_host": canonical,
            "verdict": verdict.word(),
            "reason": match &verdict { records::Verdict::Failed(w) => Some(w.clone()), _ => None },
            "cdn_status": cdn.cdn_status.wire_word(),
            "certificate": cdn.cdn_status.certificate_phrase(),
            "records": checks,
        });
        format!("{}\n", v)
    } else {
        let mut o = format!("{apex} (visitors see {canonical})\n\n");
        if host.managed {
            o.push_str("moss manages this domain's DNS; there are no records to set.\n");
        } else {
            o.push_str(&records::render_checks(&checks));
        }
        o.push_str(&format!(
            "\nCertificate: {}\n{}\n",
            cdn.cdn_status.certificate_phrase(),
            records::verdict_line(&verdict, &checks)
        ));
        o
    };
    Outcome { code: verdict.exit_code(), out, err: String::new() }
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
            (&["status"], 1),
            (&["status", "/tmp/x"], 1),
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

#[cfg(test)]
#[path = "domain/flow_tests.rs"]
mod flow_tests;
