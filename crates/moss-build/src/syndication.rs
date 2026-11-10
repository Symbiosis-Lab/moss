//! Syndication: posting a copy of a published page to each network its folder
//! names, once, and writing each copy's address back into the page.
//!
//! The author names channels with `syndicate:` (on a page, or for a folder's
//! pages through `cascade:`); moss posts after a publish lands and records each
//! copy under `syndicated:`. The design, with the reasons for each rule, is in
//! `docs/proposals/syndication/README.md`.

pub mod channel;
pub mod choose;
pub mod job;
pub mod landing;
pub mod plan;
pub mod preview;
pub mod record;
pub mod send;
pub mod shape;

/// What a page's `syndicate:` asks for: the channel ids, in the author's order.
/// An empty list is the author's `syndicate: false`, which also stops a
/// folder's cascade from reaching the page.
///
/// A single id is accepted as a list of one, since that is how an author writes
/// one channel. Anything else is an error naming what was written, and the
/// caller decides how loudly to say it.
pub fn parse_choice(value: &serde_json::Value) -> Result<Vec<String>, String> {
    use serde_json::Value;
    let id = |v: &Value| match v {
        Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        other => Err(format!("`syndicate:` lists channel ids such as bluesky, not {other}")),
    };
    match value {
        Value::Bool(false) => Ok(Vec::new()),
        Value::String(_) => Ok(vec![id(value)?]),
        Value::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                let item = id(item)?;
                if !ids.contains(&item) {
                    ids.push(item);
                }
            }
            Ok(ids)
        }
        other => Err(format!("`syndicate:` is a list of channel ids or false, not {other}")),
    }
}

/// A channel plugin running `js` as its hooks, set up for the site at `root`,
/// for tests that call real hooks.
#[cfg(test)]
pub(crate) fn test_channel(root: &std::path::Path, name: &str, js: &str) {
    use std::io::Write;
    let dir = root.join(".moss/plugins").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let global = format!("{}Plugin", name.to_uppercase());
    let manifest = serde_json::json!({
        "name": name, "version": "0.0.1", "display_name": name, "description": "", "author": "",
        "entry": "main.bundle.js", "global_name": global,
        "contributes": {"channel": {"syndication": {"forms": ["short", "teaser"], "max_chars": 300}}}
    });
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("main.bundle.js"), format!("window.{global} = {js};")).unwrap();
    let mut config = std::fs::OpenOptions::new().create(true).append(true).open(root.join(".moss/config.toml")).unwrap();
    writeln!(config, "[channels.{name}]").unwrap();
}

#[cfg(test)]
mod tests {
    use super::parse_choice;
    use serde_json::json;

    #[test]
    fn a_list_a_single_id_and_false_are_what_an_author_writes() {
        assert_eq!(parse_choice(&json!(["bluesky", "mastodon", "bluesky"])).unwrap(), ["bluesky", "mastodon"]);
        assert_eq!(parse_choice(&json!("bluesky")).unwrap(), ["bluesky"]);
        assert!(parse_choice(&json!(false)).unwrap().is_empty());
    }

    #[test]
    fn anything_else_is_refused_with_what_was_written() {
        for bad in [json!(true), json!(3), json!(["bluesky", 2]), json!([""]), json!({"bluesky": true})] {
            assert!(parse_choice(&bad).is_err(), "{bad} should be refused");
        }
    }
}
