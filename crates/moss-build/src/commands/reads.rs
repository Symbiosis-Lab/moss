//! The editor's reads: parsing, resolving, listing and describing sources.

use serde_json::Value;

use super::{CommandHost, NoArgs, VaultPath};
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

pub async fn editor_bootstrap(host: &dyn CommandHost, a: BootstrapArgs) -> Result<EditorBootstrap, String> {
    let source = a.source_path.as_ref().map(|p| host.resolve(p)).transpose()?;
    let source = source.as_deref().map(lossy);
    Ok(crate::editor::content::bootstrap_for_site(host.root(), source.as_deref(), host.editor_schema()))
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
/// [`ListDirectoryArgs`] gives. Publish dates are recomputed per call.
#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTreeArgs {
    pub show_internal: bool,
}

pub async fn list_tree(host: &dyn CommandHost, a: ListTreeArgs) -> Result<crate::editor::filesystem::TreeNode, String> {
    let root = lossy(host.root());
    crate::editor::filesystem::list_tree_inner(&root, &root, a.show_internal)
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
    let path = host.resolve(&a.file_path)?;
    crate::editor::content::describe_source_role_inner(host.root(), &lossy(&path))
}

/// A field the schema does not know is a warning, never silently accepted.
pub async fn validate_content(host: &dyn CommandHost, a: FileArgs) -> Result<Vec<EditorDiagnostic>, String> {
    let path = host.resolve(&a.file_path)?;
    // A source still in the cloud has nothing to validate.
    let Some(content) = crate::editor::content::read_vault_text_off_runtime(lossy(&path)).await? else {
        return Ok(Vec::new());
    };
    let doc = moss_core::frontmatter::parse(&content);
    Ok(crate::editor::validation::diagnose(Some(host.root()), &path, &doc.frontmatter, &host.editor_schema()))
}

/// The URL the preview serves the page at, read from the live build.
pub async fn resolve_url_for_file(
    host: &dyn CommandHost,
    a: FileArgs,
) -> Result<Option<String>, String> {
    let root = host.root();
    let abs = host.resolve(&a.file_path)?;
    let rel = abs
        .strip_prefix(root.canonicalize().as_deref().unwrap_or(root))
        .or_else(|_| abs.strip_prefix(root))
        .map_err(|_| format!("File '{}' is not inside project '{}'", a.file_path.as_written(), root.display()))?;
    crate::editor::resolve::links::resolve_url_for_file_inner(&rel.to_string_lossy(), root, &host.served_dir())
}

/// The open folder's last preflight; a caller's `folder` is not read, so no
/// request can read another folder's evidence from the process-wide record.
pub async fn get_publish_preflight(
    host: &dyn CommandHost,
    _: NoArgs,
) -> Result<Option<crate::build::types::PublishPreflightProjection>, String> {
    Ok(crate::system::build_records::records().publish_preflight(&lossy(host.root())))
}

pub async fn list_vault_terms(host: &dyn CommandHost, _: NoArgs) -> Result<crate::build::terms::VaultTerms, String> {
    let map = crate::build::scan::article_map::ArticleMap::load(&host.root().join(".moss")).unwrap_or_default();
    Ok(crate::build::terms::list_vault_terms_in(&map))
}

pub async fn get_cloud_unavailable_files(
    host: &dyn CommandHost,
    _: NoArgs,
) -> Result<Vec<String>, String> {
    crate::ops::watch::sweep::unavailable_files_for(host.root()).await
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
    if !url.is_empty() {
        host.resolve(&VaultPath::from(url))?;
    }
    crate::editor::page_source::resolve_page_source(url, host.root())
}
