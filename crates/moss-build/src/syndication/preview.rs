//! The copies as data before anything is sent: for `moss deploy --dry-run`,
//! and for a host that shows a post's copies before the author publishes.

use std::path::Path;

use serde::Serialize;

use super::channel::{ChannelSpec, Form};
use super::landing::Before;
use super::plan::{Action, ChannelInfo};
use super::shape::{shape, Teaser};
use crate::build::manifest::SealedManifest;
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
use crate::moss_paths::MossPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewPart {
    pub text: String,
    /// The text's length as this network counts it, and its limit.
    pub chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewCopy {
    pub source_path: String,
    pub channel: String,
    #[serde(flatten)]
    pub action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<Form>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PreviewPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_card: Option<Teaser>,
    /// The copy could not be shaped for this network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What a publish of the dry run's build would plan, each copy shaped.
pub fn for_dry_run(folder: &Path, sealed: &SealedManifest) -> Vec<PreviewCopy> {
    let mp = MossPaths::new(folder);
    let target = crate::build::manifest::backfill::publish_target(&mp);
    let Some(before) = Before::capture(&mp, sealed, target.as_deref()) else { return Vec::new() };
    let Some(live) = read_map(&mp) else { return Vec::new() };
    // Without an address of its own the site's is known only at publish, so
    // the preview's links to it are paths.
    let site = super::landing::canonical(sealed).unwrap_or("").to_string();
    let mut out = Vec::new();
    for plan in before.decide(folder, &live) {
        for entry in plan.entries {
            let post = live.articles.get(&entry.url_path);
            let info = before.channels().get(&entry.channel);
            let mut copy = PreviewCopy {
                source_path: entry.source_path,
                channel: entry.channel,
                action: entry.action.clone(),
                form: None,
                parts: Vec::new(),
                link_card: None,
                error: None,
            };
            if let (Action::Due | Action::Update, Some(post), Some(info)) = (&entry.action, post, info) {
                fill(&mut copy, post, &info.spec, &site);
            }
            out.push(copy);
        }
    }
    out
}

/// One post's copy on each channel it names, shaped, regardless of whether it
/// is due: what the author would send.
pub fn for_post(folder: &Path, source_path: &str, site_url: &str) -> Vec<PreviewCopy> {
    let mp = MossPaths::new(folder);
    let Some(live) = read_map(&mp) else { return Vec::new() };
    let Some(post) = live.articles.values().find(|a| a.source_path == source_path) else { return Vec::new() };
    let channels = super::landing::channels(folder);
    post.syndicate
        .iter()
        .flatten()
        .filter(|c| c.as_str() != super::plan::RESERVED_EMAIL)
        .map(|channel| {
            let mut copy = PreviewCopy {
                source_path: source_path.to_string(),
                channel: channel.clone(),
                action: Action::Due,
                form: None,
                parts: Vec::new(),
                link_card: None,
                error: None,
            };
            match channels.get(channel) {
                Some(ChannelInfo { spec, .. }) => fill(&mut copy, post, spec, site_url),
                None => copy.action = Action::Skip { reason: format!("no channel named {channel} is set up for this site") },
            }
            copy
        })
        .collect()
}

fn fill(copy: &mut PreviewCopy, post: &ArticleInfo, spec: &ChannelSpec, site: &str) {
    match shape(post, spec, site) {
        Ok(shaped) => {
            copy.form = Some(shaped.form);
            copy.parts = shaped
                .parts
                .iter()
                .map(|p| PreviewPart { text: p.text.clone(), chars: spec.length(&p.text), max_chars: spec.max_chars })
                .collect();
            copy.link_card = shaped.teaser;
        }
        Err(e) => copy.error = Some(e),
    }
}

fn read_map(mp: &MossPaths) -> Option<ArticleMap> {
    let bytes = std::fs::read(mp.article_map()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_posts_copies_are_shaped_for_each_channel_it_names_but_the_newsletter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::test_channel(root, "one", "{}");
        let post: ArticleInfo = serde_json::from_value(serde_json::json!({
            "source_path": "essays/autumn.md", "title": "Autumn", "content": "The leaves turn.\n", "url_path": "essays/autumn/",
            "date": null, "uid": "aaaa0001", "syndicate": ["email", "one", "ghost"]
        }))
        .unwrap();
        let mut map = ArticleMap::default();
        map.articles.insert(post.url_path.clone(), post);
        let mp = MossPaths::new(root);
        std::fs::create_dir_all(mp.article_map().parent().unwrap()).unwrap();
        std::fs::write(mp.article_map(), serde_json::to_vec(&map).unwrap()).unwrap();

        let copies = for_post(root, "essays/autumn.md", "https://notes.example.org/");
        let channels: Vec<&str> = copies.iter().map(|c| c.channel.as_str()).collect();
        assert_eq!(channels, ["one", "ghost"], "the newsletter is not a channel");
        assert_eq!(copies[0].form, Some(Form::Teaser));
        assert!(copies[0].parts[0].text.contains("https://notes.example.org/essays/autumn/"), "{:?}", copies[0].parts);
        assert_eq!(copies[0].parts[0].max_chars, Some(300));
        assert!(matches!(&copies[1].action, Action::Skip { reason } if reason.contains("ghost")));
    }
}
