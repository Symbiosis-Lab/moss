//! `moss comments` — list, hide, unhide and mark as spam a site's comments
//! from the command line, and list and unblock the link hosts spam blocked.
//!
//! Hiding is a signed event appended to `moderation.jsonl` (see
//! [`super::moderation`]); nothing is erased or sent anywhere, so `unhide` is
//! just a later event. Only `blocked` asks the hosting service, for how many
//! comments it dropped per blocked host. The command lives beside the comment
//! feature rather than in `cli/`, which is at its file budget.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use super::link_block::{active_blocks, comment_hosts, host_of, own_hosts, LINK_SOURCE};
use super::moderation::{append_owner_event, load_mod_events, site_value, ModKind};
use super::reduce::resolve_detailed;
use super::{load_all_social_comments, MATTERS_DOMAIN_FALLBACK};
use crate::cli::list::{display_width, fail, pad};
use crate::moss_paths::MossPaths;
use crate::vault_root::{resolve_input, VaultRoot};

fn usage() -> &'static str {
    "Usage:
  moss comments list [<folder>] [--json]
  moss comments hide [<folder>] <id>... [--source <name>] [--json]
  moss comments unhide [<folder>] <id>... [--source <name>] [--json]
  moss comments spam [<folder>] <id>... [--source <name>] [--json]
  moss comments blocked [<folder>] [--json]
  moss comments unblock [<folder>] <host>... [--json]
  moss comments adopt-key [<folder>] <pubkey> [--json]

list shows every comment, newest first, and marks the hidden ones; it needs no build.
hide and unhide change what the next publish shows: nothing is erased, and `unhide` reverses `hide`.
spam hides the comments and blocks every link host in them, except the site's own and any a comment you
unhid links to: the next publish hides other comments linking to those hosts and the comment form drops new ones.
blocked lists the blocked hosts and how many new comments each had dropped in the last 90 days; unblock removes one.
Hiding needs the site's own signing key (.moss/identity) and never creates one.
<id> is a comment id from `list`; pass --source when the same id exists under two sources.
adopt-key keeps the hides of this site's earlier key in force after its key was replaced along with
.moss/identity/public.json; `list` names such a key.
<folder> is a site's folder (one already containing .moss); with no folder, the site containing the current directory."
}

/// One comment as the command reports it.
struct Row {
    id: String,
    source: String,
    page_uid: String,
    page_url: Option<String>,
    page_title: Option<String>,
    author: String,
    created_at: String,
    text: String,
    /// Not shown on the site: hidden itself, or under a hidden parent.
    hidden: bool,
    /// Hidden by an event on this very comment, not only through its parent.
    directly_hidden: bool,
    /// The link hosts the comment carries.
    hosts: BTreeSet<String>,
}

impl Row {
    fn page_label(&self) -> String {
        safe(self.page_url.as_deref().unwrap_or(&self.page_uid))
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "source": self.source,
            "pageUid": self.page_uid,
            "pageUrl": self.page_url,
            "pageTitle": self.page_title,
            "author": self.author,
            "createdAt": self.created_at,
            "text": self.text,
            "hidden": self.hidden,
        })
    }

    /// `12 (artalk) by Ana on /posts/hello.html: "Great post, ..."`
    fn summary(&self) -> String {
        format!(
            "{} ({}) by {} on {}: \"{}\"",
            safe(&self.id),
            safe(&self.source),
            safe(&self.author),
            self.page_label(),
            safe(&self.text)
        )
    }
}

/// The first 60 characters of a comment's text, markup stripped.
fn excerpt(html: &str) -> String {
    crate::build::markdown::html_post::strip_html_tags(html).chars().take(60).collect()
}

