// @generated from moss-build's command table. Do not edit.
// Regenerate: UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract

/**
 * One condition under which a field takes effect.
 * Serialized as `{"position": "folder"}` or `{"set": "start"}`.
 */
export type AppliesWhen = 
/**
 * The page sits in this position.
 */
{ position: PagePosition } | 
/**
 * The page's frontmatter sets this other field to a non-empty value.
 */
{ set: string }

export type AssetProvenance = "literal" | "bare-fuzzy" | "separator-fallback" | "case-mismatch"

export type AttachmentDirArgs = { pageRelativePath: VaultPath }

export type BootstrapArgs = { 
/**
 * A bare boot opens no file.
 */
sourcePath: VaultPath | null }

export type ChangedPage = { source_path: string; verb: PageVerb }

/**
 * The top-level content schema.
 */
export type ContentSchema = { 
/**
 * Generator that produced this schema (e.g. "moss").
 */
generator: string; 
/**
 * Schema format version.
 */
version: string; 
/**
 * Frontmatter field definitions.
 */
frontmatter: FrontmatterSchema; 
/**
 * Optional shortcode definitions.
 */
shortcodes: ShortcodeSchema | null }

export type CreateFilesArgs = { files: NewFile[] }

export type CreateFolderArgs = { parentDir: string; name: string }

/**
 * Provenance of a resolved publish date. Distinguishes explicit dates
 * (`Frontmatter`, `FilenamePrefix`) from implicit fallbacks (`Ctime`),
 * so consumers that care (e.g. file-tree zoning, RSS feed pubDate)
 * can treat them differently.
 */
export type DateSource = "frontmatter" | "filename_prefix" | "ctime" | "none"

/**
 * A directory entry returned by `list_directory`.
 */
export type DirEntry = { 
/**
 * File or folder name (not the full path).
 */
name: string; 
/**
 * Absolute path to the entry.
 */
path: string; 
/**
 * Whether this entry is a directory.
 */
is_dir: boolean; 
/**
 * Last modified time as seconds since UNIX epoch. None if unavailable.
 */
modified: number | null; 
/**
 * Opaque cross-platform identity for this filesystem entry, serialized as
 * decimal u64 (`Some("42")`) or `None` when not extractable. Used by
 * EntryRegistry.reconcileSnapshot to reuse EntryIds when a rename moves
 * an existing file_id to a new path.
 * 
 * Unix: `Some(metadata.ino().to_string())` via MetadataExt.
 * Windows (v1): always `None` — stable Rust doesn't expose file_index
 * without nightly. Future PR can wire the `file-id` crate (already
 * transitive via notify-debouncer-full) for proper Windows support.
 */
file_id: string | null }

/**
 * A link from a published page to a `draft: true` page, which is built for
 * preview and never published.
 */
export type DraftLinkOccurrence = { 
/**
 * Markdown file holding the link, relative to the site root.
 */
source_path: string; 
/**
 * The link as the built page carries it.
 */
reference: string; 
/**
 * The draft page's source file.
 */
draft_source: string; 
/**
 * One-based line of the link in `source_path`, when it could be found.
 */
line: number | null }

/**
 * Everything the editor webview needs for first paint, in one IPC round trip.
 */
export type EditorBootstrap = { 
/**
 * Composed content schema (builtin + plugin fields) — same source as `get_schema`.
 */
schema: ContentSchema; 
/**
 * Project root, or None when no project is open (the editor still boots;
 * the file tree is skipped).
 */
project_path: string | null; 
/**
 * Best-effort parse of the editor URL's `source` param. None when there is
 * no source, it isn't markdown, or the parse failed — the frontend falls
 * back to `parse_frontmatter`, which re-surfaces the real error through
 * the existing logging path.
 */
initial_file: ParsedFrontmatter | null; 
/**
 * THE project-root folder name; `None` with no project open, or for `/`.
 * EMITTED, not re-derived (same rule as `TreeNode.is_home`): the site's home
 * note is `<root_name>.md`, and only a name matching the REAL root basename is
 * elected as `/` by `moss_core::home::is_home_file`.
 */
root_name: string | null; 
/**
 * The editor's path-domain root when there is NO project: the loose file's
 * parent directory (document mode).
 * 
 * Never `Some` at the same time as `project_path` — they are the two arms
 * of one choice, kept as separate fields so the frontend can tell "a site
 * I can preview" from "a folder I merely need for path arithmetic". The
 * editor's identity registry is project-root-relative and *throws* on an
 * absolute path, so a document-mode editor still needs a root even though
 * nothing about that folder is a site — and emitting `project_path` for it
 * would be a lie the preview and deploy paths would act on.
 */
editor_root: string | null }

/**
 * A validation diagnostic exposed to the frontend.
 * 
 * Lives here rather than in the app crate because both carriers return it:
 * the desktop `validate_content` command and the HTTP arm.
 */
export type EditorDiagnostic = { 
/**
 * Severity: 1=Error, 2=Warning, 3=Info, 4=Hint (LSP-compatible).
 */
severity: number; 
/**
 * Human-readable message.
 */
message: string; 
/**
 * Frontmatter field path (e.g. "title", "also_in[0]").
 */
path: string | null; 
/**
 * Source line (1-based), if available.
 */
line: number | null; 
/**
 * Source column (1-based), if available.
 */
column: number | null }

/**
 * One cell divider line, and whether the author used the old spelling.
 * 
 * `legacy_dash` on the whole result says "somewhere in this document";
 * this says *which line*, which is what the editor needs to put a hint
 * next to the divider the author actually typed.
 */
export type EditorDivider = { 
/**
 * Source range covering the `+++` or `---` characters only.
 */
range: EditorRange; 
/**
 * True for the deprecated `---` form, false for canonical `+++`.
 */
legacy: boolean }

