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
        Block::Image { src, alt } => Some(image_line(src, alt)),
        Block::Gallery { images } => (!images.is_empty()).then(|| {
            let lines: Vec<_> = images.iter().map(|(src, alt)| image_line(src, alt)).collect();
            format!(":::gallery\n{}\n:::", lines.join("\n"))
        }),
        Block::Video { url } => Some(format!("![[{url}]]")),
        Block::Audio { src } => Some(format!("![[{src}]]")),
        Block::Button { text, url } => Some(format!("[{text}]({url})")),
        Block::Separator => Some("---".into()),
    }
}

pub(crate) fn image_line(src: &str, alt: &str) -> String {
    // Brackets and newlines break the markdown image grammar.
    let alt = describing_alt(alt).replace(['[', ']', '\n'], " ").trim().to_string();
    // Parentheses and spaces break the link destination.
    let src = src.replace('(', "\\(").replace(')', "\\)");
    let src = if src.contains(' ') { format!("<{src}>") } else { src };
    format!("![{alt}]({src})")
}

/// The alt text if it describes the image, else empty. moss renders bracket
/// text with no separate description as the image's visible caption, and a
/// file name ("IMG_0042.jpg", "My+Photo+2015.jpg") is not a description, so
/// it would show as text under the photo. Every path that writes image
/// markdown goes through here.
pub(crate) fn describing_alt(alt: &str) -> &str {
    const EXTENSIONS: &[&str] = &[
        ".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif", ".tif", ".tiff",
    ];
    let trimmed = alt.trim();
    let lower = trimmed.to_ascii_lowercase();
    if EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        ""
    } else {
        alt
    }
}

/// HTML → trimmed markdown via the importer's htmd configuration; `None`
/// when conversion fails or the result is empty (caller drops the block).
fn html_to_markdown(html: &str) -> Option<String> {
    let md = super::widgets::html_to_markdown(html).ok()?;
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
    fn filename_alt_is_emptied_and_a_real_alt_kept() {
        let image = |alt: &str| Block::Image {
            src: "a.jpg".into(),
            alt: alt.into(),
        };
        assert_eq!(emit_markdown(&[image("My+Photo+2015.JPG")]), "![](a.jpg)");
        assert_eq!(emit_markdown(&[image(" scan 3.tiff ")]), "![](a.jpg)");
        assert_eq!(
            emit_markdown(&[image("A harbour at dusk")]),
            "![A harbour at dusk](a.jpg)"
        );
    }

    #[test]
    fn gallery_is_one_fence_with_an_image_per_line() {
        let blocks = [Block::Gallery {
            images: vec![
                ("a.jpg".into(), "IMG_1.jpg".into()),
                ("b.jpg".into(), "Second".into()),
            ],
        }];
        assert_eq!(
            emit_markdown(&blocks),
            ":::gallery\n![](a.jpg)\n![Second](b.jpg)\n:::"
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
