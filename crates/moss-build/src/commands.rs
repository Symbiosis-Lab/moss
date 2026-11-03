//! The engine's command table: every command a moss UI sends to the engine,
//! written once as a typed function.
//!
//! A command is `async fn(&dyn CommandHost, Args) -> Result<Out, String>`,
//! where `Args` is a camelCase struct, the same JSON object every transport
//! carries. Transports (the HTTP carrier, an in-process host) differ only in
//! the [`CommandHost`] they supply, never in the command body.
//!
//! The three tiers below are the trust classes a transport may expose:
//! public reads, reads that need the session credential, and writes. A
//! command is in exactly one.

use std::future::Future;
use std::path::{Path, PathBuf};

use futures::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::build::manifest::SealedManifest;
use crate::editor::resolve::asset_resolver::EditorScope;
use crate::types::events::MossEvent;
use rename_undo::RenameUndoStore;

pub mod files;
pub mod reads;
pub mod rename_undo;
pub mod versions;

#[cfg(test)]
mod tests;

/// What a command needs from whoever runs it.
pub trait CommandHost: Send + Sync {
    /// The open folder.
    fn root(&self) -> &Path;

    /// Turn a caller's path into a real one. A transport whose caller is
    /// untrusted confines it to [`Self::root`]; an in-process host whose
    /// caller is moss's own UI may resolve outside it (a loose document).
    fn resolve(&self, path: &VaultPath) -> Result<PathBuf, String>;

    fn scope(&self) -> EditorScope {
        EditorScope::Site
    }

    fn rename_undo(&self) -> &RenameUndoStore;

    /// The publish dates the folder's tree walks have read, kept between walks.
    fn publish_dates(&self) -> &crate::editor::filesystem::PublishDateMemo;

    /// Root-relative page moves a command is about to make, for a host whose
    /// watcher would otherwise read them as a delete and a create.
    fn announce_moves(&self, _pairs: &[(String, String)]) {}

    /// Moves announced by [`Self::announce_moves`] that did not happen.
    fn withdraw_moves(&self, _pairs: &[(String, String)]) {}

    /// The schema the editor checks frontmatter against. A host with a live
    /// plugin runtime answers from it; the default reads a site's installed
    /// plugins and config from disk, and a loose document gets the built-in one.
    fn editor_schema(&self) -> moss_core::schema::ContentSchema {
        match self.scope() {
            EditorScope::Site => crate::editor::validation::schema_on_disk(self.root()),
            EditorScope::Folder => crate::editor::validation::compose_schema(&[], &[]),
        }
    }

    /// Where the preview is serving the folder's current build from.
    fn served_dir(&self) -> PathBuf;

    /// Tell the folder's listeners what a command did.
    fn publish(&self, event: &MossEvent);

    /// The site as it stands now, sealed. A host without a live build builds.
    fn sealed_now(&self) -> BoxFuture<'_, Result<SealedManifest, String>>;
}

/// A path a caller names. Its only way to a real path is
/// [`CommandHost::resolve`], so no command can skip the host's confinement.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(transparent)]
pub struct VaultPath(String);

impl VaultPath {
    /// The path as the caller wrote it, for messages and path arithmetic
    /// that never touches the filesystem.
    pub fn as_written(&self) -> &str {
        &self.0
    }
}

impl From<&str> for VaultPath {
    fn from(s: &str) -> Self {
        VaultPath(s.to_string())
    }
}

impl From<String> for VaultPath {
    fn from(s: String) -> Self {
        VaultPath(s)
    }
}

/// The open folder when it is a site; a loose document's folder is not one, so
/// a command that answers with site facts (URLs, terms, roles) refuses there.
fn site_root(host: &dyn CommandHost) -> Result<&Path, String> {
    match host.scope() {
        EditorScope::Site => Ok(host.root()),
        EditorScope::Folder => Err(crate::vault::paths::NO_PROJECT_OPEN.to_string()),
    }
}

/// The site root and `path` relative to it. A host may resolve to the
/// canonical spelling of a symlinked root, so both spellings are tried.
fn site_relative<'h>(host: &'h dyn CommandHost, path: &VaultPath) -> Result<(&'h Path, String), String> {
    let root = site_root(host)?;
    let abs = host.resolve(path)?;
    let rel = abs
        .strip_prefix(root.canonicalize().as_deref().unwrap_or(root))
        .or_else(|_| abs.strip_prefix(root))
        .map_err(|_| format!("File '{}' is not inside project '{}'", path.as_written(), root.display()))?;
    Ok((root, rel.to_string_lossy().into_owned()))
}

/// Run `apply` with its page moves announced to the host first, and withdrawn
/// if it fails.
fn with_moves_announced<T>(
    host: &dyn CommandHost,
    pairs: &[(String, String)],
    apply: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    host.announce_moves(pairs);
    let out = apply();
    if out.is_err() {
        host.withdraw_moves(pairs);
    }
    out
}

/// Run one command for an in-process host, with its arguments already typed.
/// A panic inside the command answers as an error, as it does on every path.
pub async fn call<'h, A, T, Fut>(
    host: &'h dyn CommandHost,
    command: impl FnOnce(&'h dyn CommandHost, A) -> Fut,
    args: A,
) -> Result<T, String>
where
    Fut: Future<Output = Result<T, String>> + 'h,
{
    let name = name_of(&command);
    contained(name, command(host, args)).await.and_then(|out| out)
}

/// A command function's own name, for messages.
fn name_of<F>(_: &F) -> &'static str {
    std::any::type_name::<F>().rsplit("::").next().unwrap_or("command")
}

