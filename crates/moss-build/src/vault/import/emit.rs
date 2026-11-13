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
    let alt = describing_alt(alt, src).replace(['[', ']', '\n'], " ").trim().to_string();
    // Parentheses and spaces break the link destination.
    let src = src.replace('(', "\\(").replace(')', "\\)");
    let src = if src.contains(' ') { format!("<{src}>") } else { src };
    format!("![{alt}]({src})")
}

/// The alt text if it describes the image, else empty. moss renders bracket
/// text with no separate description as the image's visible caption, and a
/// file name ("IMG_0042.jpg", "My+Photo+2015.jpg"), or the picture's own file
/// name spelled without its extension ("DSC_0042", "harbour-lantern", a media
/// library's default), is not a description, so it would show as text under
/// the photo. A plain word that happens to be the file's name ("Poster" for
/// `poster.png`) stays. Every path that writes image markdown goes through here.
pub(crate) fn describing_alt<'a>(alt: &'a str, src: &str) -> &'a str {
    const EXTENSIONS: &[&str] = &[
        ".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif", ".tif", ".tiff",
    ];
    let trimmed = alt.trim();
    let lower = trimmed.to_ascii_lowercase();
    let spelled_as_a_file = !lower.contains(char::is_whitespace) && lower.contains(|c: char| c.is_ascii_digit() || "-_+".contains(c));
    if EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) || (spelled_as_a_file && file_stems(src).contains(&lower)) {
        ""
    } else {
        alt
    }
}

/// The last path segment of `src` without its extension or query, decoded
/// and lowercased, as written and without a media library's size suffix
/// (`-1024x768`, `-scaled`).
fn file_stems(src: &str) -> [String; 2] {
    let path = src.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    let name = urlencoding::decode(name).map(|n| n.into_owned()).unwrap_or_else(|_| name.to_string()).to_lowercase();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
    let bare = stem.strip_suffix("-scaled").unwrap_or(stem);
    let bare = match bare.rsplit_once('-') {
        Some((rest, size)) if size.split_once('x').is_some_and(|(w, h)| !w.is_empty() && !h.is_empty() && w.chars().chain(h.chars()).all(|c| c.is_ascii_digit())) => rest,
        _ => bare,
    };
    [stem.to_string(), bare.to_string()]
}