/**
 * Metadata about a file for the editor's file viewer.
 */
export type EditorFileInfo = { 
/**
 * File size in bytes.
 */
size: number; 
/**
 * Last modified time as ISO 8601 string, if available.
 */
modified: string | null; 
/**
 * Image width in pixels (images only).
 */
width: number | null; 
/**
 * Image height in pixels (images only).
 */
height: number | null }

/**
 * Half-open byte-offset range `[from, to)` into the original markdown
 * source. Bytes, not characters: positions are taken straight from
 * `&str` slicing math so they line up with what the editor receives over
 * the wire (the markdown is shipped as a `String`, and CodeMirror's own
 * position model is on the JS side; we never index the source as chars).
 * 
 * `u32` instead of `usize` so specta maps the fields to TS `number`
 * rather than `string` (JS Number can represent the full u32 range
 * precisely; a 64-bit `usize` cannot fit losslessly). 4 GiB markdown
 * is not a real moss editor scenario.
 */
export type EditorRange = { from: number; to: number }

/**
 * Always-filled envelope for one resolved reference: never a bare Option, so
 * the editor can show lint diagnostics for unresolved/ambiguous references too.
 * This is the SINGLE envelope every editor consumer reads (image render,
 * link/asset lint, hover, cmd-click nav).
 */
export type EditorReferenceResolution = { 
/**
 * The original reference text that was classified.
 */
target: string; 
/**
 * Whether the reference was resolved in embed context.
 */
is_embed: boolean; 
/**
 * The classified kind (Image, Iframe, Folder kinds, Link, External, …).
 */
kind: ReferenceKind; 
/**
 * For file/asset embed kinds: the enriched asset (path, URL, mime, LQIP).
 * None for non-asset kinds and for a TOCTOU miss.
 */
resolved: ResolvedAsset | null; 
/**
 * For Link kinds: the resolved deployed URL (carried from the classifier).
 */
url: string | null; 
/**
 * For Link kinds with a `#fragment`: the anchor.
 */
anchor: string | null; 
/**
 * The absolute source FILE this reference opens when followed, for the two
 * kinds that have one: a resolved internal Link (via `source_path_for_url`)
 * and a FolderListing embed (via `FolderFacts::index_source_for`, which is
 * the folder's index source, NOT the folder directory).
 * 
 * Always a file, never a directory — every consumer opens it. A folder that
 * publishes no index source of its own leaves this `None`, so the editor
 * shows no follow affordance rather than one that lands nowhere.
 */
resolved_path: string | null; 
/**
 * Human-readable diagnostic note (separator-fallback / case-mismatch / …).
 */
message: string | null; 
/**
 * For Ambiguous: all candidate paths.
 */
candidates: string[]; 
/**
 * For FolderListing embeds: what the last build listed. None for every
 * other kind, and for a folder the article map does not know.
 */
folder: FolderEmbedInfo | null }

/**
 * Result of editor-side shortcode scanning.
 */
export type EditorScanResult = { blocks: EditorShortcodeBlock[]; 
/**
 * True if any divider line in any grid block used the deprecated `---`
 * form — including inside a block that never closed, whose dividers are
 * dropped. Per-divider detail lives on [`EditorDivider::legacy`]; this
 * stays as the cheap document-level answer.
 */
legacy_dash: boolean }

/**
 * One shortcode block as seen by the editor.
 */
export type EditorShortcodeBlock = { 
/**
 * Opening fence line (e.g. `:::grid 2`).
 */
open: EditorRange; 
/**
 * Closing fence line (e.g. `:::`).
 */
close: EditorRange; 
/**
 * Shortcode name (e.g. "grid", "buttons").
 */
name: string; 
/**
 * Trailing args after the name (e.g. "2", "{.primary}").
 */
args: string; 
/**
 * Top-level cell divider lines (only the dividers at this block's depth;
 * nested-block dividers belong to their own block entry).
 */
dividers: EditorDivider[] }

/**
 * The render family a non-folder, non-link target belongs to, by extension.
 */
export type ExtKind = "Image" | "Iframe" | "Pdf" | "Video" | "Audio" | "Model" | "Transclusion" | "Notebook" | "Table" | "Other"

/**
 * A single field definition in the frontmatter schema.
 */
export type FieldDefinition = { 
/**
 * The data type of the field.
 */
type: FieldType; 
/**
 * Which UI widget to render for this field.
 * `None` for sub-definitions (e.g. array item types) that have
 * no direct UI representation.
 */
widget?: Widget | null; 
/**
 * Whether this field is required.
 */
required?: boolean; 
/**
 * Default value for the field.
 */
default?: JsonValue | null; 
/**
 * Format hint (e.g. "date" for YYYY-MM-DD).
 */
format?: string | null; 
/**
 * Allowed values for select/enum fields.
 */
enum_values?: string[] | null; 
/**
 * Item definition for array fields.
 */
items?: JsonValue | null; 
/**
 * Member variants for a `OneOf` union field. Each member is a full
 * `FieldDefinition` carrying its own `field_type` + `widget`. Only set
 * when `field_type == OneOf`. The specta override mirrors `items` to dodge
 * the self-referential type (the chip bar reads members structurally from
 * JSON; it needs no nominal TS type).
 */
one_of?: JsonValue[] | null; 
/**
 * Human-readable description of the field.
 */
description?: string | null; 
/**
 * Optional human-readable label for the chip bar. When `None`, the frontend
 * falls back to using the field key.
 */
label?: string | null; 
/**
 * i18n key for the chip bar label, resolved by the TypeScript registry.
 * `None` when no key is registered (frontend falls back to field name).
 */
label_key?: string | null; 
/**
 * Display score for chip bar ordering and add-property search list ordering.
 * Lower values appear first / sort higher in the list.
 * Formula: score = 100 - (Frequency*6 + Importance*4); see schema_fields.rs.
 * 0 means unset (skip-schema or plugin-contributed fields, sort to end).
 * Serialized as "priority" for backwards compatibility with the frontend.
 */
priority?: number; 
/**
 * Source of this field definition.
 * `None` for builtin fields, `Some("review")` for plugin-contributed fields.
 * Used by the frontend to group fields by source in the editor form.
 */
source?: string | null; 
/**
 * UI group name for the add-property dropdown (e.g. "Common", "Children").
 * `None` for plugin-contributed fields (shown in "Other" group).
 */
group?: string | null; 
/**
 * For `Widget::FilePicker` fields, the extension kinds the picker should
 * restrict search results to (e.g. `cover` -> image/video, `logo` ->
 * image only). `None` means unrestricted. Mirrors
 * `schema_fields::BuiltinField::file_kinds` — see that field's doc comment.
 */
file_kinds?: ExtKind[] | null; 
/**
 * Where this field can change a page: it applies when any condition holds.
 * `None` means everywhere, an empty list nowhere. An editor reads it to
 * leave out of its add list a field that would do nothing on the open page.
 */
applies_when?: AppliesWhen[] | null }

