//! Reading `.moss/config.toml` — the half of it that belongs to the open crate.
//!
//! # Who owns this file
//!
//! The **app** owns `.moss/config.toml`: every setting is written from a
//! settings modal, and every writer lives in the app crate.
//! What ships here is the **reader**, because the CLI reads config too and
//! `moss-cli` cannot depend on the app crate (`scripts/check-crate-dag.mjs`
//! rule 4). Ownership did not move; the reader's home did.
//!
//! # What is here, and what deliberately is not
//!
//! Here: turning the *text* of a config file into values — parse, then look a
//! key up. Not here: opening the file. That read is eviction-aware on macOS
//! (an iCloud-evicted config must never read as "absent", or a default is
//! written over the user's settings), and the code that proves absence from an
//! error lives in the build tree's cloud modules, which do not cross into this
//! crate until a later step. So a caller reads the bytes however its platform
//! requires and hands the string to [`ConfigFile::parse`]:
//!
//! - the app reads through `build::cloud_readiness` (see
//!   `domain::config::read_managed_toml`), which waits for a materializing
//!   file rather than mistaking it for a missing one;
//! - a future `moss-cli` reads with `std::fs::read_to_string` **only where
//!   that is the whole story**. It is not the whole story on macOS, which
//!   moss-cli ships on: an iCloud-evicted config returns `NotFound` there, and
//!   a CLI that believed it would build the user's site with every setting at
//!   its default. The CLI needs the eviction-aware read too — which is one more
//!   reason that cluster crosses the crate line at step 4.
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
        Ok(Self { root: table })
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
        crate::config::migrations::version_ahead(&self.root)
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
    /// no prefix are explicit passthrough roots; a `!` prefix opts a directory
    /// OUT of auto-detection. Empty when the key is absent.
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
    /// `TermKind`s. Raw here: `fields` is not yet filtered through
    /// `moss_core::schema_fields::name_list_fields()`, and `title` is not
    /// yet resolved — both happen exactly once, in `term_kinds`, not here.
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
                declared.push(RawKind { key: key.clone(), fields, title, kind_type });
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
            });
        }
        kinds.extend(declared);
        kinds
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
    /// Frontmatter field names as configured, not yet checked against
    /// `moss_core::schema_fields::name_list_fields()`.
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
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