/// Comment text, author names and ids are written by strangers and printed to
/// the owner's terminal, so every human-readable output goes through this:
/// each character that is a control (`char::is_control`: C0, DEL and C1,
/// U+0000-U+001F and U+007F-U+009F) or an invisible bidi control (U+061C,
/// U+200E, U+200F, U+202A-U+202E, U+2066-U+2069) becomes a space. U+200C and
/// U+200D stay: words and emoji need them. `--json` keeps the real characters,
/// escaped.
fn safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{61C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Every comment of the site, newest first, with its visibility as the build
/// would resolve it. Reads the synced social files, the moderation log and the
/// last build's article map; needs no build. The flag says whether some page
/// could not be named from the article map.
fn collect(project_path: &str) -> (Vec<Row>, bool) {
    let all = load_all_social_comments(project_path, MATTERS_DOMAIN_FALLBACK);
    let events = load_mod_events(project_path);
    // Read-only: `Identity::load` would migrate an old-format identity in place.
    let owners = crate::identity::keypair::Identity::read_trusted_pubkeys(Path::new(project_path));
    let articles = crate::build::load_article_map_for_features(
        &MossPaths::new(Path::new(project_path)).article_map(),
    );
    let by_uid: std::collections::HashMap<&str, &crate::build::features::ArticleInfo> =
        articles.values().map(|a| (a.uid.as_str(), a)).collect();

    let mut rows = Vec::new();
    let mut unresolved = false;
    for (uid, comments) in &all {
        let (shown, direct) = resolve_detailed(comments, &events, &owners);
        let visible: HashSet<(&str, &str)> = shown.iter().map(|c| (c.source.as_str(), c.id.as_str())).collect();
        let article = by_uid.get(uid.as_str());
        unresolved |= article.is_none();
        for c in comments {
            let hidden = !visible.contains(&(c.source.as_str(), c.id.as_str()));
            let directly_hidden = direct.contains(&(c.source.as_str(), c.id.as_str()));
            rows.push(Row {
                id: c.id.clone(),
                source: c.source.clone(),
                page_uid: uid.clone(),
                page_url: article.map(|a| format!("/{}", a.url_path.trim_start_matches('/'))),
                page_title: article.map(|a| a.title.clone()).filter(|t| !t.is_empty()),
                author: c
                    .author
                    .display_name
                    .clone()
                    .or_else(|| c.author.name.clone())
                    .unwrap_or_default(),
                created_at: c.created_at.clone(),
                text: excerpt(&c.content),
                hidden,
                directly_hidden,
                hosts: comment_hosts(c),
            });
        }
    }
    rows.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
    (rows, unresolved)
}

fn render_table(rows: &[Row]) -> String {
    let header = ["ID", "SOURCE", "HIDDEN", "CREATED", "AUTHOR", "PAGE", "TEXT"];
    let cells: Vec<[String; 7]> = rows
        .iter()
        .map(|r| {
            let page = match &r.page_title {
                Some(t) => format!("{} ({})", r.page_label(), safe(t)),
                None => r.page_label(),
            };
            [
                safe(&r.id),
                safe(&r.source),
                if r.hidden { "hidden" } else { "-" }.to_string(),
                safe(&r.created_at),
                safe(&r.author),
                page,
                safe(&r.text),
            ]
        })
        .collect();
    let mut widths = header.map(display_width);
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(c));
        }
    }
    let mut out = String::new();
    let mut line = |row: &[String]| {
        for (i, c) in row.iter().enumerate() {
            if i + 1 == row.len() {
                out.push_str(c);
            } else {
                out.push_str(&pad(c, widths[i]));
                out.push_str("  ");
            }
        }
        out.push('\n');
    };
    line(&header.map(String::from));
    for row in &cells {
        line(row);
    }
    out
}

struct Parsed {
    folder: Option<String>,
    ids: Vec<String>,
    source: Option<String>,
    json: bool,
}

/// `Ok(None)` is `--help`. A folder is only ever the FIRST positional, and only
/// when it is a directory that already owns a `.moss/`, the way `moss history`
/// and `moss build` recognize one.
fn parse(args: &[String]) -> Result<Option<Parsed>, String> {
    let mut positionals = Vec::new();
    let (mut json, mut source) = (false, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(None),
            "--json" => json = true,
            "--source" => {
                i += 1;
                source = Some(args.get(i).ok_or("--source requires a name")?.clone());
            }
            a if a.starts_with('-') => return Err(format!("unknown option '{a}'")),
            a => positionals.push(a.to_string()),
        }
        i += 1;
    }
    let first_is_folder = positionals.first().is_some_and(|first| {
        let p = resolve_input(first);
        p.is_dir() && crate::nested_roots::owns_moss(&p)
    });
    let folder = first_is_folder.then(|| positionals.remove(0));
    Ok(Some(Parsed { folder, ids: positionals, source, json }))
}