/**
 * Supported field types.
 */
export type FieldType = "string" | "boolean" | "integer" | "number" | "array" | "object" | 
/**
 * A union of member variants (see `FieldDefinition::one_of`). The authored
 * value matches exactly one member. Used for fields like `children`
 * (bool | wikilink) and `series` (bool | wikilink-list) whose real value
 * space the older scalar types could not express.
 */
"oneof"

export type FileArgs = { filePath: VaultPath }

/**
 * One reference found in a file that points to a given target.
 */
export type FileReferenceHit = { 
/**
 * Absolute path of the file that contains the reference.
 */
referencing_file: string; 
/**
 * The raw reference text (the inner part of the wikilink / markdown link).
 */
ref_text: string; 
/**
 * 1-based line number of the reference in the referencing file.
 */
line: number }

export type FolderArgs = { 
/**
 * The folder the caller asks about; refused if it is not the open one.
 */
folderPath: string }

/**
 * What the LAST BUILD listed under a folder, read from the `ArticleMap`.
 * Attached to `FolderListing` embeds so the editor's card can state what will
 * be listed and how, without reimplementing the build's selection.
 * 
 * The fidelity limits of the underlying derivation are documented on
 * [`crate::editor::resolve::url_index::FolderFacts`]; the editor renders these
 * counts as a bare number and makes no prose claim about build output.
 */
export type FolderEmbedInfo = { 
/**
 * Title of the folder's index page. None when there is no index source or
 * it cannot be read.
 */
title: string | null; 
/**
 * Pages listed at `depth:direct` (articles + sub-folder index pages).
 */
direct_child_count: number; 
/**
 * Pages listed at `depth:all` (articles only, at any depth).
 */
descendant_count: number }

/**
 * Frontmatter schema: a map of field name to definition.
 */
export type FrontmatterSchema = { 
/**
 * Field definitions keyed by field name.
 */
fields: { [key in string]: FieldDefinition }; 
/**
 * Field names that exist in the build pipeline's `FrontMatter` struct
 * but are not surfaced in the editor form (the `skip_schema: true`
 * entries from `BUILTIN_FIELDS`). The editor uses this list to filter
 * unknown values it shouldn't render as chips — auto-generated fields,
 * build-only fields, and site-level config.
 * 
 * Empty for plugin-contributed schemas loaded from JSON (they have no
 * notion of internal fields). `#[serde(default)]` keeps backward
 * compatibility with externally-loaded schema JSON that predates this
 * field.
 */
internal_fields?: string[] }

/**
 * Where the resolved heading text came from. New variants are added when a
 * new origin is introduced; consumers handle them exhaustively.
 */
export type HeadingSource = 
/**
 * Text derived from filename (or parent folder for index notes).
 */
"Filename" | 
/**
 * Text from frontmatter `title:`. `Some("")` and `Some("   ")` count as
 * Title source with empty text — `visible` will be false in that case.
 */
"Title"

/**
 * Resolved heading state for a single page.
 * 
 * `visible` is the gate the build pipeline checks before injecting an
 * `<h1 class="moss-article-title">` and the editor checks before rendering
 * its pinned heading element. `source` tells the editor where `text` came
 * from so it can route heading-element commits between rename (Filename)
 * and title-update (Title).
 */
export type HeadingState = { 
/**
 * Whether the article heading should render.
 */
visible: boolean; 
/**
 * The visible heading text (resolved through title or filename).
 */
text: string; 
/**
 * Where `text` was sourced from.
 */
source: HeadingSource }

export type HeadingStateArgs = { filePath: VaultPath; frontmatter: JsonValue; body: string }

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key in string]: JsonValue }

/**
 * One kind's fields and the display names the last build derived for it.
 */
export type KindTerms = { fields: string[]; 
/**
 * Each name once, whichever editions use it.
 */
names: string[] }

/**
 * A caller's `projectPath` is not read: a caller that names both the root
 * and the path checked against it can pass any containment test.
 */
export type ListDirectoryArgs = { path: VaultPath; showInternal: boolean }

/**
 * Walks the open folder; a caller's `path` is not read, for the reason
 * [`ListDirectoryArgs`] gives. A loose document's folder is listed only to a
 * budget, since it can be any folder on the machine.
 */
export type ListTreeArgs = { showInternal: boolean }

export type ListVersionsArgs = { scope: string; path: string | null }

/**
 * Physical-source evidence for a missing authored asset.
 */
