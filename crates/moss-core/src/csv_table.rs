//! Pure CSV/TSV renderer: a table, or a list of rows.
//!
//! No I/O: caller supplies the file content as a string. Called from
//! moss-build's marker resolver for `![[data.csv]]`/`![[data.tsv]]` embeds
//! (the `moss-embed-table:` deferred marker `resolve.rs`'s pre-pass emits).

/// How a data file is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Style {
    #[default]
    Table,
    List,
}

const KNOWN_KEYS: &[&str] = &["style", "where"];

/// What the text after an embed's `|` asks of a data file:
/// `![[books.csv|style:list,where:shelf=Poetry]]`.
///
/// A `|` segment naming `style:` or `where:` is options, split on `,`;
/// any other segment is a caption and is ignored, as a folder embed reads
/// its own. A value keeps its spaces (`where:group=His voice`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataOptions {
    pub style: Style,
    /// Every `where:<column>=<value>`; a row is shown only when it matches all.
    pub filters: Vec<(String, String)>,
    /// What the build should warn about: unknown keys and malformed values.
    pub problems: Vec<String>,
}

impl DataOptions {
    pub fn parse(pothole: &str) -> Self {
        let mut out = Self::default();
        let segments = pothole.split('|').filter(|s| crate::resolve::embed_renderer::folder_list::has_known_key(s, KNOWN_KEYS));
        for tok in segments.flat_map(|s| s.split(',')).map(str::trim).filter(|t| !t.is_empty()) {
            let Some((key, value)) = tok.split_once(':').or_else(|| tok.split_once('=')) else {
                continue;
            };
            let value = value.trim();
            match (key.trim(), value) {
                ("style", "table") => out.style = Style::Table,
                ("style", "list") => out.style = Style::List,
                ("style", other) => out.problems.push(format!("ignores `style:{other}`; a data file is drawn as `style:table` or `style:list`")),
                ("where", _) => match value.split_once('=') {
                    Some((column, wanted)) => out.filters.push((column.trim().to_string(), wanted.trim().to_string())),
                    None => out.problems.push(format!("ignores `where:{value}`; write `where:<column>=<value>`")),
                },
                (other, _) => out.problems.push(format!("ignores unknown option `{other}`; known options: style:table, style:list, where:<column>=<value>")),
            }
        }
        out
    }
}

/// The embed's output, and what the author should be told about it.
#[derive(Debug, Default)]
pub struct Rendered {
    /// HTML for the table form; markdown for the list form, which the page's
    /// own markdown pass renders, so cells may hold links and emphasis.
    pub text: String,
    pub warnings: Vec<String>,
}

/// Render a CSV/TSV string as a table (cells HTML-escaped) or, with
/// `style:list`, as a markdown bulleted list: one item per row, its `title`
/// cell (the first column when there is none) linked to its `link` cell and
/// followed by `item_separator` and its `description` cell.
///
/// The table is wrapped in the same `.moss-table-scroll` container the
/// Markdown-table renderer uses (`ast::render`), so every moss table scrolls
/// sideways the same accessible way.
pub fn render(content: &str, separator: char, options: &DataOptions, item_separator: &str) -> Rendered {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut rows = parse_rows(content, separator);
    let mut warnings = options.problems.clone();
    let header: Vec<String> = if rows.is_empty() { Vec::new() } else { rows.remove(0) };
    let column = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let mut wanted = Vec::with_capacity(options.filters.len());
    let mut missing_column = false;
    for (name, value) in &options.filters {
        match column(name) {
            Some(i) => wanted.push((i, value.as_str())),
            None => {
                warnings.push(format!("has no column `{name}` for `where:{name}={value}`, so it shows no rows"));
                missing_column = true;
            }
        }
    }
    if missing_column {
        rows.clear();
    } else if !wanted.is_empty() {
        rows.retain(|row| wanted.iter().all(|(i, v)| row.get(*i).is_some_and(|cell| cell.trim() == *v)));
        if rows.is_empty() {
            warnings.push("shows no rows: none matches its `where:` filters".to_string());
        }
    } else if rows.is_empty() && options.style == Style::List {
        warnings.push("shows no rows: the file has none below its header".to_string());
    }
    let text = if options.style == Style::List {
        let title = column("title").unwrap_or(0);
        render_list(&rows, title, column("link"), column("description"), item_separator)
    } else {
        format!("<div class=\"moss-table-scroll\" tabindex=\"0\">{}</div>", build_table(&header, &rows))
    };
    Rendered { text, warnings }
}

