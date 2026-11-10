//! A post's channel chips against what a real build resolved: the walk over a
//! page's folders has to land where the cascade did.

#[path = "support/site.rs"]
mod site;

use moss_build::build::scan::article_map::ArticleMap;
use moss_build::syndication::choose::read_post_with;
use moss_build::syndication::plan::ChannelInfo;
use site::Site;
use std::collections::BTreeMap;

const BODY: &str = "date: 2026-01-01\n---\n\nWords.\n";

fn post(front: &str) -> String {
    format!("---\ntitle: Post\n{front}{BODY}")
}

fn folder(cascade: &str) -> String {
    format!("---\ntitle: Folder\ncascade:\n  syndicate: {cascade}\n---\n")
}

#[test]
fn every_chip_agrees_with_the_cascade_the_build_applied() {
    let site = Site::new(&[
        ("index.md", &folder("[a]")),
        ("top.md", &post("")),
        ("blog/index.md", &folder("[b]")),
        ("blog/nearest.md", &post("")),
        ("blog/own.md", &post("syndicate: [c]\n")),
        ("blog/off.md", &post("syndicate: false\n")),
        ("blog/invalid.md", &post("syndicate: 3\n")),
        ("blog/deep/index.md", "---\ntitle: Deep\n---\n"),
        ("blog/deep/under.md", &post("")),
        ("closed/index.md", &folder("false")),
        ("closed/post.md", &post("")),
        ("broken/index.md", &folder("3")),
        ("broken/post.md", &post("")),
        ("笔记/index.md", &folder("[c]")),
        ("笔记/文章.md", &post("")),
        ("moved/index.md", "---\ntitle: Moved\nurl: elsewhere\ncascade:\n  syndicate: [b]\n---\n"),
        ("moved/inside.md", &post("")),
        ("blog/nearest.zh-hans.md", &post("")),
    ]);
    let map = ArticleMap::load(&site.root.join(".moss")).unwrap();
    assert!(map.articles.len() >= 10, "{:?}", map.articles.keys());
    let channels: BTreeMap<String, ChannelInfo> = ["a", "b", "c"].iter().map(|c| (c.to_string(), ChannelInfo::default())).collect();
    for post in map.articles.values() {
        let chips = read_post_with(&site.root, &post.source_path, &channels).unwrap_or_else(|| panic!("no chips for {}", post.source_path));
        let resolved = post.syndicate.clone().unwrap_or_default();
        for chip in chips {
            assert_eq!(chip.on, resolved.contains(&chip.channel), "{} ({}) on {}: built {resolved:?}", post.source_path, post.url_path, chip.channel);
        }
    }
}