/// Entry point for `moss comments`. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let Some((action, rest)) = args.split_first() else {
        eprintln!("{}", usage());
        return 1;
    };
    if matches!(action.as_str(), "-h" | "--help") {
        println!("{}", usage());
        return 0;
    }
    if !matches!(action.as_str(), "list" | "hide" | "unhide" | "spam" | "blocked" | "unblock" | "adopt-key") {
        eprintln!("error: unknown comments subcommand '{action}'\n{}", usage());
        return 1;
    }
    let parsed = match parse(rest) {
        Ok(Some(p)) => p,
        Ok(None) => {
            println!("{}", usage());
            return 0;
        }
        Err(message) => {
            eprintln!("error: {message}\n{}", usage());
            return 1;
        }
    };
    let root = match &parsed.folder {
        Some(folder) => VaultRoot::resolve(folder),
        None => VaultRoot::containing(Path::new(".")),
    };
    if !crate::nested_roots::owns_moss(root.path()) {
        return fail(
            &format!(
                "{} is not a moss site folder (no .moss inside); pass a site folder or run from inside one",
                root.as_str()
            ),
            parsed.json,
        );
    }
    let project_path = root.as_str();
    if action == "list" {
        if !parsed.ids.is_empty() || parsed.source.is_some() {
            return fail("`moss comments list` takes only a folder and --json", parsed.json);
        }
        return run_list(project_path, parsed.json);
    }
    if action == "blocked" {
        if !parsed.ids.is_empty() || parsed.source.is_some() {
            return fail("`moss comments blocked` takes only a folder and --json", parsed.json);
        }
        return run_blocked(project_path, parsed.json);
    }
    if action == "unblock" {
        if parsed.ids.is_empty() {
            return fail("`moss comments unblock` needs at least one host; `moss comments blocked` lists them", parsed.json);
        }
        return run_unblock(project_path, &parsed);
    }
    if action == "adopt-key" {
        let [pubkey] = parsed.ids.as_slice() else {
            return fail("`moss comments adopt-key` takes one public key", parsed.json);
        };
        if let Err(e) = super::moderation::adopt_key(project_path, pubkey, &site_of(project_path)) {
            return fail(&e, parsed.json);
        }
        if parsed.json {
            println!("{}", serde_json::json!({ "adopted": pubkey }));
        } else {
            println!("Moderation signed by {pubkey} applies again from the next publish.");
        }
        return 0;
    }
    if parsed.ids.is_empty() {
        return fail(
            &format!("`moss comments {action}` needs at least one comment id; `moss comments list` shows them"),
            parsed.json,
        );
    }
    if action == "spam" {
        return run_spam(project_path, &parsed);
    }
    run_moderate(project_path, action == "hide", &parsed)
}

/// The hosts blocked now, sorted.
fn blocked_now(project_path: &str) -> Vec<String> {
    let owners = crate::identity::keypair::Identity::read_trusted_pubkeys(Path::new(project_path));
    active_blocks(&load_mod_events(project_path), &owners).into_keys().map(str::to_string).collect()
}

/// One blocked host's comments the hosting service dropped in its 90-day window; `last` is unix seconds.
#[derive(serde::Deserialize)]
struct HostDrops {
    host: String,
    dropped: u64,
    last: i64,
}

#[derive(serde::Deserialize)]
struct DropsAnswer {
    hosts: Vec<HostDrops>,
}

fn run_blocked(project_path: &str, json: bool) -> i32 {
    let hosts = blocked_now(project_path);
    if hosts.is_empty() {
        print!("{}", render_blocked(&hosts, Ok(Vec::new()), json));
        return 0;
    }
    let folder = Path::new(project_path);
    let site = crate::cli::client_for_folder(folder)
        .and_then(|client| crate::build::site_config::site_id_for_folder(folder).map(|id| (client, id)));
    match site {
        Ok((client, site_id)) => crate::cli::run_async(async move {
            print!("{}", blocked_report(&hosts, &client, &site_id, json).await);
            0
        }),
        Err(e) => {
            print!("{}", render_blocked(&hosts, Err(e), json));
            0
        }
    }
}