export type MissingReferenceOccurrence = { 
/**
 * Markdown file holding the reference, relative to the site root.
 */
source_path: string; 
/**
 * SHA-256 of the exact source bytes this occurrence was extracted from.
 */
source_revision: SourceRevision; 
/**
 * The reference exactly as the author typed it.
 */
reference: string; 
/**
 * Exact destination bytes in the physical source, never a post-resolution
 * or transclusion coordinate.
 */
source_span: SourceSpan }

export type MoveEntriesArgs = { paths: VaultPath[]; targetDir: VaultPath }

/**
 * One note to create: `dir` is absolute, `name` carries the extension,
 * `frontmatter` is the JSON object the editor's save path already speaks.
 * The body is always empty: the filename is the title (monotonic mode).
 */
export type NewFile = { dir: string; name: string; frontmatter: { [key in string]: JsonValue } }

/**
 * A command that takes no arguments. Extra keys a caller sends are ignored.
 */
export type NoArgs = Record<string, never>

/**
 * What the last build knows about one source file, for an editor's add list:
 * where the page sits, which decides the fields that apply, and the value the
 * build gave each field the file leaves out.
 */
export type PageFacts = { 
/**
 * `None` for a slot, which is not a page.
 */
position: PagePosition | null; auto: { [key in string]: JsonValue } }

/**
 * Where a page sits in its site: an ordinary page, a folder's index, or a
 * home (the site root's index, or the root index of a language tree such as
 * `en/`).
 */
export type PagePosition = "page" | "folder" | "home"

/**
 * Result of resolving a preview URL to its source file and page type.
 */
export type PageSource = { 
/**
 * Absolute path to the source markdown file, if found.
 */
source_path: string | null; 
/**
 * Whether this page is a directory listing (e.g., homepage, section index).
 */
is_dir: boolean; 
/**
 * Whether this URL corresponds to an article in the article map.
 */
is_article: boolean; 
/**
 * Syndication URLs from the article's frontmatter (empty if not an article).
 */
syndicated?: string[]; 
/**
 * What an author would create to take over this page when moss generates
 * it — a home note for an index-less folder or the root, a claim file
 * for an unclaimed term, a translation for a language home. `None` for
 * pages with a source and for generated output nothing can replace
 * (feeds, redirect stubs). Computed by [`super::takeover::takeover_for`].
 */
takeover?: Takeover | null; 
/**
 * The editable source for this URL exists but could not be *identified*
 * yet, because a file the resolve had to read is still coming down from
 * the cloud.
 * 
 * Distinct from `source_path: None`, which is a positive claim that this
 * URL has nothing to edit. The two render different surfaces — a waiting
 * pane that heals itself on arrival, versus "this page is generated by
 * moss" — and conflating them is what told a user their home page did not
 * exist while the provider was still handing it over.
 */
source_pending?: boolean }

export type PageSourceArgs = { urlPath: string }

/**
 * What happened to one page since the last publish.
 * 
 * Deliberately not `Unchanged` — an unchanged page is absent from the change
 * set entirely, so a caller cannot accidentally count it.
 * 
 * Named `PageVerb`, not `Verb`, because `tasks::Verb` already exists and both
 * are `specta::Type`: specta exports by bare type name into one flat
 * namespace, so two `Verb`s emit two `export type Verb` lines and
 * `bindings.ts` stops compiling. The collision is invisible in Rust — it only
 * appears when the bindings are regenerated.
 */
export type PageVerb = 
/**
 * The author created this page since the last publish.
 */
"added" | 
/**
 * The author changed this page's source bytes.
 */
"edited" | 
/**
 * The author removed this page.
 */
"deleted" | 
/**
 * The author changed nothing; moss re-rendered the page anyway (theme,
 * config, template, or a shared component moved).
 */
"restyled"

/**
 * Frontmatter parsing result exposed to the frontend.
 */
export type ParsedFrontmatter = { 
/**
 * [`content_hash`] of the file's bytes as read, before any newline
 * normalisation. Empty while [`still_arriving`](Self::still_arriving).
 */
hash: string; 
/**
 * Parsed frontmatter as a JSON value (object).
 */
frontmatter: JsonValue; 
/**
 * The markdown body (everything after the closing `---`).
 */
body: string; 
/**
 * Whether the document had frontmatter delimiters.
 */
has_frontmatter: boolean; 
/**
 * serde_yaml error message when a delimited `---...---` block failed to
 * parse. `None` when it parsed cleanly or there was no block. The frontend
 * can render an "invalid YAML" banner off this so a malformed block flags an
 * error instead of silently showing a blank frontmatter form. `body` still
 * holds the whole document so the author can repair the raw block in CM6.
 */
frontmatter_error?: string | null; 
/**
 * The file is real and its bytes are not here yet — a cloud placeholder
 * the provider has not handed back. Every other field is
 * empty, and the editor must mount its "still arriving" pane rather than
 * this document: an empty CM6 buffer over a file with content is the same
 * lie as the errno toast this replaced.
 * 
 * Set only by [`classify_source_read`], which is the one place in the
 * editor that reads the cloud's answer. Never true together with content.
 */
still_arriving?: boolean }

export type PathArgs = { path: VaultPath }

export type PathsArgs = { paths: VaultPath[] }

/**
 * One entry being renamed or moved, project-relative to both sides.
 */
export type PlannedMove = { old_path: string; new_path: string; is_dir: boolean }

/**
 * The completed build's publish-time evidence, replaced atomically per
 * folder. An absent projection means no build in this process has answered;
 * a projection with no references is the completed clean answer.
 */
