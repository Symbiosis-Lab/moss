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
pub mod pictures;
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

/// A `syndicate:` list with each entry turned into its channel's id, the one
/// form records and idempotency keys are kept under. Everything in this
/// module reads a page's list through here, never as the page wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Named {
    ids: Vec<String>,
    shared: Vec<(String, String)>,
}

impl Named {
    /// An id stays, and so does `email`; any other entry is the name one
    /// channel goes by, ignoring case. An entry that matches nothing stays as
    /// written, so the plan can say no such channel is set up.
    pub fn of(list: &[String], channels: &std::collections::BTreeMap<String, plan::ChannelInfo>) -> Self {
        let mut named = Self::default();
        for entry in list {
            let goes_by = |c: &plan::ChannelInfo| c.name.as_ref().is_some_and(|n| n.to_lowercase() == entry.to_lowercase());
            let by_name: Vec<&str> = channels.iter().filter(|(_, c)| goes_by(c)).map(|(id, _)| id.as_str()).collect();
            let id = match by_name.as_slice() {
                _ if entry == plan::RESERVED_EMAIL || channels.contains_key(entry) => entry,
                [] => entry,
                [id] => *id,
                ids => {
                    named.shared.push((entry.clone(), format!("{entry} is the name of more than one channel ({}): write the id of the one meant", ids.join(", "))));
                    continue;
                }
            };
            if !named.ids.iter().any(|i| i == id) {
                named.ids.push(id.to_string());
            }
        }
        named
    }

    /// The channels a post's copies go to, its folders' list applied; `None`
    /// when neither the post nor a folder names any.
    pub fn of_post(post: &crate::build::scan::article_map::ArticleInfo, channels: &std::collections::BTreeMap<String, plan::ChannelInfo>) -> Option<Self> {
        post.syndicate.as_ref().map(|list| Self::of(list, channels))
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// Entries that name more than one channel, each as written with why it
    /// names none.
    pub fn shared(&self) -> &[(String, String)] {
        &self.shared
    }
}

/// A channel plugin running `js` as its hooks, set up for the site at `root`,
/// for tests that call real hooks.
#[cfg(test)]
pub(crate) fn test_channel(root: &std::path::Path, name: &str, js: &str) {
    test_channel_named(root, name, name, js);
}

/// [`test_channel`] for a network that goes by `display`.
#[cfg(test)]
pub(crate) fn test_channel_named(root: &std::path::Path, name: &str, display: &str, js: &str) {
    use std::io::Write;
    let dir = root.join(".moss/plugins").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let global = format!("{}Plugin", name.to_uppercase());
    let manifest = serde_json::json!({
        "name": name, "version": "0.0.1", "display_name": display, "description": "", "author": "",
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
    use super::{parse_choice, Named};
    use serde_json::json;

    fn channels(named: &[(&str, &str)]) -> std::collections::BTreeMap<String, super::plan::ChannelInfo> {
        named.iter().map(|(id, name)| (id.to_string(), super::plan::ChannelInfo { name: Some(name.to_string()), ..Default::default() })).collect()
    }

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| e.to_string()).collect()
    }

    #[test]
    fn an_entry_is_a_channels_id_or_the_name_its_network_goes_by() {
        let set_up = channels(&[("xcom", "X"), ("bluesky", "Bluesky"), ("buttondown", "Email")]);
        let named = Named::of(&list(&["x", "BLUESKY", "xcom", "email", "ghost"]), &set_up);
        assert_eq!(named.ids(), ["xcom", "bluesky", "email", "ghost"], "one id each, the newsletter and an unknown entry as written");
        assert!(named.shared().is_empty());
    }

    #[test]
    fn an_id_is_taken_before_another_channels_name() {
        let set_up = channels(&[("x", "Elsewhere"), ("xcom", "X")]);
        assert_eq!(Named::of(&list(&["x"]), &set_up).ids(), ["x"]);
        assert_eq!(Named::of(&list(&["X"]), &set_up).ids(), ["xcom"]);
    }

    #[test]
    fn a_name_two_channels_go_by_names_neither_and_says_both() {
        let set_up = channels(&[("xcom", "X"), ("xmirror", "x"), ("bluesky", "Bluesky")]);
        let named = Named::of(&list(&["X", "bluesky"]), &set_up);
        assert_eq!(named.ids(), ["bluesky"]);
        let [(written, why)] = named.shared() else { panic!("{named:?}") };
        assert_eq!(written, "X");
        assert!(why.contains("xcom") && why.contains("xmirror"), "{why}");
    }

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