/// The blocked hosts with the hosting service's drop counts for `site_id`.
async fn blocked_report(hosts: &[String], client: &crate::seta::client::MossSetaClient, site_id: &str, json: bool) -> String {
    let drops = client
        .get_json::<DropsAnswer>(&format!("/api/sites/{site_id}/comments/blocked"))
        .await
        .map(|a| a.hosts)
        .map_err(|e| e.to_string());
    render_blocked(hosts, drops, json)
}

fn render_blocked(hosts: &[String], drops: Result<Vec<HostDrops>, String>, json: bool) -> String {
    let day = |at: i64| chrono::DateTime::from_timestamp(at, 0).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    let count = |h: &str| drops.as_ref().ok().map(|d| d.iter().find(|x| x.host == h).map_or((0, None), |x| (x.dropped, Some(day(x.last)))));
    if json {
        let rows: Vec<_> = hosts
            .iter()
            .map(|h| match count(h) {
                Some((dropped, last)) => serde_json::json!({ "host": h, "dropped": dropped, "last": last }),
                None => serde_json::json!({ "host": h, "dropped": null, "last": null }),
            })
            .collect();
        return format!("{}\n", serde_json::json!({ "hosts": rows, "drops_unavailable": drops.as_ref().err() }));
    }
    if hosts.is_empty() {
        return "No blocked hosts.\n".into();
    }
    let mut out = String::new();
    if drops.is_ok() {
        out.push_str("New comments dropped in the last 90 days:\n");
    }
    for h in hosts {
        out.push_str(&match count(h) {
            Some((n, Some(last))) => format!("{} · {n} dropped · last {last}\n", safe(h)),
            Some((n, None)) => format!("{} · {n} dropped\n", safe(h)),
            None => format!("{}\n", safe(h)),
        });
    }
    if let Err(e) = &drops {
        out.push_str(&format!("Drop counts unavailable: {}\n", safe(e)));
    }
    out
}

fn run_unblock(project_path: &str, parsed: &Parsed) -> i32 {
    let blocked = blocked_now(project_path);
    let mut hosts = Vec::new();
    for raw in &parsed.ids {
        let Some(host) = host_of(raw) else {
            return fail(&format!("'{}' is not a host; nothing was changed", safe(raw)), parsed.json);
        };
        if !blocked.contains(&host) {
            return fail(&format!("{host} is not blocked; nothing was changed. `moss comments blocked` lists the blocked hosts"), parsed.json);
        }
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    let site = site_of(project_path);
    for host in &hosts {
        if let Err(e) = append_owner_event(project_path, ModKind::Unblock, LINK_SOURCE, host, "", &site) {
            return fail(&e, parsed.json);
        }
    }
    if parsed.json {
        println!("{}", serde_json::json!({ "unblocked": hosts }));
    } else {
        println!("Unblocked {}. The change takes effect at the next publish.", hosts.join(", "));
    }
    0
}

/// The rows `ids` name, or the message for the first that names none or
/// several. Every id is checked before anything is written.
fn find_rows<'r>(rows: &'r [Row], parsed: &Parsed) -> Result<Vec<&'r Row>, String> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for id in &parsed.ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let matches: Vec<&Row> = rows
            .iter()
            .filter(|r| &r.id == id && parsed.source.as_deref().is_none_or(|s| r.source == s))
            .collect();
        let mut sources: Vec<&str> = matches.iter().map(|r| r.source.as_str()).collect();
        sources.sort();
        sources.dedup();
        match (matches.first(), sources.len()) {
            (None, _) => {
                let scope = parsed.source.as_deref().map(|s| format!(" from source '{s}'")).unwrap_or_default();
                return Err(format!("no comment with id '{id}'{scope}; nothing was changed. `moss comments list` shows the ids"));
            }
            (Some(_), n) if n > 1 => {
                return Err(format!(
                    "id '{id}' exists under several sources ({}); say which with --source <name>. Nothing was changed",
                    sources.join(", ")
                ));
            }
            (Some(row), _) => found.push(*row),
        }
    }
    Ok(found)
}

/// What Spam did, for the line or toast that names it and for Undo.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpamOutcome {
    /// `(source, id)` of each comment this call hid; one already hidden is not here.
    pub hidden: Vec<(String, String)>,
    pub blocked: BTreeSet<String>,
    /// Hosts left unblocked because a comment the owner unhid links to them.
    pub not_blocked: BTreeSet<String>,
    /// Other comments the new blocks hide.
    pub also_hidden: usize,
}

