//! `moss guide [topic]` — print the agent guidance this binary ships.
//!
//! The guidance used to be copied onto disk, into `.claude/skills/moss/` and
//! friends. It no longer is: those files name this command instead, so the
//! prose has exactly one home and it is the binary the reader is running. See
//! [`crate::cli::agents::skill_package::render_pointer`] for the trade that
//! makes, and [`crate::cli::agents::sync`] for what the files now contain.
//!
//! Needs no project and no network — it reads nothing but itself, which is what
//! lets an agent orient in a folder it has not built yet.

use super::agents::skill_package;

/// Returns an exit code.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--list") | Some("-l") => {
            for topic in skill_package::topics() {
                println!("{topic}");
            }
            0
        }
        Some(flag) if flag.starts_with('-') => {
            eprintln!("unknown flag: {flag}");
            eprintln!("usage: moss guide [<topic>|all|--list]");
            1
        }
        topic => match skill_package::guide_text(topic) {
            Some(text) => {
                println!("{text}");
                0
            }
            // Naming the topics beats "unknown topic": the caller is one word
            // away from what they wanted, and `--list` is a round trip they do
            // not need to spend.
            None => {
                eprintln!(
                    "unknown guide topic: {}\n\navailable: {}, all",
                    topic.unwrap_or(""),
                    skill_package::topics().join(", ")
                );
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_point_is_the_skill_body_without_its_frontmatter() {
        let text = skill_package::guide_text(None).expect("entry point");
        assert!(!text.starts_with("---"), "frontmatter leaked into `moss guide`");
        assert!(text.contains("moss describe"), "entry point lost its content");
    }

    /// Every topic the pointer advertises has to resolve, or the file moss
    /// writes tells an agent to run a command that fails. Both sides derive
    /// from the same directory listing, so this pins that they keep doing so.
    #[test]
    fn every_advertised_topic_resolves() {
        let topics = skill_package::topics();
        assert!(topics.len() >= 4, "topic discovery looks broken: {topics:?}");
        for topic in &topics {
            let text = skill_package::guide_text(Some(topic))
                .unwrap_or_else(|| panic!("`moss guide {topic}` resolves to nothing"));
            assert!(!text.trim().is_empty(), "`moss guide {topic}` is empty");
        }
        let pointer = skill_package::render_pointer();
        for topic in &topics {
            assert!(
                pointer.contains(&format!("`{topic}`")),
                "the written pointer never names the `{topic}` topic"
            );
        }
    }

    #[test]
    fn an_unknown_topic_is_an_error_not_an_empty_page() {
        assert!(skill_package::guide_text(Some("nonexistent")).is_none());
        assert_eq!(run(&["nonexistent".to_string()]), 1);
    }

    #[test]
    fn all_is_the_whole_package() {
        let all = skill_package::guide_text(Some("all")).expect("all");
        for topic in skill_package::topics() {
            let body = skill_package::guide_text(Some(&topic)).expect("topic");
            let first_line = body.lines().find(|l| !l.trim().is_empty()).expect("heading");
            assert!(all.contains(first_line), "`all` is missing {topic}");
        }
    }
}