/// The one panic boundary every path into a command goes through: a
/// panicking command answers as an error instead of unwinding into its host.
async fn contained<T>(name: &str, run: impl Future<Output = T>) -> Result<T, String> {
    use futures::FutureExt;
    std::panic::AssertUnwindSafe(run).catch_unwind().await.map_err(|payload| {
        let msg = crate::editor::content::panic_message_from_payload(&*payload);
        log::error!("Engine command `{name}` panicked: {msg}");
        format!("Internal error in `{name}`: {msg}")
    })
}

/// Why a command did not answer: its arguments did not parse (the caller's
/// fault) or the command itself failed.
#[derive(Debug)]
pub enum CommandError {
    BadArgs(String),
    Failed(String),
}

pub type CommandResult = Result<Value, CommandError>;

async fn run<'h, A, T, Fut>(
    host: &'h dyn CommandHost,
    args: Value,
    command: impl FnOnce(&'h dyn CommandHost, A) -> Fut,
) -> CommandResult
where
    A: DeserializeOwned,
    T: serde::Serialize,
    Fut: Future<Output = Result<T, String>> + 'h,
{
    // A command without arguments may be sent no body at all.
    let args = if args.is_null() { Value::Object(Default::default()) } else { args };
    let args: A = serde_json::from_value(args).map_err(|e| CommandError::BadArgs(e.to_string()))?;
    let out = call(host, command, args).await.map_err(CommandError::Failed)?;
    serde_json::to_value(out).map_err(|e| CommandError::Failed(format!("serialize result: {e}")))
}

macro_rules! tier {
    ($(#[$m:meta])* $tier:ident { $($cmd:ident => $f:path),+ $(,)? }) => {
        $(#[$m])*
        pub mod $tier {
            use super::*;

            pub const COMMANDS: &[&str] = &[$(stringify!($cmd)),+];

            /// `None` when `cmd` is not in this tier.
            pub async fn dispatch(host: &dyn CommandHost, cmd: &str, args: Value) -> Option<CommandResult> {
                match cmd {
                    $(stringify!($cmd) => Some(run(host, args, $f).await),)+
                    _ => None,
                }
            }

            /// Parse `args` with `cmd`'s own argument type, running nothing.
            #[cfg(test)]
            pub(crate) fn parse(cmd: &str, args: Value) -> Option<Result<(), String>> {
                match cmd {
                    $(stringify!($cmd) => Some(parses_for(&$f, args)),)+
                    _ => None,
                }
            }

            /// This tier's name and each command with its argument and answer types.
            #[cfg(test)]
            pub(crate) fn signatures(types: &mut specta::TypeMap) -> (&'static str, Vec<Signature>) {
                let commands = vec![$({
                    let (args, out) = signature_of(&$f, types);
                    (stringify!($cmd), args, out)
                }),+];
                (stringify!($tier), commands)
            }
        }
    };
}

#[cfg(test)]
type Signature = (&'static str, specta::DataType, specta::DataType);

#[cfg(test)]
fn signature_of<'h, A: specta::Type, T: specta::Type, Fut>(
    _command: &impl FnOnce(&'h dyn CommandHost, A) -> Fut,
    types: &mut specta::TypeMap,
) -> (specta::DataType, specta::DataType)
where
    Fut: Future<Output = Result<T, String>> + 'h,
{
    (A::reference(types, &[]).inner, T::reference(types, &[]).inner)
}

#[cfg(test)]
fn parses_for<'h, A: DeserializeOwned, Fut>(
    _command: &impl FnOnce(&'h dyn CommandHost, A) -> Fut,
    args: Value,
) -> Result<(), String> {
    serde_json::from_value::<A>(args).map(|_| ()).map_err(|e| e.to_string())
}

tier! {
    /// Reads that expose nothing beyond what the published site shows.
    public_read {
        scan_shortcodes => reads::scan_shortcodes,
        parse_frontmatter => reads::parse_frontmatter,
        editor_resolve_asset => reads::editor_resolve_asset,
        editor_resolve_references => reads::editor_resolve_references,
    }
}

tier! {
    /// Reads that expose the whole folder (its tree, every source's role, its
    /// history), so they need the session credential.
    session_read {
        editor_bootstrap => reads::editor_bootstrap,
        list_directory => reads::list_directory,
        list_tree => reads::list_tree,
        get_file_info => reads::get_file_info,
        compute_heading_state => reads::compute_heading_state,
        describe_source_role => reads::describe_source_role,
        page_facts => reads::page_facts,
        resolve_url_for_file => reads::resolve_url_for_file,
        validate_content => reads::validate_content,
        get_publish_preflight => reads::get_publish_preflight,
        list_vault_terms => reads::list_vault_terms,
        get_cloud_unavailable_files => reads::get_cloud_unavailable_files,
        list_versions => versions::list_versions,
        read_version => versions::read_version,
        reveal_history_store => versions::reveal_history_store,
        resolve_attachment_dir => reads::resolve_attachment_dir,
        resolve_page_source => reads::resolve_page_source,
    }
}

tier! {
    /// Commands that write into the folder.
    write {
        create_files => files::create_files,
        create_folder => files::create_folder,
        delete_entry => files::delete_entry,
        delete_entries => files::delete_entries,
        rename_entry_with_refs => files::rename_entry_with_refs,
        move_entries => files::move_entries,
        scan_references_for_delete => files::scan_references_for_delete,
        clean_references_and_delete => files::clean_references_and_delete,
        undo_rename => files::undo_rename,
        save_editor_content => files::save_editor_content,
        restore_version => versions::restore_version,
        save_version => versions::save_version,
    }
}

/// A command that takes no arguments. Extra keys a caller sends are ignored.
#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct NoArgs {}
