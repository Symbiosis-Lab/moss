//! A country-map page's table, matched against the shapes: the rows to draw
//! and the problems an author must see.

use super::matching::{same, Atlas, Matched};
use super::spec::{self, Spec};
use moss_core::ast::{self, plain_text::inlines_to_plain_text, Block};
use moss_core::resolve::embed_renderer::folder_list::{classify_folder_segments, emit_marker, marker_encode};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Row {
    /// Index into the shapes' countries.
    pub country: usize,
    /// The country as the author wrote it.
    pub label: String,
    /// Index into the spec's levels; `None` for a blank cell.
    pub level: Option<usize>,
    /// The other columns that have a value, as (header, value).
    pub columns: Vec<(String, String)>,
}

/// A country-map page. With no rows nothing is drawn; `problems` says why.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Model {
    pub spec: Spec,
    pub rows: Vec<Row>,
    /// Build problems, each naming the page, row and value.
    pub problems: Vec<String>,
}

/// A page's settings and body: the model (`None` for a page that is not a
/// country map) and the body with the map's embed above its own table.
pub(crate) fn prepare(page: &str, body: String, settings: Option<&Value>) -> (Option<Model>, String) {
    let Some(settings) = settings else { return (None, body) };
    let model = from_page(page, &body, settings);
    let at = marker_encode(page);
    let marker = emit_marker(&at, &at, &classify_folder_segments("style:map"));
    (Some(model), format!("{marker}\n\n{body}"))
}

fn from_page(page: &str, markdown: &str, settings: &Value) -> Model {
    let failed = |problem: String| Model { problems: vec![format!("[{page}] {problem}")], ..Model::default() };
    let spec = match spec::parse(settings) {
        Ok(spec) => spec,
        Err(problem) => return failed(problem),
    };
    match first_table(markdown) {
        Some((header, rows)) => resolve(page, spec, &header, &rows),
        None => failed("country_map: the page has no table of countries".into()),
    }
}

type Cells = Vec<String>;

/// The first table of the body, as plain-text cells.
fn first_table(markdown: &str) -> Option<(Cells, Vec<Cells>)> {
    let text = |cell: &Vec<ast::Inline>| inlines_to_plain_text(cell).trim().to_string();
    ast::parse(markdown).blocks.iter().find_map(|block| match block {
        Block::Table { header, rows, .. } => Some((header.iter().map(text).collect(), rows.iter().map(|r| r.iter().map(text).collect()).collect())),
        _ => None,
    })
}

fn resolve(page: &str, spec: Spec, header: &[String], table_rows: &[Cells]) -> Model {
    let atlas = Atlas::get();
    let mut problems = Vec::new();
    let by_col = header.iter().position(|h| same(h, &spec.by));
    if by_col.is_none() {
        problems.push(format!("[{page}] country_map: no column named '{}' (the columns are {})", spec.by, header.join(", ")));
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (i, cells) in table_rows.iter().enumerate() {
        let number = i + 1;
        let cell = cells.first().map(String::as_str).unwrap_or("");
        let country = match atlas.find(cell) {
            Matched::Country(c) => c,
            Matched::Disputed(_) => {
                problems.push(format!("[{page}] row {number}: '{cell}' is disputed or breakaway land, which moss does not colour"));
                continue;
            }
            Matched::Antarctica => {
                problems.push(format!("[{page}] row {number}: Antarctica is not drawn, so '{cell}' cannot be coloured"));
                continue;
            }
            Matched::Unmatched => {
                problems.push(format!("[{page}] row {number}: '{cell}' is not a country moss knows; use its English name or ISO 3166 alpha-3 code"));
                continue;
            }
        };
        if !seen.insert(country) {
            problems.push(format!("[{page}] row {number}: '{cell}' is listed twice; the first row is used"));
            continue;
        }
        let value = by_col.and_then(|c| cells.get(c)).map(String::as_str).unwrap_or("");
        let level = if value.is_empty() {
            None
        } else {
            let found = spec.levels.iter().position(|l| same(&l.name, value));
            if found.is_none() {
                let names: Vec<&str> = spec.levels.iter().map(|l| l.name.as_str()).collect();
                problems.push(format!("[{page}] row {number}: '{value}' for {cell} is not one of the levels ({})", names.join(", ")));
            }
            found
        };
        let columns = header
            .iter()
            .enumerate()
            .filter(|(c, _)| *c != 0 && Some(*c) != by_col)
            .filter_map(|(c, h)| cells.get(c).filter(|v| !v.is_empty()).map(|v| (h.clone(), v.clone())))
            .collect();
        rows.push(Row { country, label: cell.to_string(), level, columns });
    }
    Model { spec, rows, problems }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(rows: &str) -> Model {
        let md = format!("| Country | Level | Note |\n|---|---|---|\n{rows}");
        let settings: Value = serde_yaml::from_str("by: Level\nlevels: [Low, High]").unwrap();
        from_page("risk.md", &md, &settings)
    }

    #[test]
    fn matched_rows_carry_their_level_and_extra_columns() {
        let m = model("| Mexico | **High** | Take `care`. |\n| Japan | low | |\n| Chile | | |\n");
        assert!(m.problems.is_empty(), "{:?}", m.problems);
        assert_eq!(m.rows[0].level, Some(1));
        assert_eq!(m.rows[0].columns, [("Note".to_string(), "Take care.".to_string())]);
        assert_eq!(m.rows[1].level, Some(0));
        assert_eq!(m.rows[2].level, None, "a blank cell is listed, not coloured");
    }

    #[test]
    fn each_problem_names_page_row_and_value() {
        let m = model("| Mexco | High | |\n| Japan | Severe | |\n| Chile | Low | |\n| chile | High | |\n| Somaliland | Low | |\n| Antarctica | Low | |\n");
        let all = m.problems.join("\n");
        for part in [
            "[risk.md] row 1: 'Mexco' is not a country",
            "row 2: 'Severe' for Japan is not one of the levels (Low, High)",
            "row 4: 'chile' is listed twice",
            "row 5: 'Somaliland' is disputed or breakaway",
            "row 6: Antarctica is not drawn",
        ] {
            assert!(all.contains(part), "{part} missing from:\n{all}");
        }
        assert_eq!(m.problems.len(), 5);
    }

    #[test]
    fn a_bad_block_or_no_table_leaves_a_model_with_no_rows_and_the_problem() {
        let bad: Value = serde_yaml::from_str("by: Level").unwrap();
        let m = from_page("p.md", "| a |\n|---|\n| b |", &bad);
        assert!(m.rows.is_empty() && m.problems[0].contains("[p.md]"));
        let ok: Value = serde_yaml::from_str("by: Level\nlevels: [Low]").unwrap();
        let m = from_page("p.md", "no table", &ok);
        assert!(m.rows.is_empty() && m.problems[0].contains("no table"));
    }

    #[test]
    fn a_page_that_is_not_a_country_map_is_left_alone() {
        let (model, body) = prepare("p.md", "body".into(), None);
        assert!(model.is_none());
        assert_eq!(body, "body");
    }

    #[test]
    fn a_country_map_gets_its_embed_above_its_own_table() {
        let settings: Value = serde_yaml::from_str("by: Level\nlevels: [Low]").unwrap();
        let (model, body) = prepare("p.md", "| Country | Level |\n|---|---|\n| Chile | Low |".into(), Some(&settings));
        assert_eq!(model.unwrap().rows.len(), 1);
        assert!(body.starts_with("MOSS_MARKER") || body.contains("style=map"), "{body}");
        assert!(body.trim_end().ends_with("| Chile | Low |"));
    }
}
