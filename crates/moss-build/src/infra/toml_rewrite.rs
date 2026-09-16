//! Save a changed `toml::Value` back over the file it came from **without
//! rewriting the parts that did not change**.
//!
//! # The idea
//!
//! `.moss/config.toml` is a file the user hand-writes. Comments, blank lines,
//! the order they put the keys in, whether they quoted a string with `'` or
//! `"` — all of that is theirs, and none of it survives a round trip through
//! `toml::to_string_pretty`, which regenerates the whole file from a value tree
//! that never held any of it.
//!
//! On 2026-08-07 that cost a real user four lines of hand-written comment
//! explaining a setting, silently, during a schema migration that changed
//! nothing in the section the comment was in. This module is the fix: it edits
//! the user's bytes in place instead of re-emitting them.
//!
//! It is the TOML counterpart of the rule the project already follows for
//! frontmatter ("YAML round-trip fidelity" in `.claude/CLAUDE.md`): replace the
//! part you meant to change, leave every other byte alone.
//!
//! # The mechanism
//!
//! The caller still works with an ordinary `toml::Value`, so migration steps
//! and config writers stay simple and testable. [`apply_changes`] parses the
//! original text twice — once into a `toml::Value` (what it *said*) and once
//! into a `toml_edit` document (what it *looked like*) — diffs the two value
//! trees key by key, and applies only the differences to the document.
//!
//! A key whose value is unchanged is never touched, so its formatting cannot
//! change. A key whose value changed has its value replaced, keeping the
//! surrounding whitespace and any trailing comment on that line. A key that
//! appeared is appended; a key that disappeared is removed along with its own
//! comment lines.
//!
//! Because the diff is structural, every migration step gets this for free —
//! present ones and future ones alike. There is no per-step discipline to
//! forget.
//!
//! # What it still cannot preserve
//!
//! - Comments attached to a **removed** key go with it. They describe a key
//!   that no longer exists, so that is the intended outcome.
//! - Inside an **inline table** (`{ a = 1 }`) a changed key may lose the
//!   original spacing around the values it sits beside. TOML forbids comments
//!   inside inline tables, so nothing the user wrote in prose is at risk.
//! - A key that changes **type** from a table to a scalar (or back) is
//!   rewritten wholesale.

use toml_edit::{Document, InlineTable, Item, Table, Value as EditValue};

type TomlTable = toml::value::Table;

/// Render `updated` as an edit of `original`, changing only what differs.
///
/// `original` must be the exact text `updated` was parsed from (before the
/// caller mutated it) — that is what makes "differs" meaningful.
pub fn apply_changes(original: &str, updated: &TomlTable) -> Result<String, String> {
    let before: TomlTable = toml::from_str(original)
        .map_err(|e| format!("Failed to re-parse config for a format-preserving save: {}", e))?;
    let mut doc: Document = original
        .parse()
        .map_err(|e| format!("Failed to parse config for a format-preserving save: {}", e))?;

    merge_table(doc.as_table_mut(), &before, updated);
    Ok(doc.to_string())
}

/// Apply the `before` → `after` difference to a document table.
fn merge_table(dest: &mut Table, before: &TomlTable, after: &TomlTable) {
    for key in before.keys() {
        if !after.contains_key(key) {
            dest.remove(key);
        }
    }
    for (key, new_value) in after {
        match before.get(key) {
            // Untouched: do not go near its bytes.
            Some(old_value) if old_value == new_value => {}
            // Table on both sides: descend, so a change to one leaf key cannot
            // reformat its siblings.
            Some(toml::Value::Table(old_table)) if new_value.is_table() => {
                let new_table = new_value.as_table().expect("checked is_table");
                match dest.get_mut(key) {
                    Some(Item::Table(t)) => merge_table(t, old_table, new_table),
                    Some(Item::Value(EditValue::InlineTable(t))) => {
                        merge_inline_table(t, old_table, new_table)
                    }
                    _ => {
                        dest.insert(key, new_item(new_value));
                    }
                }
            }
            // Array of tables on both sides, already rendered as `[[header]]`
            // blocks: descend per element. Replacing it wholesale would
            // collapse every block onto one inline line — which for a list
            // that changes on a schedule (`state.toml`'s probe results) is a
            // whole-section rewrite in the user's git on the first save.
            Some(toml::Value::Array(old_items))
                if matches!(dest.get(key), Some(Item::ArrayOfTables(_)))
                    && all_tables(new_value) =>
            {
                let new_items = new_value.as_array().expect("checked by all_tables");
                match dest.get_mut(key) {
                    Some(Item::ArrayOfTables(aot)) => {
                        merge_array_of_tables(aot, old_items, new_items)
                    }
                    _ => unreachable!("matched Item::ArrayOfTables above"),
                }
            }
            // Changed scalar/array, or a type change: replace the value and
            // keep the decor (the spacing and any `# trailing comment`).
            _ => match dest.get_mut(key) {
                Some(item) if item.is_value() => {
                    let decor = item.as_value().expect("checked is_value").decor().clone();
                    let mut value = to_edit_value(new_value);
                    *value.decor_mut() = decor;
                    *item = Item::Value(value);
                }
                _ => {
                    dest.insert(key, new_item(new_value));
                }
            },
        }
    }
}