export type PublishPreflightProjection = { 
/**
 * Ordered at build admission, so a later build cannot be overwritten by
 * an older build that happens to complete after it.
 */
build_generation: number; missing_references: MissingReferenceOccurrence[]; 
/**
 * Required inputs this attempt could not read. A clean later attempt
 * replaces this list, even while an older complete seal remains selected.
 */
unresolved_inputs?: string[]; 
/**
 * Links from published pages to drafts, which would be dead on the live site.
 */
drafts_linked?: DraftLinkOccurrence[] }

export type ReadVersionArgs = { id: string; path: string }

/**
 * One reference to resolve: the raw target text plus whether it appeared in an
 * embed context (`![[...]]` / `![](...)`) or a plain link (`[[...]]` / `[](...)`).
 * `is_embed` is what drives the classifier's Link-vs-embed fork.
 */
export type RefTarget = { text: string; is_embed: boolean }

export type ReferenceKind = { kind: "link"; data: { anchor: string | null } } | { kind: "image" } | { kind: "iframe" } | { kind: "pdf" } | { kind: "video" } | { kind: "audio" } | { kind: "model" } | { kind: "folder-listing" } | { kind: "folder-index-iframe" } | { kind: "transclusion" } | { kind: "notebook" } | { kind: "table" } | { kind: "external"; data: { url: string } } | { kind: "anchor" } | { kind: "ambiguous" } | { kind: "not-found" } | 
/**
 * An internal link whose URL no longer exists: the page it named now
 * lives at `canonical` (a claimed term's generated URL). `url` on the
 * resolved reference carries `canonical` so following it lands there.
 */
{ kind: "moved"; data: { canonical: string } }

export type RenameArgs = { oldPath: VaultPath; newPath: VaultPath }

/**
 * What a rename or move did, project-wide.
 */
export type RenameRefsSummary = { 
/**
 * Every top-level entry the caller asked to move, at its final (possibly
 * collision-suffixed) destination. A folder's carried home file is not a
 * separate entry.
 */
moved: PlannedMove[]; edited_link_count: number; 
/**
 * Distinct edited pages, project-relative, in first-touched order.
 */
edited_pages: string[]; 
/**
 * Pages skipped because they changed on disk after planning.
 */
skipped_pages: string[]; 
/**
 * Empty when there is nothing to undo.
 */
undo_id: string }

export type ResolveAssetArgs = { target: string; fromFile: VaultPath }

export type ResolveReferencesArgs = { targets: RefTarget[]; fromFile: VaultPath }

export type ResolvedAsset = { absolute_path: string; request_url: string; mime_type: string; width: number | null; height: number | null; size_bytes: number; 
/**
 * How the path was resolved: literal, bare-fuzzy, separator-fallback, or
 * case-mismatch. Carried from the shared engine.
 * 
 * Read by the editor off the unified reference envelope
 * (`EditorReferenceResolution`), which marks a separator-fallback or
 * case-mismatch path as a problem and offers the file's real path.
 * Also exposed by the singular `editor_resolve_asset` (chip-bar) command.
 */
provenance: AssetProvenance }

/**
 * One path [`restore_version`] could not carry through — a field-named struct
 * rather than a bare `(String, String)` tuple, so the bindings read as
 * `{path, error}` instead of a positional pair.
 */
export type RestoreFailure = { path: string; error: string }

/**
 * [`restore_version`]'s answer, unified across the page and site forms (design
 * rule 4, "best-effort with an honest count"): a page restore reports itself
 * as a one-entry `restored`/`failed`, matching the shape a site restore's
 * [`HistoryStore::restore_site`] already returns. `undo_id` is the pre-restore
 * backup record's id, when one was actually written — `save_before_restore`
 * dedups against the newest record for the target, so a repeated restore
 * with nothing new to undo writes no record and `undo_id` is `None`; the
 * panel then shows no Undo action rather than one that restores nothing.
 * 
 * Named for the boundary, not for [`super::RestoreReport`], which is the
 * store's own tuple-shaped report this is translated from.
 */
export type RestoreReport = { restored: string[]; failed: RestoreFailure[]; trashed: string[]; undo_id: string | null }

export type RestoreVersionArgs = { id: string; path: string | null; mode: string }

export type SaveEditorContentArgs = { filePath: VaultPath; frontmatter: JsonValue; body: string; expectedHash: string; keepDisplaced?: boolean }

/**
 * What an editor save did. A failure to write is an `Err`; this is the
 * outcome of a save that did not fail.
 */
export type SaveResult = 
/**
 * Written. `hash` is the [`content_hash`] of the bytes now on disk, the
 * base the editor's next save must name. `displaced` is the history
 * version that keeps the bytes this save replaced, when the save was
 * asked to keep them.
 */
{ status: "saved"; hash: string; displaced?: string | null } | 
/**
 * Nothing written: the file is no longer at this path, renamed or deleted
 * outside moss since it was opened. Not a failure — the file event that
 * follows says which, and the text stays unsaved in the editor until then.
 */
{ status: "gone" } | 
/**
 * Nothing written: another writer changed the file since the editor last
 * read it. `disk` is what is there now, so the editor can merge it into
 * the unsaved text and save again with `disk.hash` as the base.
 */
{ status: "conflict"; disk: ParsedFrontmatter } | 
/**
 * Nothing written: the save was to replace another writer's text and
 * moss could not keep that text first, because history is turned off for
 * the site or the file is in no site.
 */
{ status: "not-kept" }

export type SaveVersionArgs = { label: string | null }

export type ScanShortcodesArgs = { text: string }

/**
 * A single shortcode definition.
 */
export type ShortcodeDefinition = { 
/**
 * Display name.
 */
name: string; 
/**
 * Whether the shortcode wraps content.
 */
has_content?: boolean; 
/**
 * Parameter definitions.
 */
params?: ShortcodeParam[] }

/**
 * A shortcode parameter.
 */
export type ShortcodeParam = { 
/**
 * Parameter name.
 */
name: string; 
/**
 * Whether the parameter is required.
 */
required?: boolean; 
/**
 * Default value.
 */
default?: string | null }

