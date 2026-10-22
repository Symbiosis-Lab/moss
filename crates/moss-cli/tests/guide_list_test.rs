//! `moss guide --list` starts with the overview and gives every topic one line
//! that says when to read it, so an agent can choose a topic without opening each.
//! The topics come from the reference files the binary embeds.

use std::process::Command;

fn reference_topics() -> Vec<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../moss-build/src/assets/skills/moss/references");
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .expect("reference directory")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".md").map(str::to_string))
        .collect();
    out.sort();
    out
}

#[test]
fn guide_list_names_the_overview_first_then_each_topic_with_its_reason() {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["guide", "--list"])
        .output()
        .expect("run moss-cli guide --list");
    assert!(out.status.success(), "guide --list failed: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();

    let topics = reference_topics();
    assert!(!topics.is_empty(), "no reference files found");
    assert_eq!(lines.len(), topics.len() + 1, "one overview line plus one per topic:\n{text}");
    assert!(lines[0].starts_with("(no topic)") && lines[0].contains("start here"), "overview first:\n{text}");
    for (line, topic) in lines[1..].iter().zip(&topics) {
        assert!(line.starts_with(topic.as_str()), "expected `{topic}` next:\n{text}");
        let reason = line[topic.len()..].trim();
        assert!(reason.starts_with("read "), "`{topic}` says nothing about when to read it:\n{text}");
        assert!(reason.len() < 100, "`{topic}` reason is {} characters:\n{text}", reason.len());
    }
}