fn render_list(rows: &[Vec<String>], title: usize, link: Option<usize>, description: Option<usize>, item_separator: &str) -> String {
    let cell = |row: &Vec<String>, i: Option<usize>| -> String {
        // Embeds are lowered before this runs, so `![…]` in a cell is shown as text.
        i.and_then(|i| row.get(i)).map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ").replace("![", "\\![")).unwrap_or_default()
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let title = cell(row, Some(title));
        let link = cell(row, link);
        let description = cell(row, description);
        let head = match (title.is_empty(), link.is_empty()) {
            (false, false) => format!("[{}](<{}>)", link_label(&title), link.replace('\\', "%5C").replace('<', "%3C").replace('>', "%3E")),
            (false, true) => title,
            (true, _) => String::new(),
        };
        let item = match (head.is_empty(), description.is_empty()) {
            (true, true) => continue,
            (false, true) => head,
            (true, false) => description,
            (false, false) => format!("{head}{item_separator}{description}"),
        };
        items.push(format!("- {}", escape_block_start(&item)));
    }
    if items.is_empty() {
        return String::new();
    }
    format!("\n\n{}\n\n", items.join("\n"))
}

/// `title` as link text: an unescaped bracket would close or reopen the text,
/// and a trailing backslash would escape the closing `]`. Markdown the title
/// carries otherwise (emphasis, an escaped bracket) is kept.
fn link_label(title: &str) -> String {
    let mut out = String::with_capacity(title.len() + 2);
    let mut escaped = false;
    for c in title.chars() {
        if !escaped && (c == '[' || c == ']') {
            out.push('\\');
        }
        escaped = !escaped && c == '\\';
        out.push(c);
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Keep a cell's opening characters from being read as a heading, quote,
/// nested list, rule or code fence inside its list item.
fn escape_block_start(item: &str) -> String {
    let bytes = item.as_bytes();
    let run = |c: u8| bytes.iter().take_while(|&&b| b == c).count();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    let is_rule = item.len() >= 3 && item.bytes().all(|b| matches!(b, b'-' | b'*' | b'_' | b' ')) && item.trim().len() >= 3;
    let opens_block = match bytes.first() {
        Some(b'#' | b'>') => true,
        Some(&c @ (b'`' | b'~')) => run(c) >= 3,
        Some(b'-' | b'+' | b'*') => bytes.get(1).map_or(true, |b| b.is_ascii_whitespace()) || is_rule,
        Some(b'_') => is_rule,
        Some(b'0'..=b'9') => matches!(bytes.get(digits), Some(b'.' | b')')),
        _ => false,
    };
    if !opens_block {
        return item.to_string();
    }
    let (head, rest) = item.split_at_checked(digits).unwrap_or(("", item));
    format!("{head}\\{rest}")
}

fn parse_rows(content: &str, sep: char) -> Vec<Vec<String>> {
    // Minimal CSV parser: handles quoted fields with embedded commas, newlines,
    // and escaped quotes (`""` → `"`). Not a full RFC 4180 parser but correct
    // for well-formed authored data. Upgrade to the `csv` crate if needed.
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        match (c, in_quotes) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            ('"', true) => in_quotes = false,
            ('"', false) => in_quotes = true,
            (c, true) => field.push(c),
            (c, false) if c == sep => {
                row.push(std::mem::take(&mut field));
            }
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            ('\r', false) => { /* swallow; CRLF handled by \n branch */ }
            (c, false) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn build_table(header: &[String], rows: &[Vec<String>]) -> String {
    use crate::media::html_escape_text as esc;
    let open = format!("<table class=\"{}\" data-type=\"table\">", crate::resolve::embed_renderer::CLASS_EMBED);
    if header.is_empty() {
        return format!("{open}</table>");
    }
    let mut out = open;
    out.push_str("<thead><tr>");
    for cell in header {
        out.push_str(&format!("<th>{}</th>", esc(cell)));
    }
    out.push_str("</tr></thead><tbody>");
    for row in rows {
        out.push_str("<tr>");
        for cell in row {
            out.push_str(&format!("<td>{}</td>", esc(cell)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(csv: &str) -> String {
        render(csv, ',', &DataOptions::default(), ", ").text
    }

    fn with(csv: &str, pothole: &str) -> Rendered {
        render(csv, ',', &DataOptions::parse(pothole), ", ")
    }

    #[test]
    fn test_csv_basic() {
        let out = table("name,age\nAlice,30\nBob,25\n");
        assert!(out.contains("<thead><tr><th>name</th><th>age</th></tr></thead>"));
        assert!(out.contains("<td>Alice</td><td>30</td>"));
        assert!(out.contains("<td>Bob</td><td>25</td>"));
        assert!(out.contains("class=\"moss-embed\" data-type=\"table\""));
    }

    #[test]
    fn test_csv_quoted_field_with_comma() {
        let out = table("name,note\n\"Smith, J\",\"hi, world\"\n");
        assert!(out.contains("<td>Smith, J</td>"), "got: {}", out);
        assert!(out.contains("<td>hi, world</td>"), "got: {}", out);
    }

    #[test]
    fn test_csv_escaped_quote() {
        let out = table("name\n\"he said \"\"hi\"\"\"\n");
        assert!(out.contains("<td>he said \"hi\"</td>"), "got: {}", out);
    }

    #[test]
    fn test_csv_html_escape() {
        let out = table("html\n<script>alert(1)</script>\n");
        assert!(!out.contains("<script>"), "raw script leaked: {}", out);
        assert!(out.contains("&lt;script&gt;"), "got: {}", out);
    }

    #[test]
    fn test_tsv_tab_separator() {
        let out = render("a\tb\n1\t2\n", '\t', &DataOptions::default(), ", ").text;
        assert!(out.contains("<th>a</th>"));
        assert!(out.contains("<th>b</th>"));
        assert!(out.contains("<td>1</td><td>2</td>"));
    }

    #[test]
    fn test_csv_empty() {
        let out = table("");
        assert!(out.starts_with("<div class=\"moss-table-scroll\" tabindex=\"0\">"), "got: {}", out);
        assert!(out.ends_with("</table></div>"), "got: {}", out);
    }

    #[test]
    fn test_csv_wrapped_in_scroll_container() {
        let out = table("name,age\nAlice,30\n");
        assert!(out.starts_with("<div class=\"moss-table-scroll\" tabindex=\"0\">"), "CSV embed must share the scroll wrapper: {out}");
        assert!(out.ends_with("</div>"), "got: {}", out);
        assert!(out.contains("class=\"moss-embed\" data-type=\"table\""), "{out}");
    }

    #[test]
    fn test_csv_crlf_line_endings() {
        let out = table("a,b\r\n1,2\r\n");
        assert!(out.contains("<th>a</th>"), "got: {}", out);
        assert!(out.contains("<td>1</td><td>2</td>"), "got: {}", out);
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_column_name() {
        let out = with("\u{feff}title,link\nA,https://a.example/\n", "style:list");
        assert_eq!(out.text, "\n\n- [A](<https://a.example/>)\n\n");
    }

    #[test]
    fn a_bracket_or_trailing_backslash_in_a_title_stays_inside_its_link() {
        let item = |title: &str, link: &str| with(&format!("title,link\n\"{title}\",{link}\n"), "style:list").text;
        assert_eq!(item("A] B", "https://a.example/"), "\n\n- [A\\] B](<https://a.example/>)\n\n");
        assert_eq!(item("[Draft] notes", "https://a.example/"), "\n\n- [\\[Draft\\] notes](<https://a.example/>)\n\n");
        assert_eq!(item("C:\\", "https://a.example/"), "\n\n- [C:\\\\](<https://a.example/>)\n\n");
        assert_eq!(item("A\\] B", "https://a.example/"), "\n\n- [A\\] B](<https://a.example/>)\n\n");
        assert_eq!(item("Wiki", "https://w.example/a_(b)"), "\n\n- [Wiki](<https://w.example/a_(b)>)\n\n");
        // And a markdown parser reads each as one link whose text is the title.
        for (title, text) in [("A] B", "A] B"), ("[Draft] notes", "[Draft] notes"), ("C:\\", "C:\\")] {
            use pulldown_cmark::{Event, Parser, Tag, TagEnd};
            let md = item(title, "https://a.example/");
            let events: Vec<Event> = Parser::new(&md).collect();
            let links = events.iter().filter(|e| matches!(e, Event::Start(Tag::Link { .. }))).count();
            let open = events.iter().position(|e| matches!(e, Event::Start(Tag::Link { .. }))).unwrap_or(0);
            let close = events.iter().position(|e| matches!(e, Event::End(TagEnd::Link))).unwrap_or(0);
            let mut seen = String::new();
            for event in &events[open..close] {
                if let Event::Text(t) = event {
                    seen.push_str(t);
                }
            }
            assert_eq!((links, seen.as_str()), (1, text), "{md}");
        }
    }

    const BOOKS: &str = "title,link,description,shelf\n\
        *Leaves*,https://a.example/leaves,\"A [review](https://r.example/) of it\",Poetry\n\
        Atlas,,No copy online,Maps\n\
        Odes,https://a.example/odes,,Poetry\n";

    #[test]
    fn a_list_is_one_markdown_item_per_row_in_file_order() {
        let out = with(BOOKS, "style:list");
        assert_eq!(
            out.text,
            "\n\n- [*Leaves*](<https://a.example/leaves>), A [review](https://r.example/) of it\n\
             - Atlas, No copy online\n\
             - [Odes](<https://a.example/odes>)\n\n"
        );
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    #[test]
    fn where_keeps_only_matching_rows_in_a_list_and_a_table() {
        let list = with(BOOKS, "style:list,where:shelf=Poetry");
        assert!(list.text.contains("Leaves") && list.text.contains("Odes"), "{}", list.text);
        assert!(!list.text.contains("Atlas"), "{}", list.text);
        let table = with(BOOKS, "where:shelf=Maps").text;
        assert!(table.contains("<td>Atlas</td>") && !table.contains("Odes"), "{table}");
    }

    #[test]
    fn where_filters_combine_and_values_may_hold_spaces() {
        let csv = "title,group,lang\nA,His voice,en\nB,His voice,de\nC,Histories,en\n";
        let out = with(csv, "style:list|where:group=His voice|where:lang=en");
        assert_eq!(out.text, "\n\n- A\n\n");
    }

    #[test]
    fn a_filter_that_matches_nothing_or_names_no_column_warns() {
        let none = with(BOOKS, "style:list,where:shelf=Plays");
        assert_eq!(none.text, "");
        assert_eq!(none.warnings.len(), 1, "{:?}", none.warnings);
        let typo = with(BOOKS, "style:list,where:shelff=Poetry");
        assert_eq!(typo.text, "");
        assert!(typo.warnings[0].contains("no column `shelff`"), "{:?}", typo.warnings);
    }

    #[test]
    fn a_caption_is_ignored_even_with_a_colon_and_options_beside_it_still_apply() {
        for caption in ["Sales figures", "Note: preliminary", "Q1=draft"] {
            let out = with(BOOKS, caption);
            assert!(out.text.contains("<table") && out.warnings.is_empty(), "{caption}: {:?}", out.warnings);
        }
        let out = with(BOOKS, "Note: preliminary|style:list");
        assert!(out.text.starts_with("\n\n- "), "{}", out.text);
    }

    #[test]
    fn unknown_keys_bad_styles_and_bad_filters_beside_a_known_key_warn() {
        let typo = with(BOOKS, "style:list,stlye:grid");
        assert!(typo.warnings.iter().any(|w| w.contains("`stlye`")), "{:?}", typo.warnings);
        assert!(with(BOOKS, "style:cards").warnings[0].contains("style:cards"));
        assert!(with(BOOKS, "where:shelf").warnings[0].contains("where:<column>=<value>"));
    }

    #[test]
    fn a_list_of_a_file_with_no_rows_warns() {
        let out = with("title,link\n", "style:list");
        assert_eq!(out.text, "");
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    }

    #[test]
    fn without_a_title_column_the_first_column_titles_each_item() {
        let out = with("name,description\nAda,Engineer\n", "style:list");
        assert_eq!(out.text, "\n\n- Ada, Engineer\n\n");
    }

    #[test]
    fn a_cell_cannot_open_a_block_or_break_its_item() {
        let csv = "title\n# Not a heading\n1. Not a number\n- dash\n\"two\nlines\"\n*Emphasis*\n```fence\n---\n`code` first\n~~gone~~\nSee ![[other.csv]] and ![](a.csv)\n";
        let out = with(csv, "style:list").text;
        assert_eq!(
            out,
            "\n\n- \\# Not a heading\n- 1\\. Not a number\n- \\- dash\n- two lines\n- *Emphasis*\n- \\```fence\n- \\---\n- `code` first\n- ~~gone~~\n- See \\![[other.csv]] and \\![](a.csv)\n\n"
        );
    }

    #[test]
    fn a_link_cell_with_spaces_or_angle_brackets_stays_one_link() {
        let out = with("title,link\nA,docs/my file <1>.pdf\\\n", "style:list").text;
        assert_eq!(out, "\n\n- [A](<docs/my file %3C1%3E.pdf%5C>)\n\n");
    }
}