/**
 * Shortcode schema: delimiters and named definitions.
 */
export type ShortcodeSchema = { 
/**
 * (open_delimiter, close_delimiter) pair.
 */
delimiters: [string, string]; 
/**
 * Named shortcode definitions.
 */
definitions: { [key in string]: ShortcodeDefinition } }

/**
 * A file [`apply_planned_moves`] or [`undo_applied`] left untouched because
 * its recorded text was no longer at the recorded position.
 */
export type SkippedFile = { file: string; reason: string }

/**
 * SHA-256 of the final UTF-8 source bytes that produced an occurrence.
 * 
 * The transparent lowercase hexadecimal string is the stable serde and
 * Specta wire representation. Keeping it as text, rather than serializing
 * SHA-256's 32 bytes as an array, lets the desktop compare the digest it
 * calculates from the open file without a binary conversion boundary.
 */
export type SourceRevision = string

/**
 * What a source file becomes on the built site. "Which slot, if any" rather
 * than a boolean, so a future slot file needs no new command.
 */
export type SourceRole = { 
/**
 * Layout slot this file fills INSTEAD of becoming a page (`"footer-left"`).
 */
slot: string | null; 
/**
 * Language tree the file lives in (`"zh-hans"`); `None` for the default
 * tree. Reported for EVERY file, not just slot files: the shell compares
 * the tree of the slot file being edited against the tree of the page the
 * preview is showing, and needs the same answer for both sides of that
 * comparison. One command, one definition of "which language tree" —
 * asking a second command for the page side would be a place to drift.
 */
language: string | null; 
/**
 * Whether the build turns this file's extension into a page at all.
 * 
 * Reported so the preview follower can tell "not built YET" (keep
 * retrying on the next build) from "never a page" (stop, and say so).
 * It answers the extension question only — `slot` above still overrides
 * it, and `.moss/` inputs are a path fact the frontend already owns.
 * 
 * This rides on `describe_source_role` rather than becoming a set in
 * TypeScript for the same reason `slot` does: the authority is
 * `build::scan::classify::is_page_source`, and a second copy of it is the
 * Rust↔TS parity trap.
 */
can_be_page: boolean }

/**
 * A byte span in a physical markdown source.
 */
export type SourceSpan = { start_byte: number; end_byte: number; 
/**
 * One-based line containing this byte range in the same final source.
 */
line: number }

/**
 * A file's role on the site, beyond being a page or an asset. The editor
 * explains it before opening the file, so the tree says it rather than the
 * editor guessing from names.
 */
export type SpecialFileKind = 
/**
 * A root `AGENTS.md`/`CLAUDE.md`/`GEMINI.md`: read by coding agents, never published.
 */
"agent-instructions" | 
/**
 * The welcome email at a language-tree root: mail, not a page.
 */
"welcome-email" | 
/**
 * The reserved-name footer at a language-tree root, by name only: a
 * `slot:` declaration in frontmatter does not make a file this kind.
 */
"footer" | 
/**
 * The site's `404.md`, shown by a host for an address that does not exist.
 */
"not-found" | 
/**
 * The `index.md` of a folder whose `index.html` is copied as-is: it holds
 * that document's metadata and is not what visitors read.
 */
"page-settings"

/**
 * The whole recipe. `files` are written in order and the LAST one is the
 * page the editor opens afterwards — the claim file for a term, the home
 * note otherwise. `display` is the name the pane shows: the term, the
 * folder, the root, the language.
 */
export type Takeover = { kind: TakeoverKind; display: string; 
/**
 * For a [`TakeoverKind::TermPage`], the name-list field the claim is
 * written through — `"editor"`, not `"editor_page"`. `None` for every
 * other kind. The pane shows it, so a person offered a claim can see
 * they are about to be recorded as an editor rather than an author.
 */
field: string | null; files: NewFile[]; 
/**
 * Proven directory emptiness; unknown when enumeration fails.
 */
folder_empty?: boolean | null }

/**
 * Which generated page the recipe replaces. The editor pane keys its copy on
 * this and nothing else.
 */
export type TakeoverKind = 
/**
 * The vault root has articles but no home note.
 */
"root-home" | 
/**
 * An index-less folder, including a term namespace root (`/authors/`).
 */
"folder-home" | 
/**
 * An unclaimed term's generated page, whatever namespace it is in.
 * Which frontmatter field the claim is written through is
 * [`Takeover::field`], not a variant of its own: a site can declare any
 * number of kinds, and an enum that grew a variant per field would have
 * to be regenerated every time one was added.
 */
"term-page" | 
/**
 * A language tree's synthesized home.
 */
"lang-home"

/**
 * A recursive tree node for the full project file tree.
 */
export type TreeNode = { name: string; path: string; is_dir: boolean; 
/**
 * Last modified time as seconds since UNIX epoch. None if unavailable.
 */
modified: number | null; 
/**
 * Resolved publish date as `YYYY-MM-DD`, or `None`.
 * 
 * For markdown files: from frontmatter `date` field, filename `YYYY-MM-DD-…`
 * prefix, or ctime fallback. For folders: max recency over descendants per
 * the spec (see `compute_folder_recency`). Always `None` for non-md files.
 */
publish_date: string | null; 
/**
 * Provenance of `publish_date`. Drives sort-zone classification:
 * `Frontmatter` and `FilenamePrefix` are explicit (dated-md zone);
 * `Ctime` is implicit (undated-md zone). For folders, always `None`.
 */
date_source: DateSource; 
/**
 * Whether this node is its parent folder's ELECTED home file — the single
 * winner of `moss_core::home::detect_home_file_in_folder_marked` (order:
 * `home: true` frontmatter marker › index stems › self-named folder note ›
 * first document alphabetically), computed per folder in
 * `list_tree_inner_cached` with the folder's REAL basename (so the project
 * root's self-named home is recognized too). At most one child per folder
 * is `true`; always `false` for directories. The frontend consumes this
 * flag (`entries.find(e => e.is_home)`) instead of re-deriving the
 * election — the backend owns the home decision (consolidation-map
 * homeRank row).
 */
is_home: boolean; 
/**
 * Children of this directory. `None` for files.
 */
children: TreeNode[] | null; hidden: number; 
/**
 * See `DirEntry::file_id`.
 */
file_id: string | null; 
/**
 * The file's role on the site when it has a special one, decided by
 * `classify::special_file_kind`. Always `None` for folders.
 */
special?: SpecialFileKind | null }

