//! Reading `.moss/config.toml` — the half of it that belongs to the open crate.
//!
//! # Who owns this file
//!
//! The **app** owns `.moss/config.toml`: every setting is written from a
//! settings modal, and every writer lives in the app crate.
//! What ships here is the **reader**, because the CLI reads config too and
//! `moss-cli` cannot depend on the app crate (`scripts/check-crate-dag.mjs`
//! rule 4).
//!
//! # What is here, and what deliberately is not
//!
//! Here: turning the *text* of a config file into values — parse, then look a
//! key up. Not here: opening the file. That read is eviction-aware on macOS
//! (an iCloud-evicted config must never read as "absent", or a default is
//! written over the user's settings), and the code that proves absence from an
//! error lives in the build tree's cloud modules (`build::cloud_readiness`),
//! not here. So a caller reads the bytes however its platform
//! requires and hands the string to [`ConfigFile::parse`]:
//!
//! - the app reads through `build::cloud_readiness` (see
//!   `domain::config::read_managed_toml`), which waits for a materializing
//!   file rather than mistaking it for a missing one;
//! - a future `moss-cli` reads with `std::fs::read_to_string` **only where
//!   that is the whole story**. It is not the whole story on macOS, which
//!   moss-cli ships on: an iCloud-evicted config returns `NotFound` there, and
//!   a CLI that believed it would build the user's site with every setting at
//!   its default. The CLI needs the eviction-aware read too.
//!
//! Splitting it this way is also what keeps this module honest about a core
//! rule: **advanced users hand-edit this file.** Nothing here can
//! write, so nothing here can disturb a comment, a key order or a quoting
//! style. A reader preserves them by construction; the one writer primitive
//! this crate holds, [`crate::vault::config`],
//! preserves them with `infra::toml_rewrite`, and the app's modal-driven
//! writers go through the same door.

pub mod deployment;
pub mod environment;
pub mod migrations;
pub mod services;

use toml::Value;

/// The parsed contents of one `.moss/config.toml`.
///
/// Parsed once per call today, and holdable — which is the point. The app used
/// to re-read and re-parse the file **per key**: "a dozen parses of one file",
/// and so in principle a dozen different versions of it within one build. That
/// is now expressible as one value, but nothing holds it across a build yet;
/// that arrives with `ProjectConfig` on `BuildInputs`. Do not read this doc as
/// a claim that the duplication is already gone.
///
/// There is deliberately no `Default` impl. `Self::empty()` is only ever valid
/// for a file **proven** absent (see its doc), and a `Default` would make
/// `read_project_config(path).unwrap_or_default()` compile — which reads as
/// idiomatic Rust and silently builds a user's site with every setting at its
/// default when their config is merely unreadable.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    root: toml::Table,
    /// Keys of the tables whose written order matters, in the order the file
    /// writes them, by dotted path (`terms`, `fields.kind.of`). `root` reads
    /// tables back sorted by key, so this is the only record of that order.
    key_orders: std::collections::BTreeMap<String, Vec<String>>,
}

