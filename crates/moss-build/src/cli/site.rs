//! CLI subcommand: `moss site transfer` (usage in [`HELP`]). The request is
//! signed with the folder's key, which is the owner's authorisation.
//!
//! Exit codes:
//!   0  sent, cancelled or planned
//!   1  usage / precondition failure (bad folder, no identity, no site yet)
//!   2  the hosting service refused or could not be reached

use crate::build::site_config::site_id_for_folder;
use crate::cli::{client_for_folder, emit, run_async, Outcome};
use crate::identity::verify_email::validate_email;
use crate::seta::client::SetaError;
use crate::seta::transfer::TransferPlan;
use crate::vault_root::VaultRoot;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    To { email: String, keep_publishing: bool, dry_run: bool },
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
struct TransferArgs {
    folder: String,
    mode: Mode,
    json: bool,
}

const USAGE: &str = "usage: moss site transfer <folder> --to <email> [--keep-publishing] [--dry-run] [--json]
       moss site transfer <folder> --cancel [--json]";

const HELP: &str = "transfer  Hand the site to another email address. They get one link by
          email and accept from it, within 72 hours; until then nothing moves.
          The address, files, subscribers and history stay with the site.
          Once accepted, this folder's key can no longer publish it.
          --keep-publishing  let the receiver also take this folder's key, so
                             this folder keeps publishing the site as them
                             (refused if the key publishes other sites of yours)
          --dry-run          show what would move, send nothing
          --cancel           withdraw a pending transfer; its link stops working";

fn parse(args: &[String]) -> Result<TransferArgs, String> {
    let (mut folder, mut to, mut keep_publishing, mut dry_run, mut cancel, mut json) = (None, None, false, false, false, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--to" => to = Some(it.next().ok_or("error: --to needs an address")?.clone()),
            s if s.starts_with("--to=") => to = s.strip_prefix("--to=").map(str::to_string),
            "--keep-publishing" => keep_publishing = true,
            "--dry-run" => dry_run = true,
            "--cancel" => cancel = true,
            "--json" => json = true,
            s if s.starts_with("--") => return Err(format!("error: unknown flag {s}\n{USAGE}")),
            s if folder.is_none() => folder = Some(s.to_string()),
            s => return Err(format!("error: unexpected argument {s}\n{USAGE}")),
        }
    }
    let folder = folder.ok_or(USAGE)?;
    let mode = match (to, cancel) {
        (Some(email), false) if validate_email(&email).is_err() => return Err(format!("error: '{email}' is not an email address")),
        (Some(email), false) => Mode::To { email, keep_publishing, dry_run },
        (None, true) if !keep_publishing && !dry_run => Mode::Cancel,
        _ => return Err(format!("error: give either --to <email> or --cancel\n{USAGE}")),
    };
    Ok(TransferArgs { folder, mode, json })
}

/// Entry point for `moss site`. `args` is everything after the verb.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("transfer") => match parse(&args[1..]) {
            Ok(a) => run_async(transfer(a)),
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("--help" | "-h" | "help") => {
            println!("moss site — manage a project's mosspub.com site\n\n{USAGE}\n\n{HELP}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            1
        }
    }
}

async fn transfer(a: TransferArgs) -> i32 {
    let root = VaultRoot::resolve(&a.folder);
    let folder = root.path();
    let client = match client_for_folder(folder) {
        Ok(c) => c,
        Err(e) => return emit(Outcome::fail(a.json, 1, e)),
    };
    let site_id = match site_id_for_folder(folder) {
        Ok(s) => s,
        Err(e) => return emit(Outcome::fail(a.json, 1, e)),
    };
    let Mode::To { email: to, keep_publishing, dry_run } = a.mode else {
        return emit(match client.cancel_transfer(&site_id).await {
            Ok(()) => ok(a.json, serde_json::json!({ "status": "cancelled", "site_id": site_id }), format!("Cancelled the transfer of {site_id}; its link no longer works.\n")),
            Err(e) => refused(a.json, e),
        });
    };

    let response = match client.request_transfer(&site_id, &to, dry_run, keep_publishing).await {
        Ok(r) => r,
        Err(e) => return emit(refused(a.json, e)),
    };
    let plan = describe(&response.plan);
    if dry_run {
        return emit(ok(a.json, serde_json::json!({ "dry_run": true, "plan": response.plan }), format!("{plan}Dry run: nothing sent.\n")));
    }
    let after = if keep_publishing {
        format!("If they also let this folder keep publishing, its key becomes {to}'s and it publishes the site as them.\n")
    } else {
        "Once accepted, this folder can no longer publish the site.\n".to_string()
    };
    emit(ok(
        a.json,
        serde_json::json!({ "status": "sent", "plan": response.plan, "expires_at": response.expires_at, "keep_publishing": keep_publishing }),
        format!("{plan}Sent one link to {to}. It works for 72 hours; nothing moves until it is accepted.\n{after}"),
    ))
}

fn describe(plan: &TransferPlan) -> String {
    let domains = if plan.domains.is_empty() { "none".to_string() } else { plan.domains.join(", ") };
    let subscription = plan.subscription.as_ref().map_or("none".to_string(), |s| format!("{} ({}), stays with the site", s.plan, s.origin));
    format!(
        "site          {}\nfrom          {}\nto            {}\ndomains moved {}\nsubscription  {}\n",
        plan.site_id, plan.from, plan.to, domains, subscription
    )
}

fn ok(json: bool, value: serde_json::Value, text: String) -> Outcome {
    if json {
        Outcome { code: 0, out: format!("{value}\n"), err: String::new() }
    } else {
        Outcome { code: 0, out: text, err: String::new() }
    }
}

/// The service's refusal in words; its code is kept in `--json`.
fn refused(json: bool, e: SetaError) -> Outcome {
    let (code, sentence) = match &e {
        SetaError::Api { message, .. } => {
            let (code, detail) = message.split_once(": ").unwrap_or((message.as_str(), ""));
            (code.to_string(), refusal_sentence(code, detail))
        }
        other => (String::new(), other.to_string()),
    };
    if json {
        let v = serde_json::json!({ "error": sentence, "code": code, "exit": 2 });
        return Outcome { code: 2, out: format!("{v}\n"), err: String::new() };
    }
    Outcome { code: 2, out: String::new(), err: format!("error: {sentence}\n") }
}

fn refusal_sentence(code: &str, detail: &str) -> String {
    match code {
        "already_owner" => "that address already owns this site".into(),
        "stripe_billed" => "this site is billed through Stripe; its billing has to move first".into(),
        "purchased_domain_linked" => "a domain bought through moss is linked to this site; its registration has to move first".into(),
        "site_limit_reached" => "the receiving account is at its site limit".into(),
        "key_shared" => detail.to_string(),
        "no_pending_transfer" => "no transfer of this site is waiting".into(),
        "email_rate_limited" => "too many links were sent to that address; try again later".into(),
        "Not authorized for this site" => "this folder's key does not own the site".into(),
        other if detail.is_empty() => format!("the hosting service refused the transfer: {other}"),
        other => format!("the hosting service refused the transfer: {other}: {detail}"),
    }
}

#[cfg(test)]
#[path = "site_tests.rs"]
mod site_tests;
