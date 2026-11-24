//! CLI subcommand: `moss provider [status|connect|disconnect]`, the site's
//! outside providers of its parts (the newsletter today), connected through
//! the hosting service with the folder's own signing key.
//!
//!   moss provider status <folder> [<part>]        Every part, or one: the provider named, whether
//!                                                 its plugin is installed, the service's state.
//!   moss provider connect <folder> <part> [--provider <id>] [--list <id> | --new-list <name>]
//!                                                 The key from the variable the part's credential
//!                                                 names, else stdin; never an argument.
//!   moss provider disconnect <folder> <part>
//!   All take `--json` for a stable object.
//!
//! Exit codes:
//!   0  success (`status`: every part that names a provider is connected)
//!   1  usage / precondition failure (bad folder, no key, a key of the wrong shape)
//!   2  `connect` without a list: the account's lists are printed, nothing is written
//!   3  the service or the hosting service refused, or `status`: a part is not connected

use std::io::{IsTerminal, Read};

use crate::build::site_config::{read_project_config, site_id_for_folder};
use crate::cli::{client_for_folder, emit, run_async, Outcome};
use crate::plugins::contributions::provider::{newsletter_provider, Credential, NewsletterPart, Part};
use crate::seta::client::MossSetaClient;
use crate::seta::newsletter::{ConnectAnswer, ConnectionState, ListPick, NewsletterRefusal, SetaNewsletter};
use crate::vault_root::VaultRoot;

const PARTS: &str = "newsletter";

fn part(name: &str) -> Result<Part, String> {
    match name {
        "newsletter" => Ok(Part::Newsletter),
        other => Err(format!("`{other}` is not a part a provider serves; the parts are: {PARTS}")),
    }
}

struct Connect {
    provider: Option<String>,
    list: Option<ListPick>,
}

fn parse_connect(flags: &[String]) -> Result<Connect, String> {
    let mut c = Connect { provider: None, list: None };
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{flag} takes a value"));
        match flag.as_str() {
            "--provider" => c.provider = Some(value()?),
            "--list" if c.list.is_none() => c.list = Some(ListPick::Id(value()?)),
            "--new-list" if c.list.is_none() => c.list = Some(ListPick::New { new: value()? }),
            "--list" | "--new-list" => return Err("pass one of --list and --new-list".into()),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(c)
}

/// Entry point for `moss provider`. `args` is everything after the verb.
pub fn run(args: &[String]) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let usage = |text: &str| emit(Outcome::fail(json, 1, format!("usage: moss provider {text} [--json]")));
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("status"), Some(folder)) => match args.get(2).map(|p| part(p)).transpose() {
            Ok(_) => run_async(cmd_status(folder, json)),
            Err(e) => emit(Outcome::fail(json, 1, e)),
        },
        (Some(verb @ ("connect" | "disconnect")), Some(folder)) => {
            let Some(named) = args.get(2) else { return usage(&format!("{verb} <folder> <part>")) };
            if let Err(e) = part(named) {
                return emit(Outcome::fail(json, 1, e));
            }
            if verb == "disconnect" {
                return run_async(cmd_disconnect(folder, json));
            }
            match parse_connect(&args[3..]) {
                Ok(c) => run_async(cmd_connect(folder, c, json)),
                Err(e) => emit(Outcome::fail(json, 1, e)),
            }
        }
        (Some("--help" | "-h" | "help"), _) | (None, _) => {
            print_help();
            i32::from(args.is_empty())
        }
        (Some(verb @ ("status" | "connect" | "disconnect")), None) => usage(&format!("{verb} <folder>")),
        (Some(other), _) => {
            eprintln!("Unknown provider subcommand: {other}");
            print_help();
            1
        }
    }
}

fn print_help() {
    println!("moss provider status <folder> [<part>] [--json]");
    println!("moss provider connect <folder> <part> [--provider <id>] [--list <id> | --new-list <name>] [--json]");
    println!("moss provider disconnect <folder> <part> [--json]");
    println!();
    println!("Parts: {PARTS}. connect reads the key from the variable the provider names (such as SENDGRID_API_KEY), or from stdin.");
}