export type UndoRenameArgs = { undoId: string }

/**
 * The result of [`undo_applied`].
 */
export type UndoResult = { restored_files: string[]; skipped: SkippedFile[] }

/**
 * A path a caller names. Its only way to a real path is
 * [`CommandHost::resolve`], so no command can skip the host's confinement.
 */
export type VaultPath = string

/**
 * Every term in the vault, by kind, for chip completion. Display forms as
 * the build recorded them, in key order. `fields` is the kind's own field
 * list, so the caller can map the field a chip speaks (`editor`) to the
 * names it should complete against — every field of one kind shares one
 * name list.
 */
export type VaultTerms = { kinds: { [key in string]: KindTerms } }

/**
 * [`read_version`]'s answer. `kept: false` means the bytes at this version
 * were never stored (over the size ceiling, cloud-evicted at publish time,
 * or otherwise unreadable) — the panel shows "not kept" and disables
 * Restore, never a fabricated body. `kept: true, text: None` is a kept
 * binary entry (a media file): moss never ships raw bytes over a command
 * for a history read, only the fact that they exist. A symlink entry's
 * stored bytes are its target string, which decodes as ordinary text.
 */
export type VersionContent = { kept: boolean; text: string | null }

/**
 * [`list_versions`]'s answer. `rows` is the day-grouped list the panel renders
 * for either scope ("This page" → `page_timeline`, "Whole site" with no
 * `path` → `site_timeline`); `changed_since`/`unchanged` are populated ONLY
 * for a site-scope drill-down (`scope: "site"`, `path: Some(<version id>)`)
 * — the "A site version" sub-view's Changed-since/Unchanged groups — and
 * `rows` is empty in that mode. One command rather than two: the drill-down
 * reuses [`HistoryStore::site_version_pages`], which needs the live sealed
 * manifest exactly like the top-level list's `live_id` does, so the two
 * share one body instead of duplicating the manifest plumbing.
 */
export type VersionList = { rows: VersionRow[]; 
/**
 * The row id whose `generation_id` matches the deploy baseline live for
 * this project's current target — never the newest row (design rule 7:
 * "Live" is the baseline's own record, and history's write can lag it).
 */
live_id: string | null; has_multiple_targets: boolean; 
/**
 * "Show in Finder" footer target.
 */
store_dir: string; changed_since: ChangedPage[]; unchanged: string[] }

/**
 * One row in the Versions list — a publish, a manual save, a pre-restore
 * save, or a kept outside edit of one page. `trigger` is
 * `"publish" | "manual" | "restore" | "displaced"`, the same flattened-
 * string convention [`restore_version`]'s `mode` argument uses, so the
 * frontend never imports a second enum shape for the same words.
 */
export type VersionRow = { id: string; published_at: string; target: string; trigger: string; label: string | null; changed: number; added: number; removed: number }

/**
 * UI widget types.
 */
export type Widget = "text-input" | "text-area" | "date-picker" | "number-input" | "checkbox" | "select" | "tag-input" | "file-picker" | "code-editor" | 
/**
 * Parent dispatcher for a `OneOf` field: reads `one_of` and renders the
 * active branch's UI. Never falls through to value-type inference.
 */
"union" | 
/**
 * Single wikilink/path picker with folder autocomplete (e.g. `children`).
 */
"wikilink-picker" | 
/**
 * Ordered list of wikilinks (e.g. `series` explicit order).
 */
"wikilink-list-picker"

export type EngineCommands = {
	scan_shortcodes: { args: ScanShortcodesArgs; out: EditorScanResult };
	parse_frontmatter: { args: FileArgs; out: ParsedFrontmatter };
	editor_resolve_asset: { args: ResolveAssetArgs; out: ResolvedAsset | null };
	editor_resolve_references: { args: ResolveReferencesArgs; out: EditorReferenceResolution[] };
	editor_bootstrap: { args: BootstrapArgs; out: EditorBootstrap };
	list_directory: { args: ListDirectoryArgs; out: DirEntry[] };
	list_tree: { args: ListTreeArgs; out: TreeNode };
	get_file_info: { args: FileArgs; out: EditorFileInfo };
	compute_heading_state: { args: HeadingStateArgs; out: HeadingState };
	describe_source_role: { args: FileArgs; out: SourceRole };
	page_facts: { args: FileArgs; out: PageFacts };
	resolve_url_for_file: { args: FileArgs; out: string | null };
	validate_content: { args: FileArgs; out: EditorDiagnostic[] };
	get_publish_preflight: { args: FolderArgs; out: PublishPreflightProjection | null };
	list_vault_terms: { args: NoArgs; out: VaultTerms };
	get_cloud_unavailable_files: { args: FolderArgs; out: string[] };
	list_versions: { args: ListVersionsArgs; out: VersionList };
	read_version: { args: ReadVersionArgs; out: VersionContent };
	reveal_history_store: { args: NoArgs; out: null };
	resolve_attachment_dir: { args: AttachmentDirArgs; out: string };
	resolve_page_source: { args: PageSourceArgs; out: PageSource };
	create_files: { args: CreateFilesArgs; out: string[] };
	create_folder: { args: CreateFolderArgs; out: string };
	delete_entry: { args: PathArgs; out: null };
	delete_entries: { args: PathsArgs; out: null };
	rename_entry_with_refs: { args: RenameArgs; out: RenameRefsSummary };
	move_entries: { args: MoveEntriesArgs; out: RenameRefsSummary };
	scan_references_for_delete: { args: PathsArgs; out: FileReferenceHit[] };
	clean_references_and_delete: { args: PathsArgs; out: null };
	undo_rename: { args: UndoRenameArgs; out: UndoResult };
	save_editor_content: { args: SaveEditorContentArgs; out: SaveResult };
	restore_version: { args: RestoreVersionArgs; out: RestoreReport };
	save_version: { args: SaveVersionArgs; out: VersionRow };
};