/// A non-empty array whose every element is a table — the only shape TOML can
/// render as `[[header]]` blocks.
fn all_tables(value: &toml::Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(|v| v.is_table()))
}

/// Apply the difference element by element, matching by position.
///
/// Position is the only correspondence available — the entries carry no key —
/// and it is the right one for the writers that have such an array: they
/// rebuild a fixed-order list and change fields inside it.
fn merge_array_of_tables(
    dest: &mut toml_edit::ArrayOfTables,
    before: &[toml::Value],
    after: &[toml::Value],
) {
    while dest.len() > after.len() {
        dest.remove(dest.len() - 1);
    }
    // `before` describes the bytes in `dest`, so the two are the same length
    // in practice; `get` rather than index keeps that an assumption the code
    // does not have to be right about.
    let overlap = dest.len().min(after.len());
    for i in 0..overlap {
        if let (Some(old), Some(new)) = (
            before.get(i).and_then(|v| v.as_table()),
            after[i].as_table(),
        ) {
            merge_table(dest.get_mut(i).expect("i < dest.len()"), old, new);
        }
    }
    for value in &after[overlap..] {
        if let Item::Table(t) = new_item(value) {
            dest.push(t);
        }
    }
}

/// The same difference, applied inside an inline table. Inline tables hold
/// only values (TOML forbids both comments and sub-headers inside them), so
/// this is `merge_table` minus the header cases.
fn merge_inline_table(dest: &mut InlineTable, before: &TomlTable, after: &TomlTable) {
    for key in before.keys() {
        if !after.contains_key(key) {
            dest.remove(key);
        }
    }
    for (key, new_value) in after {
        match before.get(key) {
            Some(old_value) if old_value == new_value => {}
            Some(toml::Value::Table(old_table)) if new_value.is_table() => {
                let new_table = new_value.as_table().expect("checked is_table");
                match dest.get_mut(key) {
                    Some(EditValue::InlineTable(t)) => {
                        merge_inline_table(t, old_table, new_table)
                    }
                    _ => {
                        dest.insert(key, to_edit_value(new_value));
                    }
                }
            }
            _ => {
                dest.insert(key, to_edit_value(new_value));
            }
        }
    }
}

/// Build a document item for a key that did not exist before. A table becomes
/// a real `[header]` section rather than an inline table, matching how a user
/// would have written it; a table holding nothing but tables is marked
/// implicit so it renders as `[channels.email]` and not a bare `[channels]`
/// followed by it.
fn new_item(value: &toml::Value) -> Item {
    match value {
        // Real `[[header]]` blocks, which is what a user would have written
        // and what `toml::to_string` emitted before this writer existed.
        toml::Value::Array(items) if all_tables(value) => {
            let mut out = toml_edit::ArrayOfTables::new();
            for child in items {
                if let Item::Table(t) = new_item(child) {
                    out.push(t);
                }
            }
            Item::ArrayOfTables(out)
        }
        toml::Value::Table(table) => {
            let mut out = Table::new();
            out.set_implicit(!table.is_empty() && table.values().all(|v| v.is_table()));
            for (key, child) in table {
                out.insert(key, new_item(child));
            }
            Item::Table(out)
        }
        other => Item::Value(to_edit_value(other)),
    }
}