/// The folder, its site and a signed client, or the failure to print.
fn site(folder: &str, json: bool) -> Result<(String, String, MossSetaClient), Outcome> {
    let root = VaultRoot::resolve(folder);
    let path = root.path();
    let client = client_for_folder(path).map_err(|e| Outcome::fail(json, 1, e))?;
    let site_id = site_id_for_folder(path).map_err(|e| Outcome::fail(json, 1, e))?;
    Ok((path.to_string_lossy().into_owned(), site_id, client))
}

/// `[channels.email] provider`, `None` when the folder names none.
fn named_provider(folder: &str) -> Result<Option<String>, String> {
    read_project_config(folder)?.newsletter_service().transpose()
}

async fn cmd_status(folder: &str, json: bool) -> i32 {
    let (path, site_id, client) = match site(folder, json) {
        Ok(s) => s,
        Err(o) => return emit(o),
    };
    emit(status_flow(&client, &site_id, &path, json).await)
}

async fn status_flow(client: &MossSetaClient, site_id: &str, folder: &str, json: bool) -> Outcome {
    let named = match named_provider(folder) {
        Ok(n) => n,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let installed = named.as_deref().map(|n| newsletter_provider(folder, n).map(|_| ()));
    let seta: Option<SetaNewsletter> = match client.newsletter_status(site_id).await {
        Ok(s) => Some(s),
        Err(e) => return Outcome::fail(json, 3, e),
    };
    let connected = seta.as_ref().and_then(|s| s.connection.as_ref()).is_some_and(|c| c.state == ConnectionState::Connected);
    let ok = named.is_none() || (connected && matches!(installed, Some(Ok(()))));
    let code = if ok { 0 } else { 3 };
    let newsletter = serde_json::json!({
        "provider": named,
        "installed": installed.as_ref().map(Result::is_ok),
        "problem": installed.and_then(Result::err),
        "connected": connected,
        "seta": seta,
    });
    if json {
        return Outcome { code, out: format!("{}\n", serde_json::json!({ "newsletter": newsletter })), err: String::new() };
    }
    let words = match (&newsletter["provider"], connected) {
        (serde_json::Value::Null, _) => "the hosting service sends it".to_string(),
        (p, true) => format!("{} (connected)", p.as_str().unwrap_or_default()),
        (p, false) => format!("{} (not connected)", p.as_str().unwrap_or_default()),
    };
    let problem = newsletter["problem"].as_str().map(|p| format!("\n  {p}")).unwrap_or_default();
    Outcome { code, out: format!("newsletter: {words}{problem}\n"), err: String::new() }
}

async fn cmd_connect(folder: &str, c: Connect, json: bool) -> i32 {
    let (path, site_id, client) = match site(folder, json) {
        Ok(s) => s,
        Err(o) => return emit(o),
    };
    emit(connect_flow(&client, &site_id, &path, c, read_key, json).await)
}

/// The key from `credential.env`, else from stdin when it is not a terminal.
fn read_key(credential: &Credential) -> Result<String, String> {
    if let Some(key) = credential.env.as_deref().and_then(|v| std::env::var(v).ok()).filter(|k| !k.trim().is_empty()) {
        return Ok(key);
    }
    let mut stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let mut key = String::new();
        stdin.read_to_string(&mut key).map_err(|e| format!("reading the key from stdin: {e}"))?;
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    let from = credential.env.as_deref().map(|v| format!("set {v}, or ")).unwrap_or_default();
    Err(format!("no key: {from}pipe it in, as in `… | moss provider connect <folder> newsletter`"))
}

async fn connect_flow(
    client: &MossSetaClient,
    site_id: &str,
    folder: &str,
    c: Connect,
    read_key: impl FnOnce(&Credential) -> Result<String, String>,
    json: bool,
) -> Outcome {
    let named = match named_provider(folder) {
        Ok(n) => n,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let provider = match (named.clone(), c.provider) {
        (Some(a), Some(b)) if a != b => {
            return Outcome::fail(json, 1, format!("this folder's provider is {a}, not {b}: disconnect {a} first"));
        }
        (Some(p), _) | (None, Some(p)) => p,
        (None, None) => return Outcome::fail(json, 1, "this folder names no newsletter provider: pass --provider <id>"),
    };
    let contribution = match newsletter_provider(folder, &provider) {
        Ok(p) => p,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let Some(credential) = contribution.credentials.first() else {
        return Outcome::fail(json, 1, format!("{provider} names no key"));
    };
    let paste = match read_key(credential) {
        Ok(k) => k,
        Err(e) => return Outcome::fail(json, 1, e),
    };
    let Some(key) = credential.fits(&paste) else {
        return Outcome::fail(json, 1, format!("that is not a {provider} key: it starts with {} and is {} characters", credential.prefix, credential.length));
    };
    let answer = match client.newsletter_connect(site_id, &key, c.list).await {
        Ok(a) => a,
        Err(e) => return Outcome::fail(json, 3, e),
    };
    let (code, text) = match &answer {
        ConnectAnswer::Lists { account, lists } => {
            let rows: String = lists.iter().map(|l| format!("  {}  {} ({})\n", l.id, l.name, l.count)).collect();
            (2, format!("{account} has these lists; pass --list <id> or --new-list <name>:\n{rows}"))
        }
        ConnectAnswer::Refused { refused } => (3, format!("{}\n", refusal_words(*refused, &provider, credential, contribution.newsletter.as_ref()))),
        ConnectAnswer::Connected(done) => {
            if named.is_none() {
                if let Err(e) = crate::vault::config::save_newsletter_provider(folder, Some(&provider)) {
                    return Outcome::fail(json, 1, format!("connected, but [channels.email] provider was not written: {e}"));
                }
            }
            (0, format!("connected: {} sends to {}\n", done.account, done.list.name))
        }
    };
    let out = if json { format!("{}\n", serde_json::json!(answer)) } else { text };
    Outcome { code, out, err: String::new() }
}

/// What a refusal means and what to do about it, in the provider's own words.
fn refusal_words(refused: NewsletterRefusal, provider: &str, key: &Credential, body: Option<&NewsletterPart>) -> String {
    let at = |url: Option<&String>| url.map(|u| format!(" at {u}")).unwrap_or_default();
    let keys = at(Some(&key.url));
    match refused {
        NewsletterRefusal::Auth => format!("{provider} did not accept the key: make a new one{keys}"),
        NewsletterRefusal::Scopes => {
            let permission = &key.permission;
            format!("the key does not have {permission}: make a new one with {permission}{keys}")
        }
        NewsletterRefusal::WebhooksFull => {
            format!("{provider} has no room for another webhook: remove one{}, then connect again", at(body.and_then(|b| b.webhooks_url.as_ref())))
        }
        NewsletterRefusal::Sender => format!("{provider} cannot send from that address: add it{}", at(body.and_then(|b| b.senders_url.as_ref()))),
        NewsletterRefusal::CannotSend => "the connected key cannot send: connect again with a new key".to_string(),
        NewsletterRefusal::Unknown => format!("{provider} refused the key and gave no reason moss knows"),
        NewsletterRefusal::Unreachable => "moss's server is not answering: try again".to_string(),
    }
}

async fn cmd_disconnect(folder: &str, json: bool) -> i32 {
    let (path, site_id, client) = match site(folder, json) {
        Ok(s) => s,
        Err(o) => return emit(o),
    };
    emit(disconnect_flow(&client, &site_id, &path, json).await)
}

/// The hosting service forgets the connection first, so a refused call leaves
/// the folder's line as it was.
async fn disconnect_flow(client: &MossSetaClient, site_id: &str, folder: &str, json: bool) -> Outcome {
    if let Err(e) = client.newsletter_disconnect(site_id).await {
        return Outcome::fail(json, 3, e);
    }
    if let Err(e) = crate::vault::config::save_newsletter_provider(folder, None) {
        return Outcome::fail(json, 1, e);
    }
    let out = if json { format!("{}\n", serde_json::json!({ "status": "disconnected" })) } else { "disconnected\n".into() };
    Outcome { code: 0, out, err: String::new() }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