export type EngineCommand = keyof EngineCommands;

export const ENGINE_TIER = {
	scan_shortcodes: "public_read",
	parse_frontmatter: "public_read",
	editor_resolve_asset: "public_read",
	editor_resolve_references: "public_read",
	editor_bootstrap: "session_read",
	list_directory: "session_read",
	list_tree: "session_read",
	get_file_info: "session_read",
	compute_heading_state: "session_read",
	describe_source_role: "session_read",
	page_facts: "session_read",
	resolve_url_for_file: "session_read",
	validate_content: "session_read",
	get_publish_preflight: "session_read",
	list_vault_terms: "session_read",
	get_cloud_unavailable_files: "session_read",
	list_versions: "session_read",
	read_version: "session_read",
	reveal_history_store: "session_read",
	resolve_attachment_dir: "session_read",
	resolve_page_source: "session_read",
	create_files: "write",
	create_folder: "write",
	delete_entry: "write",
	delete_entries: "write",
	rename_entry_with_refs: "write",
	move_entries: "write",
	scan_references_for_delete: "write",
	clean_references_and_delete: "write",
	undo_rename: "write",
	save_editor_content: "write",
	restore_version: "write",
	save_version: "write",
} as const;

// Each command's arguments in declaration order, for a client that takes them positionally:
// an Args struct's field order is its client's argument order, so reordering fields is a breaking change.
export type EngineParams = {
	scan_shortcodes: [text: string];
	parse_frontmatter: [filePath: VaultPath];
	editor_resolve_asset: [target: string, fromFile: VaultPath];
	editor_resolve_references: [targets: RefTarget[], fromFile: VaultPath];
	editor_bootstrap: [sourcePath: VaultPath | null];
	list_directory: [path: VaultPath, showInternal: boolean];
	list_tree: [showInternal: boolean];
	get_file_info: [filePath: VaultPath];
	compute_heading_state: [filePath: VaultPath, frontmatter: JsonValue, body: string];
	describe_source_role: [filePath: VaultPath];
	page_facts: [filePath: VaultPath];
	resolve_url_for_file: [filePath: VaultPath];
	validate_content: [filePath: VaultPath];
	get_publish_preflight: [folderPath: string];
	list_vault_terms: [];
	get_cloud_unavailable_files: [folderPath: string];
	list_versions: [scope: string, path: string | null];
	read_version: [id: string, path: string];
	reveal_history_store: [];
	resolve_attachment_dir: [pageRelativePath: VaultPath];
	resolve_page_source: [urlPath: string];
	create_files: [files: NewFile[]];
	create_folder: [parentDir: string, name: string];
	delete_entry: [path: VaultPath];
	delete_entries: [paths: VaultPath[]];
	rename_entry_with_refs: [oldPath: VaultPath, newPath: VaultPath];
	move_entries: [paths: VaultPath[], targetDir: VaultPath];
	scan_references_for_delete: [paths: VaultPath[]];
	clean_references_and_delete: [paths: VaultPath[]];
	undo_rename: [undoId: string];
	save_editor_content: [filePath: VaultPath, frontmatter: JsonValue, body: string, expectedHash: string, keepDisplaced?: boolean];
	restore_version: [id: string, path: string | null, mode: string];
	save_version: [label: string | null];
};

export const ENGINE_PARAMS = {
	scan_shortcodes: ["text"],
	parse_frontmatter: ["filePath"],
	editor_resolve_asset: ["target", "fromFile"],
	editor_resolve_references: ["targets", "fromFile"],
	editor_bootstrap: ["sourcePath"],
	list_directory: ["path", "showInternal"],
	list_tree: ["showInternal"],
	get_file_info: ["filePath"],
	compute_heading_state: ["filePath", "frontmatter", "body"],
	describe_source_role: ["filePath"],
	page_facts: ["filePath"],
	resolve_url_for_file: ["filePath"],
	validate_content: ["filePath"],
	get_publish_preflight: ["folderPath"],
	list_vault_terms: [],
	get_cloud_unavailable_files: ["folderPath"],
	list_versions: ["scope", "path"],
	read_version: ["id", "path"],
	reveal_history_store: [],
	resolve_attachment_dir: ["pageRelativePath"],
	resolve_page_source: ["urlPath"],
	create_files: ["files"],
	create_folder: ["parentDir", "name"],
	delete_entry: ["path"],
	delete_entries: ["paths"],
	rename_entry_with_refs: ["oldPath", "newPath"],
	move_entries: ["paths", "targetDir"],
	scan_references_for_delete: ["paths"],
	clean_references_and_delete: ["paths"],
	undo_rename: ["undoId"],
	save_editor_content: ["filePath", "frontmatter", "body", "expectedHash", "keepDisplaced"],
	restore_version: ["id", "path", "mode"],
	save_version: ["label"],
} as const;
