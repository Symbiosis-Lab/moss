//! Block IR → markdown emission, shared by every import producer.
//!
//! Byte-shape parity with the retired Strikingly `Renderer` is load-bearing:
//! the corpus snapshot gate (`strikingly_tests.rs::corpus_snapshot`) diffs
//! output across the refactor.

use super::blocks::Block;

/// Render blocks to a markdown body: one paragraph-block each, joined by
/// blank lines. Blocks that render empty (e.g. prose whose htmd conversion
/// trims to nothing) are dropped, not emitted as empty paragraphs.
pub(crate) fn emit_markdown(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(emit_block)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn emit_block(b: &Block) -> Option<String> {
    match b {
        Block::RichText { html } => html_to_markdown(html),
        Block::Quote { html } => html_to_markdown(html).map(|md| {
            md.lines()
                .map(|l| {
                    if l.is_empty() {
                        ">".into()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }),
        Block::Image { src, alt } => {
            // Brackets and newlines break the markdown image grammar.
            let alt = alt.replace(['[', ']', '\n'], " ").trim().to_string();
            Some(format!("![{alt}]({src})"))
        }
        Block::Video { url } => Some(format!("![[{url}]]")),
        Block::Audio { src } => Some(format!("![[{src}]]")),
        Block::Button { text, url } => Some(format!("[{text}]({url})")),
        Block::Separator => Some("---".into()),
    }
}

/// HTML → trimmed markdown via htmd; `None` when conversion fails or the
/// result is empty (caller drops the block).
fn html_to_markdown(html: &str) -> Option<String> {
    let md = htmd::convert(html).ok()?;
    let md = md.trim();
    (!md.is_empty()).then(|| md.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn richtext_converts_html_and_drops_empty() {
        let blocks = [
            Block::RichText {
                html: "<p>Hello <b>world</b></p>".into(),
            },
            Block::RichText { html: "   ".into() },
        ];
        assert_eq!(emit_markdown(&blocks), "Hello **world**");
    }

    #[test]
    fn quote_prefixes_every_line_and_marks_empty_lines() {
        let blocks = [Block::Quote {
            html: "<p>one</p><p>two</p>".into(),
        }];
        assert_eq!(emit_markdown(&blocks), "> one\n>\n> two");
    }

    #[test]
    fn image_alt_sanitized_for_markdown_grammar() {
        let blocks = [Block::Image {
            src: "https://cdn.example.com/a.jpg".into(),
            alt: "a [bracketed]\ncaption ".into(),
        }];
        assert_eq!(
            emit_markdown(&blocks),
            "![a  bracketed  caption](https://cdn.example.com/a.jpg)"
        );
    }

    #[test]
    fn video_button_separator_shapes() {
        let blocks = [
            Block::Video {
                url: "https://youtu.be/x123".into(),
            },
            Block::Button {
                text: "Apply".into(),
                url: "/submit".into(),
            },
            Block::Separator,
        ];
        assert_eq!(
            emit_markdown(&blocks),
            "![[https://youtu.be/x123]]\n\n[Apply](/submit)\n\n---"
        );
    }
}
