//! `moss comments` — list, hide and unhide a site's comments from the
//! command line.
//!
//! Hiding is a signed event appended to `moderation.jsonl` (see
//! [`super::moderation`]); nothing is erased and nothing is sent anywhere, so
//! `unhide` is just a later event. The command lives beside the comment
//! feature rather than in `cli/`, which is at its file budget.

use std::collections::HashSet;
use std::path::Path;

use super::moderation::{append_owner_event, load_mod_events, site_value, ModKind};
use super::reduce::resolve_detailed;
use super::{load_all_social_comments, MATTERS_DOMAIN_FALLBACK};
use crate::cli::list::{display_width, json_error, pad};
use crate::moss_paths::MossPaths;
use crate::vault_root::{resolve_input, VaultRoot};

fn usage() -> &'static str {
    "Usage:
  moss comments list [<folder>] [--json]
  moss comments hide [<folder>] <id>... [--source <name>] [--json]
  moss comments unhide [<folder>] <id>... [--source <name>] [--json]

list shows every comment, newest first, and marks the hidden ones; it needs no build.
hide and unhide change what the next publish shows: nothing is erased, and `unhide` reverses `hide`.
Hiding needs the site's own signing key (.moss/identity) and never creates one.
<id> is a comment id from `list`; pass --source when the same id exists under two sources.
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
    let pubkey = crate::identity::keypair::Identity::read_pubkey(Path::new(project_path));
    let articles = crate::build::load_article_map_for_features(
        &MossPaths::new(Path::new(project_path)).article_map(),
    );
    let by_uid: std::collections::HashMap<&str, &crate::build::features::ArticleInfo> =
        articles.values().map(|a| (a.uid.as_str(), a)).collect();

    let mut rows = Vec::new();
    let mut unresolved = false;
    for (uid, comments) in &all {
        let moderated = pubkey.as_deref().filter(|_| !events.is_empty());
        let (visible, direct): (Option<HashSet<(&str, &str)>>, HashSet<(&str, &str)>) = match moderated {
            Some(pk) => {
                let (shown, direct) = resolve_detailed(comments, &events, pk);
                (Some(shown.iter().map(|c| (c.source.as_str(), c.id.as_str())).collect()), direct)
            }
            None => (None, HashSet::new()),
        };
        let article = by_uid.get(uid.as_str());
        unresolved |= article.is_none();
        for c in comments {
            let hidden = visible
                .as_ref()
                .is_some_and(|v| !v.contains(&(c.source.as_str(), c.id.as_str())));
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

fn fail(message: &str, json: bool) -> i32 {
    if json {
        println!("{}", json_error(message));
    } else {
        eprintln!("error: {message}");
    }
    1
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
    if !matches!(action.as_str(), "list" | "hide" | "unhide") {
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
    if parsed.ids.is_empty() {
        return fail(
            &format!("`moss comments {action}` needs at least one comment id; `moss comments list` shows them"),
            parsed.json,
        );
    }
    run_moderate(project_path, action == "hide", &parsed)
}

fn run_list(project_path: &str, json: bool) -> i32 {
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
    let mut targets: Vec<&Row> = Vec::new();
    let mut unchanged: Vec<(&Row, &str)> = Vec::new();
    let mut seen = HashSet::new();
    for id in &parsed.ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let matches: Vec<&Row> = rows
            .iter()
            .filter(|r| &r.id == id && parsed.source.as_deref().is_none_or(|s| r.source == s))
            .collect();
        let sources: HashSet<&str> = matches.iter().map(|r| r.source.as_str()).collect();
        let row = match (matches.first(), sources.len()) {
            (None, _) => {
                let scope = parsed
                    .source
                    .as_deref()
                    .map(|s| format!(" from source '{s}'"))
                    .unwrap_or_default();
                return fail(
                    &format!("no comment with id '{id}'{scope}; nothing was changed. `moss comments list` shows the ids"),
                    parsed.json,
                );
            }
            (Some(_), n) if n > 1 => {
                let mut names: Vec<&str> = sources.into_iter().collect();
                names.sort();
                return fail(
                    &format!(
                        "id '{id}' exists under several sources ({}); say which with --source <name>. Nothing was changed",
                        names.join(", ")
                    ),
                    parsed.json,
                );
            }
            (Some(row), _) => *row,
        };
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

    let site_id = crate::build::site_config::get_domain_config(project_path)
        .ok()
        .and_then(|c| c.site_id);
    let site = site_value(site_id.as_deref());
    let mut changed: Vec<(&Row, u64)> = Vec::new();
    for row in targets {
        match append_owner_event(project_path, kind, &row.source, &row.id, &row.page_uid, site) {
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
