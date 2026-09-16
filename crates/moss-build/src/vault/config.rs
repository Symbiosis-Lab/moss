//! Writing a managed `.moss` TOML file — the one primitive every writer goes
//! through, and the writers that have an open-binary caller. `config.toml` is
//! the file the rules below are written about; `state.toml`'s writers use the
//! same primitive, from `super::deployment_state`. The `[site]` writer family
//! joined them 2026-09-08 (track C4c): `moss domain` writes `[site].domain`
//! from the terminal, so that door cannot be app-only either. `set_domain_in_config`
//! joined 2026-09-15 for the same reason — `moss domain link` needed the exact
//! write `save_domain_selection` (desktop) already makes, dns-observation reset
//! included, rather than a second hand-rolled one.
//!
//! [ADR-059](../../../../../docs/decisions/ADR-059-config-reader-and-migration-runner-after-the-crate-split.md)
//! keeps the app the owner of this file: its settings modals write it, the CLI
//! reads it, advanced users hand-edit it. The 2026-09-07 amendment moved the
//! *primitive* here — a load that substitutes an empty table only for a file
//! proven absent, a render that edits the original bytes instead of
//! re-emitting them, and a byte-identity guard so a no-op save is no write —
//! because `moss env <name> <folder>` is a terminal verb both binaries answer
//! and it has to write through the same door the app does. The compiler's one
//! pre-existing rewrite (plugin uninstall) now goes through it too, which is
//! what closed the breach the amendment names. Everything else the app's
//! settings modals write alone stays in `domain/config.rs`.

use crate::build::site_config::read_managed_toml;
use crate::config::environment::HostingEnvironment;
use std::path::Path;
use toml::{Table, Value};

/// A managed TOML file the caller is about to rewrite: the value tree to edit,
/// and the exact bytes it was parsed from.
///
/// Keeping the bytes is what lets the save be an **edit**. `config.toml` is
/// hand-written; re-emitting it from `root` alone deletes the user's comments,
/// blank lines, key order and quoting — on 2026-08-07 that silently cost a
/// real site four lines of comment.
///
/// So every caller saves through [`write_managed_toml`] (which renders with
/// `infra::toml_rewrite::apply_changes`), never with `toml::to_string_pretty`.
///
/// `root` is a [`Table`], not a `Value`: a TOML *document* is a table by
/// definition, so `toml::from_str` on one cannot yield anything else. Typing
/// it that way deleted the writers' top-level "is it a table?" guards, which
/// asked a question the parser had already answered. A *section* inside the
/// document is a different matter — `services = 5` is a document the user can
/// hand-write — and that check survives, once, in [`subtable`].
pub struct ManagedToml {
    pub original: String,
    pub root: Table,
}

/// Loads a TOML file the caller is about to rewrite, or an empty document when
/// the file is genuinely absent.
///
/// An empty table is only ever substituted for a file proven not to exist.
/// Every caller writes the result straight back, so standing in an empty table
/// for a file that merely could not be read or parsed — evicted, half-synced,
/// permissions — replaces every setting the user has with the one field being
/// saved.
pub fn load_managed_toml(path: &Path) -> Result<ManagedToml, String> {
    let Some(original) = read_managed_toml(path)? else {
        return Ok(ManagedToml {
            original: String::new(),
            root: Table::new(),
        });
    };
    let root = toml::from_str(&original).map_err(|e| {
        format!("{} is malformed and was left untouched: {}", path.display(), e)
    })?;
    Ok(ManagedToml { original, root })
}

/// Render the edited value tree back over the bytes it was read from, then
/// write it — **unless the render is byte-identical to what was read**.
///
/// `.moss/config.toml` is deliberately watched, so every byte written to it
/// triggers a rebuild. Some writers run on paths that re-save a value the
/// file already holds — a migration at build entry, a modal that saves the
/// state it just loaded — and for those an unconditional write turns "save"
/// into "rebuild", and a save at build entry into a rebuild that saves again.
///
/// The guard is byte identity of the rendered file, not equality of the one
/// field being saved, because `toml_rewrite::apply_changes` is surgical: an
/// edit that changes nothing renders the original bytes back exactly. One
/// check therefore covers every writer, including those that rewrite a whole
/// section. Until 2026-08-31 this check lived inside the site-language
/// writer alone, which made "does not thrash the watcher" a property of one
/// function rather than of writing config; that writer has since gone.
///
/// The write commits through [`crate::infra::atomic_write::write_atomic`], so
/// a reader — the watcher rebuilding on the change, or the other binary —
/// never observes a half-written config, and two writers racing cannot rename
/// each other's bytes into place. Until 2026-09-07 only `save_environment`
/// committed atomically; nobody has argued for a config write that may be
/// observed half-written, so it is now the one way.
pub fn write_managed_toml(path: &Path, original: &str, root: &Table) -> Result<(), String> {
    let out = crate::infra::toml_rewrite::apply_changes(original, root)?;
    if out == original {
        return Ok(());
    }
    crate::infra::atomic_write::write_atomic(path, &out)
}

