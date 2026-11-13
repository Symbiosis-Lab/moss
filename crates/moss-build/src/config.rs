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
pub mod map_layers;
pub mod migrations;
pub mod plugin_view;
pub mod services;
pub mod site_identity;
pub mod site_settings;

use moss_core::page::site_config::{ConfigValue, CoreConfig};
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
    core: CoreConfig,
    warnings: Vec<String>,
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
    /// `persist_migration`, run only when an author edits the file) and no
    /// writer of a migrated config ships in this crate.
    /// A transform error falls back to the raw value: a config moss cannot
    /// migrate still renders with the keys it can read, matching the app's
    /// "corrupt config renders with absent-key defaults" stance.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut table: toml::Table =
            toml::from_str(text).map_err(|e| format!("Failed to parse config.toml: {}", e))?;
        if let Err(e) = crate::config::migrations::migrate_to_current(&mut table) {
            log::info!(target: "config", "in-memory config migration failed, reading raw values: {e}");
        }
        let (core, warnings) = CoreConfig::from_value(&core_table(&table));
        Ok(Self { root: table, core, warnings, key_orders: written_key_orders(text) })
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
            core: Default::default(),
            warnings: Vec::new(),
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

    /// The oldest moss release that renders this site's design correctly, as
    /// its designer wrote it. moss never writes or raises it.
    pub fn min_moss_version(&self) -> Option<&str> {
        self.root.get("min_moss_version").and_then(|v| v.as_str())
    }

    /// Values of the wrong type under `[site] lang` and `[terms]`, one sentence
    /// each; the build reports them once, at its report point.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// `[site] lang` as written, read by the same typed reader every host uses.
    pub fn site_lang(&self) -> Option<&str> {
        self.core.site_lang()
    }

    /// A string field under `[site]`, e.g. `typesetting`.
    pub fn site_str(&self, field: &str) -> Option<&str> {
        self.root
            .get("site")
            .and_then(|site| site.get(field))
            .and_then(|v| v.as_str())
    }

    /// The site's one authored address: `[site].url`, path included, or, in a
    /// folder written before `url` existed, `[site].domain` read as
    /// `https://<domain>/`. Both set with different hosts is refused, naming
    /// both keys, rather than one silently winning.
    pub fn site_address(&self) -> Result<Option<String>, String> {
        let url = self.site_str("url").map(str::trim).filter(|s| !s.is_empty());
        let domain = self.site_str("domain").map(str::trim).filter(|s| !s.is_empty());
        let Some(url) = url else {
            return Ok(domain.map(|d| format!("https://{d}/")));
        };
        let parsed = crate::build::site_url::SiteUrl::parse(url)
            .map_err(|e| format!("[site] url in .moss/config.toml: {e}"))?;
        if let Some(domain) = domain.filter(|d| !parsed.host_name().eq_ignore_ascii_case(d)) {
            return Err(format!(
                "[site] url = \"{url}\" and [site] domain = \"{domain}\" in .moss/config.toml name different hosts; \
                 keep url, which replaces domain, and delete domain"
            ));
        }
        Ok(Some(url.to_string()))
    }

    /// A list-of-strings field under `[site]`, e.g. `me`; one string is a
    /// list of one. Entries that are not strings are skipped.
    pub fn site_strings(&self, field: &str) -> Vec<String> {
        match self.root.get("site").and_then(|site| site.get(field)) {
            Some(toml::Value::String(one)) => vec![one.clone()],
            Some(toml::Value::Array(items)) => items.iter().filter_map(|v| v.as_str()).map(str::to_string).collect(),
            _ => Vec::new(),
        }
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

    /// One note per `[site]` key the build does not read: it names the key and
    /// the file, and the nearest known key when one is near. A note, not a
    /// problem, because older moss versions left keys that no code reads now.
    pub fn unknown_site_keys(&self) -> Vec<String> {
        use crate::config::site_settings::{nearest_known_key, site_settings, UNLISTED_SITE_KEYS};
        let Some(site) = self.root.get("site").and_then(|v| v.as_table()) else {
            return Vec::new();
        };
        let known: Vec<&str> = site_settings().iter().map(|s| s.name).collect();
        site.keys()
            .filter(|key| !known.contains(&key.as_str()) && !UNLISTED_SITE_KEYS.contains(&key.as_str()))
            .map(|key| {
                if key == "title" {
                    return "[site] title is not a setting in .moss/config.toml; the site's name is the `title:` of its home page".to_string();
                }
                match nearest_known_key(key, &known) {
                    Some(k) => format!("[site] has an unknown key \"{key}\" in .moss/config.toml; did you mean \"{k}\"?"),
                    None => format!("[site] has an unknown key \"{key}\" in .moss/config.toml; the build does not read it"),
                }
            })
            .collect()
    }

    /// Per-channel link-back choice, falling back to the global default (on).
    pub fn syndication_link_back(&self, channel: &str) -> bool {
        self.root.get("channels").and_then(|v| v.get(channel)).and_then(|v| v.get("link_back")).and_then(Value::as_bool)
            .or_else(|| self.root.get("syndication").and_then(|v| v.get("link_back")).and_then(Value::as_bool))
            .unwrap_or(true)
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

    /// Every declared term kind, plus the built-ins no table replaced.
    pub fn terms_kinds(&self) -> Vec<moss_core::terms::config::RawKind> {
        self.core.terms_kinds()
    }

    /// The kinds whose `[terms.<key>]` block says `nav = true`, in the order
    /// the file writes them, each with its `weight` if it set one.
    /// Whether a kind has a root page to link is decided later, from the terms
    /// the site actually has (`TermIndex::nav_entries`).
    pub fn terms_nav(&self) -> Vec<(String, Option<i32>)> {
        let mut nav: Vec<_> = self.core.terms_nav().into_iter()
            .map(|(key, weight)| (self.written_position("terms", &key), key, weight))
            .collect();
        nav.sort();
        nav.into_iter().map(|(_, key, weight)| (key, weight)).collect()
    }

    /// The `[map.layers.<name>]` tables in written order. An error names the
    /// table and key that is wrong; the build stops on it. `source` and a
    /// missing `bbox` are settled later by [`map_layers::resolve_layers`].
    pub fn map_layers(&self) -> Result<Vec<map_layers::MapLayer>, String> {
        map_layers::parse_layers(self.root.get("map"), |name| self.written_position("map.layers", name))
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
                        moss_core::schema_fields::PLACEABLE_BUILTINS.join(", ")
                    ));
                    continue;
                }
                for (key, value) in sub {
                    if !matches!(key.as_str(), "place" | "label")
                        && !(name == "tagline" && key == "type" && value.as_str() == Some("text"))
                    {
                        problems.push(format!(
                            "[fields.{name}] has \"{key}\"; built-in fields take only place and label"
                        ));
                    }
                }
                let place = Self::declared_place(sub, name, FieldKind::Text, &mut problems);
                let label = Self::declared_labels(sub, "label", name, &mut problems);
                decls.push(FieldDecl { name: name.clone(), place, label, ..Default::default() });
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
                if !matches!(key.as_str(), "type" | "of" | "place" | "label" | "text" | "in" | "card" | "pictures") {
                    problems.push(format!("[fields.{name}] has an unknown key \"{key}\""));
                }
            }
            let of = if kind == FieldKind::Group { Self::string_list(sub, "of", &format!("fields.{name}"), &mut problems) } else { Vec::new() };
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
                        .filter_map(|(v, label)| match core_value(label).labels() {
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
                (FieldKind::Group, _) => Vec::new(),
                (_, Some(_)) => {
                    problems.push(format!("[fields.{name}] has `of`, which only a choice or a group takes; ignoring it"));
                    Vec::new()
                }
                (_, None) => Vec::new(),
            };
            let place = Self::declared_place(sub, name, kind, &mut problems);
            let label = Self::declared_labels(sub, "label", name, &mut problems);
            let pictures = Self::declared_pictures(sub, name, place, &mut problems);
            let text = if kind == FieldKind::Url {
                Self::declared_labels(sub, "text", name, &mut problems)
            } else {
                if sub.contains_key("text") {
                    problems.push(format!("[fields.{name}] has `text`, which only a url field takes; ignoring it"));
                }
                Vec::new()
            };
            let scope = Self::string_list(sub, "in", &format!("fields.{name}"), &mut problems);
            let card = Self::declared_card(sub, name, kind, &mut problems);
            decls.push(FieldDecl { name: name.clone(), kind, choices, place, label, text, scope, of, card, pictures });
        }
        let declared: Vec<String> = decls.iter().filter(|d| d.kind != FieldKind::Group).map(|d| d.name.clone()).collect();
        let groups: Vec<String> = decls.iter().filter(|d| d.kind == FieldKind::Group).map(|d| d.name.clone()).collect();
        decls.retain(|group| {
            if group.kind != FieldKind::Group {
                return true;
            }
            if group.place.is_none() {
                problems.push(format!("[fields.{}] is a group with no `place`, so it draws nowhere; skipping it", group.name));
                return false;
            }
            if group.of.is_empty() {
                problems.push(format!("[fields.{}] is a group with no `of`; name the fields it draws, in order; skipping it", group.name));
                return false;
            }
            for (at, part) in group.of.iter().enumerate() {
                if group.of[..at].contains(part) {
                    problems.push(format!("[fields.{}] `of` names \"{part}\" twice; skipping the group", group.name));
                    return false;
                }
            }
            for part in &group.of {
                let reason = if *part == group.name {
                    "names itself"
                } else if groups.contains(part) {
                    "names another group, and a group cannot draw a group"
                } else if !declared.contains(part) && !moss_core::fields::GROUP_BUILTIN_PARTS.contains(&part.as_str()) {
                    "is neither a declared field, title nor date"
                } else {
                    continue;
                };
                problems.push(format!("[fields.{}] `of` names \"{part}\", which {reason}; skipping the group", group.name));
                return false;
            }
            true
        });
        (decls, problems)
    }

    /// A declaration's `place`, when it names one. `none` and an absent key both mean no place.
    fn declared_place(sub: &toml::Table, name: &str, kind: moss_core::fields::FieldKind, problems: &mut Vec<String>) -> Option<moss_core::fields::Place> {
        use moss_core::fields::{parse_place, place_names, FieldKind};
        match sub.get("place").and_then(|v| v.as_str()) {
            Some("end") if !matches!(kind, FieldKind::Page | FieldKind::Pages) => {
                problems.push(format!("[fields.{name}] has place \"end\", which draws linked pages and so needs type \"page\" or \"pages\"; it draws nowhere"));
                None
            }
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

    /// A declaration's `pictures`: how many of each linked page's pictures an `end` card draws, 1 to 12. Absent means the cover alone (0).
    fn declared_pictures(sub: &toml::Table, name: &str, place: Option<moss_core::fields::Place>, problems: &mut Vec<String>) -> usize {
        let Some(v) = sub.get("pictures") else { return 0 };
        if place != Some(moss_core::fields::Place::End) {
            problems.push(format!("[fields.{name}] has `pictures`, which only a field placed at \"end\" takes; ignoring it"));
            return 0;
        }
        match v.as_integer() {
            Some(1) => 1,
            Some(n) => match moss_core::resolve::embed_renderer::folder_list::picture_count(&n.to_string()) {
                Ok(n) => n,
                Err(why) => {
                    problems.push(format!("[fields.{name}] pictures: {why}; ignoring it"));
                    0
                }
            },
            None => {
                problems.push(format!("[fields.{name}] `pictures` must be a whole number from 1 to 12; ignoring it"));
                0
            }
        }
    }

    /// A declaration's `card`: whether listing cards draw its value. Absent means no.
    fn declared_card(sub: &toml::Table, name: &str, kind: moss_core::fields::FieldKind, problems: &mut Vec<String>) -> bool {
        match sub.get("card") {
            None => false,
            Some(Value::Boolean(true)) if kind == moss_core::fields::FieldKind::Group => {
                problems.push(format!("[fields.{name}] is a group, which holds no value for a card to draw; ignoring `card`"));
                false
            }
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                problems.push(format!("[fields.{name}] `card` must be true or false; ignoring it"));
                false
            }
        }
    }

    /// A declaration's `label` or `text` key, as `(language tag, string)` pairs; empty when absent or malformed.
    fn declared_labels(sub: &toml::Table, key: &str, name: &str, problems: &mut Vec<String>) -> Vec<(String, String)> {
        match sub.get(key) {
            None => Vec::new(),
            Some(v) => core_value(v).labels().unwrap_or_else(|| {
                problems.push(format!("[fields.{name}] `{key}` must be a string or a table of language labels; ignoring it"));
                Vec::new()
            }),
        }
    }
}