impl SpamOutcome {
    /// `Spam · a.example, b.example blocked · 4 more comments hidden`.
    pub fn line(&self) -> String {
        let mut line = String::from("Spam");
        if !self.blocked.is_empty() {
            line.push_str(&format!(" · {} blocked", self.blocked.iter().map(String::as_str).collect::<Vec<_>>().join(", ")));
        }
        match self.also_hidden {
            0 => {}
            1 => line.push_str(" · 1 more comment hidden"),
            n => line.push_str(&format!(" · {n} more comments hidden")),
        }
        line
    }
}

/// Spam: hide each `(source, id)` (already hidden ones included) and block
/// every host they link to that is not the site's own, not already blocked,
/// and not linked from a comment the owner unhid: other comments still shown
/// with the same host are usually the same spammer, and the line names what
/// was blocked so a wrong host is seen and undone.
pub fn mark_spam(project_path: &str, targets: &[(String, String)]) -> Result<SpamOutcome, String> {
    let (rows, _) = collect(project_path);
    let events = load_mod_events(project_path);
    let owners = crate::identity::keypair::Identity::read_trusted_pubkeys(Path::new(project_path));
    let unhid: HashSet<(&str, &str)> = super::link_block::latest_by_target(&events, &owners, ["hide", "unhide"])
        .into_iter()
        .filter(|(_, e)| e.kind == "unhide")
        .map(|(k, _)| k)
        .collect();
    let is_target = |r: &Row| targets.iter().any(|(source, id)| *id == r.id && *source == r.source);
    let chosen: Vec<&Row> = rows.iter().filter(|r| is_target(r)).collect();
    if let Some((source, id)) = targets.iter().find(|(s, i)| !chosen.iter().any(|r| r.source == *s && r.id == *i)) {
        return Err(format!("no comment with id '{id}' from source '{source}'; nothing was changed"));
    }
    let own = own_hosts(project_path);
    let blocked: BTreeSet<String> = blocked_now(project_path).into_iter().collect();
    let kept_hosts: BTreeSet<&String> = rows
        .iter()
        .filter(|r| !r.hidden && !is_target(r) && unhid.contains(&(r.source.as_str(), r.id.as_str())))
        .flat_map(|r| &r.hosts)
        .collect();
    let mut outcome = SpamOutcome::default();
    for host in chosen.iter().flat_map(|r| &r.hosts).filter(|h| !own.contains(*h) && !blocked.contains(*h)) {
        let into = if kept_hosts.contains(host) { &mut outcome.not_blocked } else { &mut outcome.blocked };
        into.insert(host.clone());
    }

    let site = site_of(project_path);
    for row in chosen.iter().filter(|r| !r.directly_hidden) {
        append_owner_event(project_path, ModKind::Hide, &row.source, &row.id, &row.page_uid, &site)?;
        outcome.hidden.push((row.source.clone(), row.id.clone()));
    }
    for host in &outcome.blocked {
        append_owner_event(project_path, ModKind::Block, LINK_SOURCE, host, "", &site)?;
    }
    let before: HashSet<(&str, &str)> = rows.iter().filter(|r| r.hidden).map(|r| (r.source.as_str(), r.id.as_str())).collect();
    outcome.also_hidden = collect(project_path)
        .0
        .iter()
        .filter(|r| r.hidden && !is_target(r) && !before.contains(&(r.source.as_str(), r.id.as_str())))
        .count();
    Ok(outcome)
}

/// Undo of [`mark_spam`]: unhide what it hid and unblock what it blocked.
pub fn undo_spam(project_path: &str, outcome: &SpamOutcome) -> Result<(), String> {
    let site = site_of(project_path);
    let (rows, _) = collect(project_path);
    for (source, id) in &outcome.hidden {
        let page = rows.iter().find(|r| r.source == *source && r.id == *id).map_or("", |r| r.page_uid.as_str());
        append_owner_event(project_path, ModKind::Unhide, source, id, page, &site)?;
    }
    for host in &outcome.blocked {
        append_owner_event(project_path, ModKind::Unblock, LINK_SOURCE, host, "", &site)?;
    }
    Ok(())
}