/// The nested table at `key`, created empty if it is missing.
///
/// Every writer of a managed TOML file reaches one or two levels into the
/// document to set a field. Written out, that is an
/// `entry().or_insert_with(Value::Table)` followed by an `as_table_mut()`, and
/// the `None` arm was handled at three different strengths across the writers
/// — an error in some, an `if let` with no `else` in others, which is a save
/// that silently did nothing.
///
/// The arm is reachable, so it is an error rather than a panic: a user may
/// hand-write `services = 5` in a config, and the honest answer to
/// saving a field under it is to say which key is in the way, not to
/// overwrite what they typed and not to report success having done nothing.
pub fn subtable<'a>(table: &'a mut Table, key: &str) -> Result<&'a mut Table, String> {
    table
        .entry(key.to_string())
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("`{key}` is not a table; moss left the file alone"))
}

/// Write the top-level `environment` key into `.moss/config.toml`.
pub fn save_environment(project_path: &str, env: HostingEnvironment) -> Result<(), String> {
    let config_path = Path::new(project_path).join(".moss").join("config.toml");
    let ManagedToml { original, mut root } = load_managed_toml(&config_path)?;
    root.insert("environment".to_string(), Value::String(env.name().to_string()));
    write_managed_toml(&config_path, &original, &root)
}

/// Write one `[site]` field — the shared frame behind the typed wrappers
/// [`save_site_str`] and [`save_site_bool`], which were byte-level twins of
/// each other until 2026-08-31.
fn save_site_value(project_path: &str, field: &str, value: Value) -> Result<(), String> {
    let config_path = Path::new(project_path).join(".moss").join("config.toml");
    let ManagedToml { original, mut root } = load_managed_toml(&config_path)?;
    subtable(&mut root, "site")?.insert(field.to_string(), value);
    write_managed_toml(&config_path, &original, &root)
}

/// Write a string field into `[site]`.
pub fn save_site_str(project_path: &str, field: &str, value: &str) -> Result<(), String> {
    save_site_value(project_path, field, Value::String(value.to_string()))
}

/// Write a boolean field into `[site]`.
pub fn save_site_bool(project_path: &str, field: &str, value: bool) -> Result<(), String> {
    save_site_value(project_path, field, Value::Boolean(value))
}

/// Set the authored domain — `[site].domain` in config.toml, the one place
/// the build reads it from. When the domain actually changed, resets the
/// observation: what was verified was verified about the OLD domain. The
/// deployer's `dns_target` survives the reset — it describes where to point
/// a domain (the deploy target's endpoints), not which domain points there,
/// and the deploy-first flow sets the domain AFTER the deploy that supplied
/// it. This is the single code path used by `save_domain_selection`,
/// `complete_domain_purchase` (both desktop) and `moss domain link` (CLI) —
/// moved here from the app's `domain/config.rs` (2026-09-15) so the terminal
/// verb doesn't have to reimplement the observation reset inline.
pub fn set_domain_in_config(folder_path: &str, domain: &str) -> Result<(), String> {
    let domain_changed = crate::build::site_config::get_domain_config(folder_path)?
        .domain
        .as_deref()
        != Some(domain);
    save_site_str(folder_path, "domain", domain)?;
    if domain_changed {
        crate::vault::deployment_state::update_domain_observation(folder_path, |o| {
            o.dns_target.map(|target| crate::config::deployment::DomainObservation {
                dns_target: Some(target),
                ..Default::default()
            })
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::site_config::{get_environment_field, get_site_lang};
    use std::fs;

    #[test]
    fn save_environment_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_str().unwrap();
        save_environment(path, HostingEnvironment::Staging).unwrap();
        assert_eq!(get_environment_field(path).unwrap(), Some("staging".to_string()));
    }

    #[test]
    fn save_environment_preserves_pre_existing_keys_and_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let moss = tmp.path().join(".moss");
        fs::create_dir_all(&moss).unwrap();
        fs::write(moss.join("config.toml"), "# kept\n[site]\nlang = \"en\"\n").unwrap();
        let path = tmp.path().to_str().unwrap();
        save_environment(path, HostingEnvironment::Staging).unwrap();
        assert_eq!(get_environment_field(path).unwrap(), Some("staging".to_string()));
        assert_eq!(get_site_lang(path).unwrap(), Some("en".to_string()));
        let bytes = fs::read_to_string(moss.join("config.toml")).unwrap();
        assert!(bytes.contains("# kept"), "the hand-written comment must survive: {bytes}");
    }

    #[test]
    fn a_no_op_save_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_str().unwrap();
        save_environment(path, HostingEnvironment::Local).unwrap();
        let config = tmp.path().join(".moss/config.toml");
        let before = fs::metadata(&config).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        save_environment(path, HostingEnvironment::Local).unwrap();
        assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), before, "a re-save of the same value must not touch the file");
        let strays: Vec<_> = fs::read_dir(tmp.path().join(".moss"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(strays.is_empty(), "no temp file may linger: {strays:?}");
    }
}