/// The parsed config in core's value shape: the desktop reads `lang` and
/// `[terms]` through core's typed reader without re-parsing the text with
/// core's narrower TOML parser. Converted by hand, not through serde: `toml`'s
/// `Value` hands a datetime to a deserializer as a string, so `lang = 1979-05-27`
/// would read as a language instead of a value to warn about.
fn core_table(table: &toml::Table) -> ConfigValue {
    ConfigValue::Table(table.iter().map(|(key, value)| (key.clone(), core_value(value))).collect())
}

fn core_value(value: &Value) -> ConfigValue {
    match value {
        Value::String(text) => ConfigValue::Text(text.clone()),
        Value::Integer(number) => ConfigValue::Integer(*number),
        Value::Float(number) => ConfigValue::Float(*number),
        Value::Boolean(on) => ConfigValue::Bool(*on),
        Value::Datetime(_) => ConfigValue::Datetime,
        Value::Array(items) => ConfigValue::Array(items.iter().map(core_value).collect()),
        Value::Table(table) => core_table(table),
    }
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
    if let Some(layers) = doc.get("map").and_then(|v| v.as_table_like()).and_then(|m| m.get("layers")).and_then(|v| v.as_table_like()) {
        orders.insert("map.layers".to_string(), keys(layers));
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