fn to_edit_value(value: &toml::Value) -> EditValue {
    match value {
        toml::Value::String(s) => EditValue::from(s.as_str()),
        toml::Value::Integer(i) => EditValue::from(*i),
        toml::Value::Float(f) => EditValue::from(*f),
        toml::Value::Boolean(b) => EditValue::from(*b),
        // `toml` and `toml_edit` share one `toml_datetime::Datetime`. If that
        // ever stops being true this line stops compiling, which is the right
        // way to find out.
        toml::Value::Datetime(d) => EditValue::from(*d),
        toml::Value::Array(items) => items
            .iter()
            .map(to_edit_value)
            .collect::<toml_edit::Array>()
            .into(),
        toml::Value::Table(table) => {
            let mut out = InlineTable::new();
            for (key, child) in table {
                out.insert(key, to_edit_value(child));
            }
            EditValue::InlineTable(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse, hand the value to `mutate`, save. The shape every caller uses.
    fn round_trip(original: &str, mutate: impl FnOnce(&mut TomlTable)) -> String {
        let mut root: TomlTable = toml::from_str(original).unwrap();
        mutate(&mut root);
        apply_changes(original, &root).unwrap()
    }

    #[test]
    fn an_unchanged_value_is_returned_byte_for_byte() {
        let original = "# top\n\nkey   =    'single quoted'\n\n[site]\n# why\nlang = \"en\"\n";
        assert_eq!(round_trip(original, |_| {}), original);
    }

    #[test]
    fn changing_one_key_leaves_the_comments_around_it_alone() {
        let original = "\
# Site configuration.

schema_version = 4

[site]
# We turn this off because the captions are already in the images.
# Do not turn it back on without asking 阿明.
implicit_figure = false
lang = \"zh\"
";
        let after = round_trip(original, |v| {
            v
                .insert("schema_version".into(), toml::Value::Integer(5));
        });
        assert_eq!(
            after,
            original.replace("schema_version = 4", "schema_version = 5")
        );
    }

    #[test]
    fn a_new_root_key_lands_before_the_first_section_not_inside_it() {
        let original = "# hi\n[site]\nlang = \"en\"\n";
        let after = round_trip(original, |v| {
            v
                .insert("schema_version".into(), toml::Value::Integer(5));
        });
        let reparsed: toml::Value = toml::from_str(&after).unwrap();
        assert_eq!(
            reparsed.get("schema_version").and_then(|v| v.as_integer()),
            Some(5),
            "a root key appended after a [section] header would become that \
             section's key instead; got:\n{after}"
        );
        assert!(after.contains("# hi"), "got:\n{after}");
    }

    #[test]
    fn a_new_table_of_tables_renders_as_headers() {
        let original = "schema_version = 0\n";
        let after = round_trip(original, |v| {
            let mut email = toml::value::Table::new();
            email.insert("send_mode".into(), toml::Value::String("all".into()));
            let mut channels = toml::value::Table::new();
            channels.insert("email".into(), toml::Value::Table(email));
            channels.insert("matters".into(), toml::Value::Table(toml::value::Table::new()));
            v
                .insert("channels".into(), toml::Value::Table(channels));
        });
        assert!(after.contains("[channels.email]"), "got:\n{after}");
        assert!(after.contains("[channels.matters]"), "got:\n{after}");
        assert!(!after.contains("channels = {"), "got:\n{after}");
    }

    #[test]
    fn removing_a_key_removes_its_comment_and_nothing_else() {
        let original = "\
[services.comments]
# stale, points at the dead HK box
server_url = \"https://api.moss.host/comments\"
# this one is still ours
enabled = true
";
        let after = round_trip(original, |v| {
            v.get_mut("services")
                .and_then(|s| s.get_mut("comments"))
                .and_then(|c| c.as_table_mut())
                .unwrap()
                .remove("server_url");
        });
        assert!(!after.contains("server_url"), "got:\n{after}");
        assert!(after.contains("# this one is still ours"), "got:\n{after}");
        assert!(!after.contains("dead HK box"), "got:\n{after}");
    }

    #[test]
    fn a_sibling_sections_formatting_survives_a_change_elsewhere() {
        let original = "\
[a]
odd   =   [ 1,2,  3 ]
str = 'kept as-is'

[b]
n = 1
";
        let after = round_trip(original, |v| {
            v.get_mut("b").and_then(|b| b.as_table_mut()).unwrap()
                .insert("n".into(), toml::Value::Integer(2));
        });
        assert!(after.contains("odd   =   [ 1,2,  3 ]"), "got:\n{after}");
        assert!(after.contains("str = 'kept as-is'"), "got:\n{after}");
        assert!(after.contains("n = 2"), "got:\n{after}");
    }

    #[test]
    fn a_trailing_comment_on_a_changed_line_survives() {
        let original = "n = 1 # keep me\n";
        let after = round_trip(original, |v| {
            v.insert("n".into(), toml::Value::Integer(2));
        });
        assert_eq!(after, "n = 2 # keep me\n");
    }

    #[test]
    fn an_inline_table_keeps_the_keys_that_did_not_change() {
        let original = "t = { keep = 'yes', change = 1 }\n";
        let after = round_trip(original, |v| {
            v.get_mut("t").and_then(|t| t.as_table_mut()).unwrap()
                .insert("change".into(), toml::Value::Integer(2));
        });
        assert!(after.contains("keep = 'yes'"), "got:\n{after}");
        assert!(after.contains("change = 2"), "got:\n{after}");
    }

    #[test]
    fn every_scalar_kind_survives_being_written_fresh() {
        let original = "placeholder = 0\n";
        let after = round_trip(original, |v| {
            let root = v;
            root.insert("s".into(), toml::Value::String("x".into()));
            root.insert("i".into(), toml::Value::Integer(-3));
            root.insert("f".into(), toml::Value::Float(1.5));
            root.insert("b".into(), toml::Value::Boolean(true));
            root.insert(
                "d".into(),
                toml::Value::Datetime("1979-05-27T07:32:00Z".parse().unwrap()),
            );
            root.insert(
                "a".into(),
                toml::Value::Array(vec![toml::Value::Integer(1), toml::Value::Integer(2)]),
            );
        });
        let reparsed: toml::Value = toml::from_str(&after).unwrap();
        assert_eq!(reparsed.get("s").unwrap().as_str(), Some("x"));
        assert_eq!(reparsed.get("i").unwrap().as_integer(), Some(-3));
        assert_eq!(reparsed.get("f").unwrap().as_float(), Some(1.5));
        assert_eq!(reparsed.get("b").unwrap().as_bool(), Some(true));
        assert!(reparsed.get("d").unwrap().as_datetime().is_some());
        assert_eq!(reparsed.get("a").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn an_array_of_tables_stays_headers_and_only_the_changed_entry_moves() {
        // `state.toml`'s probe list. A tick changes one entry's state; the
        // rest of the blocks must not be touched, and none of them may
        // collapse into an inline array.
        let original = "\
[deployment_setup]
started_at = 1

[[deployment_setup.probe]]
kind   =   \"dns\"
state = \"pending\"

[[deployment_setup.probe]]
# hand-annotated
kind = \"tls\"
state = \"pending\"
";
        let after = round_trip(original, |root| {
            root.get_mut("deployment_setup")
                .and_then(|d| d.get_mut("probe"))
                .and_then(|p| p.as_array_mut())
                .unwrap()[0]
                .as_table_mut()
                .unwrap()
                .insert("state".into(), toml::Value::String("ok".into()));
        });
        assert!(after.contains("[[deployment_setup.probe]]"), "got:\n{after}");
        assert!(!after.contains("probe = ["), "collapsed to inline:\n{after}");
        assert!(after.contains("state = \"ok\""), "got:\n{after}");
        assert!(after.contains("kind   =   \"dns\""), "sibling reformatted:\n{after}");
        assert!(after.contains("# hand-annotated"), "comment lost:\n{after}");
    }

    #[test]
    fn a_new_array_of_tables_renders_as_headers_not_one_inline_line() {
        let original = "n = 1\n";
        let after = round_trip(original, |root| {
            let mut one = toml::value::Table::new();
            one.insert("kind".into(), toml::Value::String("dns".into()));
            root.insert("probe".into(), toml::Value::Array(vec![toml::Value::Table(one)]));
        });
        assert!(after.contains("[[probe]]"), "got:\n{after}");
        assert!(!after.contains("probe = ["), "got:\n{after}");
    }

    #[test]
    fn an_inline_array_of_tables_is_not_rewritten_into_headers() {
        // The user wrote it inline. Changing a field inside must not convert
        // their file to `[[header]]` blocks.
        let original = "t = [ { a = 1 }, { a = 2 } ]\n";
        let after = round_trip(original, |root| {
            root.get_mut("t").and_then(|t| t.as_array_mut()).unwrap()[0]
                .as_table_mut()
                .unwrap()
                .insert("a".into(), toml::Value::Integer(9));
        });
        assert!(!after.contains("[[t]]"), "converted to headers:\n{after}");
        assert!(after.contains("9"), "got:\n{after}");
    }
}