fn run_spam(project_path: &str, parsed: &Parsed) -> i32 {
    let (rows, _) = collect(project_path);
    let targets: Vec<(String, String)> = match find_rows(&rows, parsed) {
        Ok(found) => found.iter().map(|r| (r.source.clone(), r.id.clone())).collect(),
        Err(e) => return fail(&e, parsed.json),
    };
    let outcome = match mark_spam(project_path, &targets) {
        Ok(o) => o,
        Err(e) => return fail(&e, parsed.json),
    };
    if parsed.json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap_or_default());
        return 0;
    }
    println!("{}", outcome.line());
    if !outcome.not_blocked.is_empty() {
        println!(
            "Not blocked, a comment you unhid links to it: {}",
            outcome.not_blocked.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
        );
    }
    println!("The change takes effect at the next publish.");
    0
}

/// The site value moderation events carry for this folder.
fn site_of(project_path: &str) -> String {
    let site_id = crate::build::site_config::get_domain_config(project_path)
        .ok()
        .and_then(|c| c.site_id);
    site_value(site_id.as_deref()).to_string()
}

fn run_list(project_path: &str, json: bool) -> i32 {
    let owners = crate::identity::keypair::Identity::read_trusted_pubkeys(Path::new(project_path));
    if !owners.is_empty() {
        for key in super::moderation::untrusted_signers(&load_mod_events(project_path), &owners) {
            eprintln!(
                "Moderation signed by {key} does not apply: it is not one of this folder's keys. If it was this site's earlier key, run `moss comments adopt-key {project_path} {key}`; a new key also has to be signed in again (`moss deploy {project_path} --email <address>`)."
            );
        }
    }
    let (rows, unresolved) = collect(project_path);
    if json {
        let list: Vec<_> = rows.iter().map(Row::to_json).collect();
        println!("{}", serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into()));
        return 0;
    }
    if rows.is_empty() {
        println!("No comments.");
        return 0;
    }
    print!("{}", render_table(&rows));
    if unresolved {
        println!();
        println!("Pages show their uid until a build fills in addresses: run `moss build {project_path}`.");
    }
    0
}

fn run_moderate(project_path: &str, hide: bool, parsed: &Parsed) -> i32 {
    let kind = if hide { ModKind::Hide } else { ModKind::Unhide };
    let (rows, _) = collect(project_path);

    // Validate every id before appending anything: a call applies to all of
    // its ids or to none.
    let found = match find_rows(&rows, parsed) {
        Ok(f) => f,
        Err(e) => return fail(&e, parsed.json),
    };
    let mut targets: Vec<&Row> = Vec::new();
    let mut unchanged: Vec<(&Row, &str)> = Vec::new();
    for row in found {
        let id = &row.id;
        if hide && row.hidden {
            unchanged.push((row, "already hidden"));
        } else if !hide && !row.directly_hidden {
            if row.hidden {
                return fail(
                    &format!("comment '{id}' is hidden only because its parent is hidden; unhide the parent instead. Nothing was changed"),
                    parsed.json,
                );
            }
            unchanged.push((row, "not hidden"));
        } else {
            targets.push(row);
        }
    }

    let site = site_of(project_path);
    let mut changed: Vec<(&Row, u64)> = Vec::new();
    for row in targets {
        match append_owner_event(project_path, kind, &row.source, &row.id, &row.page_uid, &site) {
            Ok(seq) => changed.push((row, seq)),
            Err(e) => return fail(&e, parsed.json),
        }
    }

    if parsed.json {
        let with = |row: &Row, key: &str, value: serde_json::Value| {
            let mut v = row.to_json();
            v[key] = value;
            v
        };
        let out = serde_json::json!({
            "action": kind.as_str(),
            "changed": changed.iter().map(|(r, seq)| with(r, "seq", (*seq).into())).collect::<Vec<_>>(),
            "unchanged": unchanged.iter().map(|(r, why)| with(r, "reason", (*why).into())).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    let (done, undo) = if hide { ("Hid", "unhide") } else { ("Unhid", "hide") };
    for (row, _) in &changed {
        println!("{done} {}", row.summary());
    }
    for (row, why) in &unchanged {
        println!("Left {} ({why})", row.summary());
    }
    if !changed.is_empty() {
        println!("The change takes effect at the next publish; `moss comments {undo}` reverses it.");
    }
    0
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
