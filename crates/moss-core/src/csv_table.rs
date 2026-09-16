//! Pure CSV/TSV → HTML table renderer.
//!
//! No I/O: caller supplies the file content as a string. Called from
//! src-tauri's Deferred-marker resolver for `![[data.csv]]` embeds via
//! [`TableRenderer`](crate::resolve::embed_renderer::TableRenderer).

/// Options controlling how a CSV/TSV payload is rendered as an HTML table.
pub struct CsvTableOptions {
    /// Column separator: `,` for CSV, `\t` for TSV.
    pub separator: char,
    /// Treat the first row as `<th>` headers.
    pub has_header: bool,
    /// Optional caption rendered inside a `<caption>` element.
    pub caption: Option<String>,
    /// CSS class applied to the wrapping `<table>` element.
    pub class: String,
    /// Optional `data-type` attribute applied to the wrapping `<table>` element
    /// (v1 vocabulary: `.moss-embed[data-type="table"]`).
    pub data_type: Option<String>,
}

/// Render a CSV/TSV string as HTML. Caller supplies options; content is
/// HTML-escaped before emission.
///
/// The table is wrapped in the same `.moss-table-scroll` container the
/// Markdown-table renderer uses (`ast::render`). CSV/TSV embeds are the
/// data-heavy path, so they get the identical accessible horizontal-scroll
/// affordance (keyboard-focusable, no page overflow on a narrow viewport) and
/// share the default table styling — one consistent look for every moss table.
pub fn render(content: &str, options: &CsvTableOptions) -> String {
    let rows = parse_rows(content, options.separator);
    let table = build_table(&rows, options);
    format!("<div class=\"moss-table-scroll\" tabindex=\"0\">{table}</div>")
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

fn build_table(rows: &[Vec<String>], opts: &CsvTableOptions) -> String {
    let data_type_attr = opts.data_type.as_deref()
        .map(|t| format!(" data-type=\"{}\"", t))
        .unwrap_or_default();
    if rows.is_empty() {
        return format!("<table class=\"{}\"{}></table>", opts.class, data_type_attr);
    }
    let mut out = String::new();
    out.push_str(&format!("<table class=\"{}\"{}>", opts.class, data_type_attr));
    if let Some(cap) = &opts.caption {
        out.push_str(&format!("<caption>{}</caption>", escape(cap)));
    }
    let (header, body) = if opts.has_header {
        (Some(&rows[0]), &rows[1..])
    } else {
        (None, rows)
    };
    if let Some(h) = header {
        out.push_str("<thead><tr>");
        for cell in h {
            out.push_str(&format!("<th>{}</th>", escape(cell)));
        }
        out.push_str("</tr></thead>");
    }
    out.push_str("<tbody>");
    for row in body {
        out.push_str("<tr>");
        for cell in row {
            out.push_str(&format!("<td>{}</td>", escape(cell)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_default() -> CsvTableOptions {
        CsvTableOptions {
            separator: ',',
            has_header: true,
            caption: None,
            class: "moss-embed".to_string(),
            data_type: Some("table".to_string()),
        }
    }

    #[test]
    fn test_csv_basic() {
        let csv = "name,age\nAlice,30\nBob,25\n";
        let out = render(csv, &opts_default());
        assert!(out.contains("<thead><tr><th>name</th><th>age</th></tr></thead>"));
        assert!(out.contains("<td>Alice</td><td>30</td>"));
        assert!(out.contains("<td>Bob</td><td>25</td>"));
        assert!(out.contains("class=\"moss-embed\" data-type=\"table\""));
    }

    #[test]
    fn test_csv_no_header() {
        let csv = "a,1\nb,2\n";
        let opts = CsvTableOptions {
            has_header: false,
            ..opts_default()
        };
        let out = render(csv, &opts);
        assert!(!out.contains("<thead>"), "got: {}", out);
        assert!(out.contains("<td>a</td>"));
    }

    #[test]
    fn test_csv_quoted_field_with_comma() {
        let csv = "name,note\n\"Smith, J\",\"hi, world\"\n";
        let out = render(csv, &opts_default());
        assert!(out.contains("<td>Smith, J</td>"), "got: {}", out);
        assert!(out.contains("<td>hi, world</td>"), "got: {}", out);
    }

    #[test]
    fn test_csv_escaped_quote() {
        let csv = "name\n\"he said \"\"hi\"\"\"\n";
        let out = render(csv, &opts_default());
        assert!(out.contains("<td>he said \"hi\"</td>"), "got: {}", out);
    }

    #[test]
    fn test_csv_html_escape() {
        let csv = "html\n<script>alert(1)</script>\n";
        let out = render(csv, &opts_default());
        assert!(!out.contains("<script>"), "raw script leaked: {}", out);
        assert!(out.contains("&lt;script&gt;"), "got: {}", out);
    }

    #[test]
    fn test_tsv_tab_separator() {
        let tsv = "a\tb\n1\t2\n";
        let opts = CsvTableOptions {
            separator: '\t',
            ..opts_default()
        };
        let out = render(tsv, &opts);
        assert!(out.contains("<th>a</th>"));
        assert!(out.contains("<th>b</th>"));
        assert!(out.contains("<td>1</td><td>2</td>"));
    }

    #[test]
    fn test_csv_with_caption() {
        let csv = "a\n1\n";
        let opts = CsvTableOptions {
            caption: Some("My Data".to_string()),
            ..opts_default()
        };
        let out = render(csv, &opts);
        assert!(out.contains("<caption>My Data</caption>"), "got: {}", out);
    }

    #[test]
    fn test_csv_empty() {
        let out = render("", &opts_default());
        // Wrapped in the shared scroll container; the table is still emitted.
        assert!(
            out.starts_with("<div class=\"moss-table-scroll\" tabindex=\"0\">"),
            "got: {}",
            out
        );
        assert!(out.contains("<table"), "got: {}", out);
        assert!(out.ends_with("</table></div>"), "got: {}", out);
    }

    #[test]
    fn test_csv_wrapped_in_scroll_container() {
        let out = render("name,age\nAlice,30\n", &opts_default());
        assert!(
            out.starts_with("<div class=\"moss-table-scroll\" tabindex=\"0\">"),
            "CSV embed must share the scroll wrapper: {out}"
        );
        assert!(out.ends_with("</div>"), "got: {}", out);
        // Table + its class survive inside the wrapper.
        assert!(out.contains("class=\"moss-embed\" data-type=\"table\""), "{out}");
    }

    #[test]
    fn test_csv_crlf_line_endings() {
        let csv = "a,b\r\n1,2\r\n";
        let out = render(csv, &opts_default());
        assert!(out.contains("<th>a</th>"), "got: {}", out);
        assert!(out.contains("<th>b</th>"), "got: {}", out);
        assert!(out.contains("<td>1</td><td>2</td>"), "got: {}", out);
    }
}