/// Builder residue a reader never sees as content: a spacer paragraph (only
/// spaces, non-breaking spaces, zero-width spaces or a lone hard break)
/// becomes a blank line, a run of blank lines becomes one, and a strip of
/// pictures repeated for a looping marquee keeps one copy. Code blocks stay
/// as written.
pub(crate) fn tidy(markdown: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut strip: Vec<&str> = Vec::new();
    let mut fence: Option<char> = None;
    for line in markdown.split('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| (*c == '`' || *c == '~') && trimmed.starts_with(&c.to_string().repeat(3)));
        let code = fence.is_some() || marker.is_some();
        match (fence, marker) {
            (None, Some(c)) => fence = Some(c),
            (Some(open), Some(c)) if open == c => fence = None,
            _ => {}
        }
        let blank = !code && is_spacer(line);
        if !code && (is_picture_line(line) || (blank && !strip.is_empty())) {
            if !(blank && strip.last().is_some_and(|l| l.is_empty())) { strip.push(if blank { "" } else { line }); }
            continue;
        }
        flush(&mut strip, &mut out);
        if blank && out.last().is_some_and(|l| l.is_empty()) { continue; }
        out.push(if blank { String::new() } else { line.to_string() });
    }
    flush(&mut strip, &mut out);
    out.join("\n")
}

fn is_spacer(line: &str) -> bool {
    let t = line.trim_matches(|c: char| c.is_whitespace() || c == '\u{200b}' || c == '\u{feff}');
    t.is_empty() || t == "\\" || t == "&nbsp;"
}

static PICTURE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"!\[[^\]]*\]\([^)\s]*\)").expect("picture regex"));

fn is_picture_line(line: &str) -> bool {
    PICTURE.is_match(line) && PICTURE.replace_all(line, "").trim().is_empty()
}

/// The `![alt](src)` tokens of a line, in order.
fn pictures(line: &str) -> Vec<&str> {
    PICTURE.find_iter(line).map(|m| m.as_str()).collect()
}

/// Move a run of picture lines (single blank lines between them allowed) to
/// `out`, once when the whole run is one sequence of pictures repeated at
/// least twice.
fn flush(strip: &mut Vec<&str>, out: &mut Vec<String>) {
    let trailing = strip.last().is_some_and(|l| l.is_empty());
    let run = &strip[..strip.len() - usize::from(trailing)];
    let tokens: Vec<&str> = run.iter().flat_map(|l| pictures(l)).collect();
    let period = (1..=tokens.len() / 2).find(|&p| tokens.len() % p == 0 && (p..tokens.len()).all(|i| tokens[i] == tokens[i % p]));
    let mut kept = 0;
    for line in run {
        let found = pictures(line);
        match period {
            Some(p) if kept >= p => break,
            // The first copy ends inside this line: keep its first pictures.
            Some(p) if kept + found.len() > p => out.push(found[..p - kept].join(" ")),
            _ => out.push(line.to_string()),
        }
        kept += found.len();
    }
    if trailing && out.last().is_some_and(|l| !l.is_empty()) { out.push(String::new()); }
    strip.clear();
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
    fn an_alt_that_is_the_pictures_own_file_name_is_emptied() {
        let line = |src: &str, alt: &str| image_line(src, alt);
        assert_eq!(line("https://x.example/wp-content/uploads/2024/03/harbour-lantern-1024x683.jpg", "harbour-lantern"), "![](https://x.example/wp-content/uploads/2024/03/harbour-lantern-1024x683.jpg)");
        assert_eq!(line("/uploads/DSC_0042-scaled.jpeg?w=800", "DSC_0042"), "![](/uploads/DSC_0042-scaled.jpeg?w=800)");
        assert_eq!(line("/uploads/harbour.jpg", "The harbour at dusk"), "![The harbour at dusk](/uploads/harbour.jpg)");
        assert_eq!(line("/uploads/harbour.jpg", "Harbour"), "![Harbour](/uploads/harbour.jpg)");
        assert_eq!(line("/uploads/harbour-lantern.jpg", "Harbour lantern"), "![Harbour lantern](/uploads/harbour-lantern.jpg)");
    }

    #[test]
    fn spacer_paragraphs_collapse_to_one_blank_line_outside_code() {
        let md = "First.\n\n\u{a0}\n\n\u{200b}\n\n\\\n\n&nbsp;\n\nSecond.\n\n```\n\u{a0}\n\n\n```";
        assert_eq!(tidy(md), "First.\n\nSecond.\n\n```\n\u{a0}\n\n\n```");
    }

    #[test]
    fn a_marquees_repeated_pictures_are_written_once() {
        let strip = "![](a.jpg)\n\n![](b.jpg)\n\n![](a.jpg)\n\n![](b.jpg)\n\n![](a.jpg)\n\n![](b.jpg)";
        assert_eq!(tidy(&format!("Intro.\n\n{strip}\n\nAfter.")), "Intro.\n\n![](a.jpg)\n\n![](b.jpg)\n\nAfter.");
        assert_eq!(tidy("![](a.jpg)![](b.jpg)![](a.jpg)![](b.jpg)"), "![](a.jpg) ![](b.jpg)");
        assert_eq!(tidy("![A kiln](a.jpg)\n\n![A kiln](a.jpg)"), "![A kiln](a.jpg)");
        // Not a whole repetition: every picture stays.
        for kept in ["![](a.jpg)\n\n![](b.jpg)\n\n![](a.jpg)", "![](a.jpg)\n\n![](b.jpg)\n\nText\n\n![](a.jpg)\n\n![](b.jpg)"] {
            assert_eq!(tidy(kept), kept);
        }
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