impl ConfigFile {
    /// Parse config text. Returns the parse error verbatim; a malformed config
    /// is never silently treated as an empty one, because "empty" is what the
    /// caller would then act on.
    ///
    /// The parsed value is migrated to `CURRENT_VERSION` **in memory** before
    /// any key is read (open-CLI slice 3). Once
    /// moss-cli is a separate binary, "no single migration point both reach"
    /// exists, and a host that forgets to migrate renders a v4 config's absent
    /// keys as defaults with no error anywhere — migrating at the one parse
    /// funnel closes that class for every host, current and future. The
    /// transform is idempotent, so the app's persisted-at-entry file passes
    /// through unchanged; persistence itself stays a host concern (the app's
    /// `HostStore::run_vault_migrations`) and no writer ships in this crate.
    /// A transform error falls back to the raw value: a config moss cannot
    /// migrate still renders with the keys it can read, matching the app's
    /// "corrupt config renders with absent-key defaults" stance.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut table: toml::Table =
            toml::from_str(text).map_err(|e| format!("Failed to parse config.toml: {}", e))?;
        if let Err(e) = crate::config::migrations::migrate_to_current(&mut table) {
            log::warn!(target: "config", "in-memory config migration failed, reading raw values: {e}");
        }
        Ok(Self { root: table, key_orders: written_key_orders(text) })
    }

    /// The config of a project that has none — every lookup answers `None`.
    ///
    /// Only ever stand this in for a file **proven** absent. A file that could
    /// not be read (evicted, half-synced, permissions) is not an empty file,
    /// and treating it as one hands every caller the default instead of the
    /// user's setting.
    pub fn empty() -> Self {
        Self {
            root: toml::Table::new(),
            key_orders: Default::default(),
        }
    }

    /// `Some(v)` when this file declared a schema newer than this build
    /// supports. Free — [`Self::parse`]'s `VersionAhead` short-circuits
    /// before touching `root`, so the original `schema_version` survives
    /// into `self.root` untouched and this is just [`migrations::version_ahead`]
    /// over the table already in hand. Callers that already hold a
    /// `ConfigFile` (e.g. the pipeline's one-parse-per-build `cfg`) should
    /// call this instead of re-reading and re-parsing the file.
    pub fn schema_version_ahead(&self) -> Option<u32> {
        crate::config::migrations::version_ahead(&self.root, crate::config::migrations::CURRENT_VERSION)
    }

    /// A string field under `[site]`, e.g. `lang`, `typesetting`.
    pub fn site_str(&self, field: &str) -> Option<&str> {
        self.root
            .get("site")
            .and_then(|site| site.get(field))
            .and_then(|v| v.as_str())
    }

    /// A boolean field under `[site]`, e.g. `comments`, `math`.
    ///
    /// `None` means the key is absent — which is not the same as `false`.
    /// Most of these knobs default to ON, so the caller supplies the default.
    pub fn site_bool(&self, field: &str) -> Option<bool> {
        self.root
            .get("site")
            .and_then(|site| site.get(field))
            .and_then(|v| v.as_bool())
    }

    /// The top-level `environment` key. Top-level rather than under `[site]`
    /// so it never collides with `state.toml`'s `[deployment]` section.
    pub fn environment(&self) -> Option<&str> {
        self.root.get("environment").and_then(|v| v.as_str())
    }

    /// `[build].passthrough` — an ordered list of path strings. Entries with
    /// no prefix are explicit passthrough directories or scanned HTML files; a
    /// `!` prefix removes a match. Empty when the key is absent.
    pub fn build_passthrough(&self) -> Vec<String> {
        self.root
            .get("build")
            .and_then(|b| b.get("passthrough"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `[build].keep_generations` — how many generation directories survive
    /// retention. `None` when absent or not an integer; a malformed knob means
    /// "use the default", never "stop the build".
    pub fn build_keep_generations(&self) -> Option<usize> {
        self.root
            .get("build")
            .and_then(|b| b.get("keep_generations"))
            .and_then(|v| v.as_integer())
            .map(|n| n.max(0) as usize)
    }

    /// `[build].prune_orphaned_images` — whether ship-time orphan pruning
    /// runs. `None` when unset; the caller supplies the default (on).
    pub fn build_prune_orphaned_images(&self) -> Option<bool> {
        self.root
            .get("build")
            .and_then(|b| b.get("prune_orphaned_images"))
            .and_then(|v| v.as_bool())
    }

    /// `[history].enabled` — whether a landed publish snapshots into the
    /// vault's own publish-history store, at `.moss/history/`.
    /// `None` when unset; the caller's default is on.
    pub fn history_enabled(&self) -> Option<bool> {
        self.root
            .get("history")
            .and_then(|h| h.get("enabled"))
            .and_then(|v| v.as_bool())
    }

    /// A section by dotted path, e.g. `["services"]` or
    /// `["channels", "email", "send_mode"]`.
    ///
    /// The escape hatch for values whose typed form still lives in the app —
    /// `[services]` and `[channels.email.send_mode]` deserialize into types
    /// that sit on the Tauri command boundary today. The caller does the
    /// `try_into`; this hands it the subtree.
    pub fn section(&self, path: &[&str]) -> Option<&Value> {
        let (first, rest) = path.split_first()?;
        let mut cursor = self.root.get(*first)?;
        for key in rest {
            cursor = cursor.get(key)?;
        }
        Some(cursor)
    }

    /// Every `[terms.<key>]` table, plus a built-in `RawKind` for each of
    /// `authors`/`tags` that NO declared table already used — the one
    /// unified config read `build::terms::term_kinds` turns into
    /// `TermKind`s. Raw here: `fields` is not yet checked against the
    /// built-in schema, and `title` is not yet resolved — both happen exactly
    /// once, in `term_kinds`, not here.
    ///
    /// A declared `[terms.authors]` or `[terms.tags]` table IS that kind —
    /// its own `fields` and `title` win outright, not just its own
    /// declared fields layered on top of the built-in default. Reading the
    /// sub-tables first and skipping the matching built-in is what makes
    /// that true: the old order (built-ins unconditionally, then every
    /// declared table appended) produced two `RawKind`s sharing one key
    /// whenever an author declared `[terms.tags]` or `[terms.authors]`,
    /// which `term_kinds` had no way to reconcile — `roots_in_use` and
    /// `synthetic_folder_keys` would seed the same pseudo-folder twice, and
    /// `members_by_field` would double-count every member.
    ///
    /// A `[terms.<key>]` table with no `fields` array is skipped with a
    /// diagnostic — a kind that names no fields would derive nothing anyway,
    /// and is more likely a typo (`field = [...]`) than an intentional
    /// no-op.
    pub fn terms_kinds(&self) -> Vec<RawKind> {
        let mut declared: Vec<RawKind> = Vec::new();
        if let Some(terms) = self.root.get("terms").and_then(|v| v.as_table()) {
            for (key, value) in terms {
                // The legacy `author`/`tags` booleans live in this same
                // table but are not sub-tables, so `as_table()` already
                // excludes them here — no separate key check needed.
                let Some(table) = value.as_table() else { continue };
                let Some(fields) = table.get("fields").and_then(|v| v.as_array()) else {
                    crate::build::cli_output::log_warn_problem!(
                        "[terms.{key}] has no `fields` array; skipping"
                    );
                    continue;
                };
                let fields: Vec<String> =
                    fields.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                let title = table.get("title").and_then(|v| v.as_str()).map(str::to_string);
                let kind_type = table.get("type").and_then(|v| v.as_str()).map(str::to_string);
                let explorer = table.get("explorer").and_then(|v| v.as_bool());
                let line = table.get("line").and_then(|v| v.as_bool());
                let cards = table.get("cards").cloned();
                declared.push(RawKind { key: key.clone(), fields, title, kind_type, explorer, line, cards });
            }
        }
        let declared_keys: std::collections::HashSet<&str> =
            declared.iter().map(|k| k.key.as_str()).collect();

        let mut kinds = Vec::with_capacity(declared.len() + 2);
        for (ns, field) in crate::build::terms::BUILTIN_DEFAULT_FIELDS {
            if declared_keys.contains(ns) {
                continue;
            }
            kinds.push(RawKind {
                key: ns.to_string(),
                fields: if self.legacy_terms_flag(field).unwrap_or(true) {
                    vec![field.to_string()]
                } else {
                    Vec::new()
                },
                title: None,
                kind_type: None,
                explorer: None, line: None, cards: None,
            });
        }
        kinds.extend(declared);
        kinds
    }

    /// The kinds whose `[terms.<key>]` block says `nav = true`, in the order
    /// the file writes them, each with its explicit `title` and `weight` if it set them.
    /// Whether a kind has a root page to link is decided later, from the terms
    /// the site actually has (`TermIndex::nav_entries`).
    pub fn terms_nav(&self) -> Vec<(String, Option<String>, Option<i32>)> {
        let Some(terms) = self.root.get("terms").and_then(|v| v.as_table()) else { return Vec::new() };
        let mut nav: Vec<(usize, String, Option<String>, Option<i32>)> = terms
            .iter()
            .filter_map(|(key, value)| {
                let table = value.as_table()?;
                table.get("nav")?.as_bool().filter(|on| *on)?;
                let title = table.get("title").and_then(|v| v.as_str()).map(str::to_string);
                let order = self.written_position("terms", key);
                let weight = table.get("weight").and_then(|v| v.as_integer()).and_then(|w| i32::try_from(w).ok());
                Some((order, key.clone(), title, weight))
            })
            .collect();
        nav.sort();
        nav.into_iter().map(|(_, key, title, weight)| (key, title, weight)).collect()
    }

    /// The `[redirects]` table: old address, new address, as the author wrote
    /// them. Optional — a config without it is a config with no hand-declared
    /// redirects, and adding the table needs no schema bump. A value that is
    /// not a string, or a `redirects` that is not a table, is reported and
    /// skipped; the entries around it still read.
    pub fn declared_redirects(&self) -> Vec<(String, String)> {
        use crate::build::cli_output::log_warn_problem;
        let Some(value) = self.root.get("redirects") else { return Vec::new() };
        let Some(table) = value.as_table() else {
            log_warn_problem!("[redirects] must be a table of old address = new address; ignoring it");
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for (from, to) in table {
            match to.as_str() {
                Some(to) => pairs.push((from.clone(), to.to_string())),
                None => log_warn_problem!("[redirects] \"{from}\" must be a string address; skipping it"),
            }
        }
        pairs
    }

    /// `key` of `sub` as a list of strings; `table` names `sub` in a problem
    /// (`fields.kind`). A non-list value is one problem and yields nothing; a
    /// non-string item is one problem and is skipped.
    fn string_list(sub: &toml::Table, key: &str, table: &str, problems: &mut Vec<String>) -> Vec<String> {
        match sub.get(key) {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| match item.as_str() {
                    Some(s) => Some(s.to_string()),
                    None => {
                        problems.push(format!("[{table}] `{key}` must hold strings; skipping {item}"));
                        None
                    }
                })
                .collect(),
            Some(_) => {
                problems.push(format!("[{table}] `{key}` must be a list of strings; ignoring it"));
                Vec::new()
            }
        }
    }

    /// One choice's label: a string for every language, or a table of
    /// `language tag = label`. `None` when the value is neither, or a table
    /// holds a non-string.
    fn choice_labels(label: &Value) -> Option<Vec<(String, String)>> {
        match label {
            Value::String(s) => Some(vec![("en".to_string(), s.clone())]),
            Value::Table(by_lang) => by_lang
                .iter()
                .map(|(lang, l)| l.as_str().map(|l| (lang.clone(), l.to_string())))
                .collect(),
            _ => None,
        }
    }

    /// Where `key` sits among the keys of the table at `path`, as the file
    /// writes them; keys the file did not order come last.
    fn written_position(&self, path: &str, key: &str) -> usize {
        self.key_orders.get(path).and_then(|keys| keys.iter().position(|k| k == key)).unwrap_or(usize::MAX)
    }

    /// The `[fields.<name>]` declarations, and one problem string for each
    /// table or key the build cannot use. The caller reports the problems once
    /// per build. A declaration with a bad `place` or an unknown key is kept;
    /// one with a bad type or a built-in name is dropped.
    pub fn field_decls(&self) -> (Vec<moss_core::fields::FieldDecl>, Vec<String>) {
        use moss_core::fields::{Choice, FieldDecl, FieldKind};
        let mut decls = Vec::new();
        let mut problems = Vec::new();
        let Some(value) = self.root.get("fields") else {
            return (decls, problems);
        };
        let Some(table) = value.as_table() else {
            problems.push("[fields] must be a table of [fields.NAME] tables; ignoring it".to_string());
            return (decls, problems);
        };
        let mut written: Vec<_> = table.iter().collect();
        written.sort_by_key(|(name, _)| self.written_position("fields", name));
        for (name, value) in written {
            let Some(sub) = value.as_table() else {
                problems.push(format!("[fields.{name}] must be a table; ignoring it"));
                continue;
            };
            if moss_core::schema_fields::builtin_field(name).is_some() {
                if !moss_core::schema_fields::PLACEABLE_BUILTINS.contains(&name.as_str()) {
                    problems.push(format!(
                        "[fields.{name}] is a moss field and cannot be redeclared; of moss's fields only {} can take a place",
                        moss_core::schema_fields::PLACEABLE_BUILTINS.join(" and ")
                    ));
                    continue;
                }
                for key in sub.keys() {
                    if !matches!(key.as_str(), "place" | "label") {
                        problems.push(format!(
                            "[fields.{name}] has \"{key}\"; built-in fields take only place and label"
                        ));
                    }
                }
                let place = Self::declared_place(sub, name, &mut problems);
                let label = Self::declared_label(sub, name, &mut problems);
                decls.push(FieldDecl { name: name.clone(), kind: FieldKind::Text, choices: Vec::new(), place, label, scope: Vec::new() });
                continue;
            }
            let Some(type_name) = sub.get("type").and_then(|v| v.as_str()) else {
                problems.push(format!("[fields.{name}] has no type; one of: {}", FieldKind::names().join(", ")));
                continue;
            };
            let Some(kind) = FieldKind::from_name(type_name) else {
                problems.push(format!(
                    "[fields.{name}] has an unknown type \"{type_name}\"; one of: {}",
                    FieldKind::names().join(", ")
                ));
                continue;
            };
            for key in sub.keys() {
                if !matches!(key.as_str(), "type" | "of" | "place" | "label" | "in") {
                    problems.push(format!("[fields.{name}] has an unknown key \"{key}\""));
                }
            }
            let choices = match (kind, sub.get("of")) {
                (FieldKind::Choice, None) => {
                    problems.push(format!("[fields.{name}] is a choice and needs `of`, a list or a table of labels"));
                    continue;
                }
                (FieldKind::Choice, Some(Value::Array(_))) => Self::string_list(sub, "of", &format!("fields.{name}"), &mut problems)
                    .into_iter()
                    .map(|v| Choice { value: v.clone(), labels: vec![("en".to_string(), v)] })
                    .collect(),
                (FieldKind::Choice, Some(Value::Table(values))) => {
                    let path = format!("fields.{name}.of");
                    let mut written: Vec<_> = values.iter().collect();
                    written.sort_by_key(|(v, _)| self.written_position(&path, v));
                    written
                        .into_iter()
                        .filter_map(|(v, label)| match Self::choice_labels(label) {
                            Some(labels) => Some(Choice { value: v.clone(), labels }),
                            None => {
                                problems.push(format!(
                                    "[fields.{name}] label for \"{v}\" must be a string or a table of language labels; skipping it"
                                ));
                                None
                            }
                        })
                        .collect()
                }
                (FieldKind::Choice, Some(_)) => {
                    problems.push(format!("[fields.{name}] `of` must be a list or a table of labels"));
                    continue;
                }
                (_, Some(_)) => {
                    problems.push(format!("[fields.{name}] has `of`, which only a choice takes; ignoring it"));
                    Vec::new()
                }
                (_, None) => Vec::new(),
            };
            let place = Self::declared_place(sub, name, &mut problems);
            let label = Self::declared_label(sub, name, &mut problems);
            let scope = Self::string_list(sub, "in", &format!("fields.{name}"), &mut problems);
            decls.push(FieldDecl { name: name.clone(), kind, choices, place, label, scope });
        }
        (decls, problems)
    }

    /// A declaration's `place`, when it names one. `none` and an absent key both mean no place.
    fn declared_place(sub: &toml::Table, name: &str, problems: &mut Vec<String>) -> Option<moss_core::fields::Place> {
        use moss_core::fields::{parse_place, place_names};
        match sub.get("place").and_then(|v| v.as_str()) {
            None | Some("none") => None,
            Some(v) => match parse_place(v) {
                Some(place) => Some(place),
                None => {
                    problems.push(format!(
                        "[fields.{name}] has an unknown place \"{v}\"; one of: {}, none",
                        place_names().join(", ")
                    ));
                    None
                }
            },
        }
    }

    /// A declaration's `label`, as `(language tag, label)` pairs; empty when absent or malformed.
    fn declared_label(sub: &toml::Table, name: &str, problems: &mut Vec<String>) -> Vec<(String, String)> {
        match sub.get("label") {
            None => Vec::new(),
            Some(v) => Self::choice_labels(v).unwrap_or_else(|| {
                problems.push(format!("[fields.{name}] `label` must be a string or a table of language labels; ignoring it"));
                Vec::new()
            }),
        }
    }

    /// A boolean field under `[terms]`, e.g. `author`, `tags`: the legacy,
    /// pre-kinds toggle `terms_kinds` folds into the two built-ins' `fields`.
    /// `None` means the key is absent, which is not the same as `false` —
    /// `terms_kinds` supplies the default (on). Private: nothing outside
    /// `terms_kinds` reads a raw per-field `[terms]` boolean any more.
    fn legacy_terms_flag(&self, field: &str) -> Option<bool> {
        self.root
            .get("terms")
            .and_then(|terms| terms.get(field))
            .and_then(|v| v.as_bool())
    }
}

/// One `[terms.<key>]` table before `build::terms::term_kinds` filters its
/// `fields` and resolves its `title` into a `TermKind`. See
/// [`ConfigFile::terms_kinds`].
#[derive(Debug, Clone)]
pub struct RawKind {
    /// URL namespace / pseudo-folder prefix (`authors`, `tags`, or a
    /// declared key like `people`).
    pub key: String,
    /// Frontmatter field names as configured, not yet checked by
    /// `build::terms::term_kinds`.
    pub fields: Vec<String>,
    /// Explicit `title` from the table, when the author set one. `None` for
    /// the two built-ins (their title comes from `i18n::term_root_title`)
    /// and for a declared kind that didn't set one (its title is its key).
    pub title: Option<String>,
    /// The declared `type = "..."` key, unvalidated — `"place"` is the only
    /// value `build::terms::term_kinds` recognizes today; anything else is a
    /// diagnostic there, not here. `None` for the two built-ins and for a
    /// declared kind that didn't set one.
    pub kind_type: Option<String>,
    /// `[terms.<key>] explorer`, unvalidated and not yet gated on
    /// `kind_type`: `term_kinds` reads this only for the place-typed kind
    /// and drops it for every other one, the same way `type` itself is only
    /// ever meaningful there. `None` for the two built-ins and for a
    /// declared kind that didn't set the key.
    pub explorer: Option<bool>,
    /// `[terms.<key>] line`, unvalidated and gated on `kind_type` the same
    /// way as `explorer`: `term_kinds` keeps it only for the place-typed kind.
    pub line: Option<bool>,
    /// `[terms.<key>] cards`, unvalidated: the raw TOML value, so that
    /// `build::terms::term_kinds` is the one place that reports a value that
    /// is not a bool. `None` when the key is absent and for the two built-ins.
    pub cards: Option<toml::Value>,
}

/// The written key order of `[terms]`, of `[fields]`, and of each `[fields.<name>]` table's
/// `of`, read with `toml_edit`, which keeps the order `toml` discards.
fn written_key_orders(text: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut orders = std::collections::BTreeMap::new();
    let Ok(doc) = text.parse::<toml_edit::Document>() else { return orders };
    let keys = |t: &dyn toml_edit::TableLike| t.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
    if let Some(terms) = doc.get("terms").and_then(|v| v.as_table_like()) {
        orders.insert("terms".to_string(), keys(terms));
    }
    if let Some(fields) = doc.get("fields").and_then(|v| v.as_table_like()) {
        orders.insert("fields".to_string(), keys(fields));
        for (name, field) in fields.iter() {
            if let Some(of) = field.as_table_like().and_then(|f| f.get("of")).and_then(|v| v.as_table_like()) {
                orders.insert(format!("fields.{name}.of"), keys(of));
            }
        }
    }
    orders
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
