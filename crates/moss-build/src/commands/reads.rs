//! The editor's reads: parsing, resolving, listing and describing sources.

use serde_json::Value;

use super::{site_root, CommandHost, NoArgs, VaultPath};
use crate::editor::resolve::asset_resolver::EditorScope;
use crate::editor::content::{EditorBootstrap, ParsedFrontmatter, SourceRole};
use crate::editor::filesystem::EditorFileInfo;
use crate::editor::resolve::asset_resolver::ResolvedAsset;
use crate::editor::resolve::reference_resolver::RefTarget;
use crate::editor::validation::EditorDiagnostic;

fn lossy(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

#[derive(serde::Deserialize, specta::Type)]
pub struct ScanShortcodesArgs {
    pub text: String,
}

pub async fn scan_shortcodes(
    _host: &dyn CommandHost,
    a: ScanShortcodesArgs,
) -> Result<moss_core::ast::editor_scan::EditorScanResult, String> {
    Ok(moss_core::ast::editor_scan::editor_scan(&a.text))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileArgs {
    pub file_path: VaultPath,
}

pub async fn parse_frontmatter(host: &dyn CommandHost, a: FileArgs) -> Result<ParsedFrontmatter, String> {
    crate::editor::content::parse_frontmatter_from_disk(lossy(&host.resolve(&a.file_path)?)).await
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAssetArgs {
    pub target: String,
    pub from_file: VaultPath,
}

pub async fn editor_resolve_asset(host: &dyn CommandHost, a: ResolveAssetArgs) -> Result<Option<ResolvedAsset>, String> {
    let from_file = host.resolve(&a.from_file)?;
    Ok(crate::editor::resolve::asset_resolver::resolve_asset(&a.target, &from_file, host.root(), host.scope()))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveReferencesArgs {
    pub targets: Vec<RefTarget>,
    pub from_file: VaultPath,
}

pub async fn editor_resolve_references(
    host: &dyn CommandHost,
    a: ResolveReferencesArgs,
) -> Result<Vec<crate::editor::resolve::reference_resolver::EditorReferenceResolution>, String> {
    let from_file = host.resolve(&a.from_file)?;
    Ok(crate::editor::resolve::reference_resolver::resolve_references_batch(
        &a.targets,
        &from_file.to_string_lossy(),
        host.root(),
        host.scope(),
    ))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapArgs {
    /// A bare boot opens no file.
    pub source_path: Option<VaultPath>,
}

/// A site answers `project_path`; a loose document answers `editor_root`, the
/// document's folder, which has no preview.
pub async fn editor_bootstrap(host: &dyn CommandHost, a: BootstrapArgs) -> Result<EditorBootstrap, String> {
    use crate::editor::content::{assemble_bootstrap, bootstrap_source_path, read_vault_text_off_runtime};
    let root = lossy(host.root());
    let source = a.source_path.as_ref().map(|p| host.resolve(p)).transpose()?;
    let initial = match bootstrap_source_path(Some(&root), source.as_deref().map(lossy).as_deref()) {
        // A file still in the cloud boots without its parse; the editor loads it.
        Some(abs) => read_vault_text_off_runtime(abs).await.ok().flatten(),
        None => None,
    };
    let (project_path, editor_root) = match host.scope() {
        EditorScope::Site => (Some(root), None),
        EditorScope::Folder => (None, Some(root)),
    };
    Ok(assemble_bootstrap(project_path, editor_root, host.editor_schema(), initial))
}

/// A caller's `projectPath` is not read: a caller that names both the root
/// and the path checked against it can pass any containment test.
#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListDirectoryArgs {
    pub path: VaultPath,
    pub show_internal: bool,
}

pub async fn list_directory(
    host: &dyn CommandHost,
    a: ListDirectoryArgs,
) -> Result<Vec<crate::editor::filesystem::DirEntry>, String> {
    let path = host.resolve(&a.path)?;
    crate::editor::filesystem::list_directory_inner(&lossy(&path), &lossy(host.root()), a.show_internal)
}

/// Walks the open folder; a caller's `path` is not read, for the reason
/// [`ListDirectoryArgs`] gives. A loose document's folder is listed only to a
/// budget, since it can be any folder on the machine.
#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTreeArgs {
    pub show_internal: bool,
}

pub async fn list_tree(host: &dyn CommandHost, a: ListTreeArgs) -> Result<crate::editor::filesystem::TreeNode, String> {
    use crate::editor::filesystem::{bounded_tree, list_tree_inner_cached, DOCUMENT_TREE_BUDGET};
    let root = lossy(host.root());
    match host.scope() {
        EditorScope::Folder => bounded_tree(&root, a.show_internal, DOCUMENT_TREE_BUDGET),
        EditorScope::Site => host
            .publish_dates()
            .with_folder(host.root(), |cache| list_tree_inner_cached(&root, &root, a.show_internal, cache)),
    }
}

pub async fn get_file_info(host: &dyn CommandHost, a: FileArgs) -> Result<EditorFileInfo, String> {
    crate::editor::content::get_file_info_inner(&lossy(&host.resolve(&a.file_path)?))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HeadingStateArgs {
    pub file_path: VaultPath,
    pub frontmatter: Value,
    pub body: String,
}

pub async fn compute_heading_state(
    host: &dyn CommandHost,
    a: HeadingStateArgs,
) -> Result<moss_core::heading::HeadingState, String> {
    let path = host.resolve(&a.file_path)?;
    Ok(crate::editor::content::compute_heading_state_inner(&lossy(&path), &a.frontmatter, &a.body))
}

pub async fn describe_source_role(host: &dyn CommandHost, a: FileArgs) -> Result<SourceRole, String> {
    let root = site_root(host)?;
    let path = host.resolve(&a.file_path)?;
    crate::editor::content::describe_source_role_inner(root, &lossy(&path))
}

/// A field the schema does not know is a warning, never silently accepted.
pub async fn validate_content(host: &dyn CommandHost, a: FileArgs) -> Result<Vec<EditorDiagnostic>, String> {
    let path = host.resolve(&a.file_path)?;
    // A source still in the cloud has nothing to validate.
    let Some(content) = crate::editor::content::read_vault_text_off_runtime(lossy(&path)).await? else {
        return Ok(Vec::new());
    };
    let doc = moss_core::frontmatter::parse(&content);
    let site = site_root(host).ok();
    Ok(crate::editor::validation::diagnose(site, &path, &doc.frontmatter, &host.editor_schema()))
}

/// The URL the preview serves the page at, read from the live build.
pub async fn resolve_url_for_file(
    host: &dyn CommandHost,
    a: FileArgs,
) -> Result<Option<String>, String> {
    let root = site_root(host)?;
    let abs = host.resolve(&a.file_path)?;
    let rel = abs
        .strip_prefix(root.canonicalize().as_deref().unwrap_or(root))
        .or_else(|_| abs.strip_prefix(root))
        .map_err(|_| format!("File '{}' is not inside project '{}'", a.file_path.as_written(), root.display()))?;
    crate::editor::resolve::links::resolve_url_for_file_inner(&rel.to_string_lossy(), root, &host.served_dir())
}

/// The open folder's last preflight. A request about any other folder is
/// refused, so no caller reads another folder's evidence from the
/// process-wide record, and a caller keying answers by folder is never handed
/// the wrong one.
pub async fn get_publish_preflight(
    host: &dyn CommandHost,
    a: FolderArgs,
) -> Result<Option<crate::build::types::PublishPreflightProjection>, String> {
    require_open_folder(host.root(), &a.folder_path)?;
    Ok(crate::system::build_records::records().publish_preflight(&lossy(host.root())))
}

fn require_open_folder(root: &std::path::Path, asked: &str) -> Result<(), String> {
    match (root.canonicalize(), std::path::Path::new(asked).canonicalize()) {
        (Ok(open), Ok(asked)) if open == asked => Ok(()),
        _ => Err("Preview target belongs to a different folder".to_string()),
    }
}

pub async fn list_vault_terms(host: &dyn CommandHost, _: NoArgs) -> Result<crate::build::terms::VaultTerms, String> {
    let map = crate::build::scan::article_map::ArticleMap::load(&site_root(host)?.join(".moss")).unwrap_or_default();
    Ok(crate::build::terms::list_vault_terms_in(&map))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderArgs {
    /// The folder the caller asks about; refused if it is not the open one.
    pub folder_path: String,
}

/// A host whose folder can change while this waits checks again after it.
pub async fn get_cloud_unavailable_files(host: &dyn CommandHost, a: FolderArgs) -> Result<Vec<String>, String> {
    let root = site_root(host)?;
    require_open_folder(root, &a.folder_path)?;
    crate::ops::watch::sweep::unavailable_files_for(root).await
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDirArgs {
    pub page_relative_path: VaultPath,
}

/// The project-relative folder (`""` is the root) where a file dropped on the
/// page belongs. It does not create the folder: this is a read.
pub async fn resolve_attachment_dir(host: &dyn CommandHost, a: AttachmentDirArgs) -> Result<String, String> {
    host.resolve(&a.page_relative_path)?;
    let raw = crate::build::site_config::load_attachment_folder(&lossy(host.root()));
    Ok(moss_core::attachment::attachment_dir_for_page(&raw, &a.page_relative_path.as_written().replace('\\', "/")))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PageSourceArgs {
    pub url_path: String,
}

/// The source page that produces a served URL (the home page for `""`/`"/"`).
pub async fn resolve_page_source(
    host: &dyn CommandHost,
    a: PageSourceArgs,
) -> Result<crate::editor::resolve::links::PageSource, String> {
    let url = a.url_path.trim_start_matches('/');
    let root = site_root(host)?;
    if !url.is_empty() {
        host.resolve(&VaultPath::from(url))?;
    }
    crate::editor::page_source::resolve_page_source(url, root)
}
