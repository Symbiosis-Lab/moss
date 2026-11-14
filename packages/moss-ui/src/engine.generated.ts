// @generated from moss-build's command table. Do not edit.
// Regenerate: UPDATE_ENGINE_CONTRACT=1 cargo test -p moss-build --lib commands::tests::the_contract

/**
 * The recovery affordance for an advisory, expressed as data (no rendering
 * logic on the model — the frontend maps each variant to a control). Per
 * §12b R1 there is intentionally **no** `display` field.
 */
export type Action = 
/**
 * Nothing to do — informational advisory.
 */
"None" | 
/**
 * A shell command the user can copy/run (e.g. `brew install ffmpeg`).
 */
{ Command: { 
/**
 * The command text.
 */
run: string; 
/**
 * Label for the affordance (e.g. "Copy").
 */
label: string } } | 
/**
 * An in-app operation the frontend can dispatch.
 */
{ InApp: { 
/**
 * Which operation to run.
 */
op: AppOp; 
/**
 * Operation arguments as free-form JSON (shape is op-specific).
 */
args: JsonValue; 
/**
 * Label for the affordance.
 */
label: string } } | 
/**
 * Nothing to repair — moss already acted, irreversibly, and the author's
 * part is to look at the result and then say she has. The affordance
 * dismisses the notice.
 * 
 * It is also what keeps the notice on screen. Every other advisory
 * describes a *condition of this build*, so a rebuild re-derives it and
 * the panel clears the lot at build-start; this one describes an *act*,
 * which a rebuild does not answer and cannot repeat. So an `Acknowledge`
 * advisory is sticky: it survives the build-start clear and leaves only
 * by being acknowledged. Without that, a notice asking her to check
 * something disappears the moment she saves a file — possibly before she
 * has read it, and with no way to bring it back (2026-08-30).
 */
{ Acknowledge: { 
/**
 * Label for the affordance.
 */
label: string } } | 
/**
 * An external link to open. `href` is a plain `String` (not `url::Url`)
 * — specta + `Url` is unused elsewhere in the contract.
 */
{ Link: { 
/**
 * The URL to open.
 */
href: string; 
/**
 * Label for the affordance.
 */
label: string } }

/**
 * Payload for `MossEvent::ActionPanelOpened`. The constructors that derive
 * the chrome fields from `PanelOccupant` stay app-side
 * (`plugins/runtime/window.rs`) — the occupant vocabulary is shell chrome.
 */
export type ActionPanelOpened = { 
/**
 * Logical width of the action-panel webview, in CSS pixels.
 */
width: number; 
/**
 * Initial title text for the action-panel titlebar (e.g., plugin name).
 */
title: string; 
/**
 * Whether the shell shows the plugin titlebar strip + loading spinner.
 * From PanelOccupant::shows_plugin_titlebar() (NOT is_below_titlebar —
 * they diverge for the composer).
 */
shows_plugin_titlebar: boolean; 
/**
 * Whether the corner Edit (pencil) affordance hides while this panel is open. From PanelOccupant::hides_edit_affordance().
 */
hides_edit: boolean; 
/**
 * Geometry model: the panel sits below the titlebar, so the preview stays
 * full-window and the content insets from the left (--action-panel-width)
 * — distinct from shows_plugin_titlebar, which is chrome. browser=true,
 * editor/composer=false. Drives the InsetLeft vs Tiled preview model in
 * the frontend's ActionPanelOpened handler.
 */
is_below_titlebar: boolean; 
/**
 * The login the browser-panel shows, if any. A login renders its own
 * chrome (identity and actions) instead of the generic plugin-title strip.
 * Never set for non-browser occupants (editor, composer).
 */
login: LoginKind | null }

/**
 * What one address IS, which decides how moss offers it.
 * 
 * Closed for the kinds moss knows, open at the wire: a plugin from a newer
 * registry than this moss can name a kind we have never heard of, and that
 * must degrade to a plain labelled row rather than failing the whole deploy
 * result to deserialize.
 */
export type AddressKind = 
/**
 * A content identifier — immutable, names this exact publish.
 */
"cid" | 
/**
 * A mutable name that points at the latest publish.
 */
"ipns" | 
/**
 * An HTTP door onto content that is not natively HTTP.
 */
"gateway" | 
/**
 * A hostname the reader can type.
 */
"domain" | 
/**
 * A kind this moss does not know. Rendered as label + value, and
 * persisted as `"other"` — the original word is not kept, because moss
 * has nothing it could do with it.
 */
"other"

/**
 * A soft, non-blocking advisory from a background task — something the
 * pipeline encountered but did not fully process this build (e.g. a video
 * that failed to transcode and was published unoptimized). Replaces the
 * old free-form `warnings: Vec<String>`: structured (scope · severity ·
 * item · what · action-as-data) so the frontend can render `item` with
 * `what` on hover, branch the build-wide case (`item: None`, e.g. "FFmpeg
 * not available") into a distinct inline row, and offer a recovery
 * affordance driven by `action`.
 */
export type Advisory = { 
/**
 * Which axis of the system this advisory is about.
 */
scope: Scope; 
/**
 * How serious it is — drives the surface tier.
 */
severity: Severity; 
/**
 * The item this advisory is about: the SITE-ROOT-RELATIVE path of the
 * source file, exactly as it sits under the open folder on disk — never
 * a bare filename, and never a page-tree/URL path reshaped by a `url:`
 * override. `None` for a build-wide advisory that isn't tied to a single
 * file. The frontend's click-to-open resolves `item` by joining it onto
 * the open folder (`resolveAgainstFolder`), so a value that isn't the
 * real on-disk path opens nothing. Every moss-internal producer sets it
 * via [`Advisory::for_source`] (or [`Advisory::blocking_file`], which
 * delegates to it) — a producer that hand-writes `item: Some(...)`
 * without checking [`is_site_relative`] first has almost certainly
 * narrowed a site-relative path down to something that no longer
 * resolves. `clamp_plugin_advisory` is the one legitimate exception: it
 * starts from a plugin-supplied `Option<String>` rather than a bare
 * path, so it applies `is_site_relative` itself instead of going
 * through `for_source`.
 */
item: string | null; 
/**
 * What happened, shown on hover (per-file) or inline (build-wide).
 */
what: string; 
/**
 * The recovery affordance, expressed as data.
 */
action: Action; 
/**
 * How long the app keeps it.
 */
lifetime?: AdvisoryLifetime }

/**
 * How long the app keeps an advisory once raised.
 */
export type AdvisoryLifetime = 
/**
 * A condition of this build: the next build start clears it.
 */
"Build" | 
/**
 * Kept across build starts until its own task's next tick, for an
 * advisory that a rebuild not running its task cannot speak for (a
 * plugin's Sync offer: save-rebuilds run no process hook).
 */
"UntilTaskRestates"

/**
 * The quantity a Job reports on success: "142 pages", "3 posts". Present iff
 * the run reached `Done` (§4 invariant #4). The renderer formats `count` +
 * `noun`; never a pre-baked string (Hickey §3).
 */
export type Amount = { count: number; noun: string }

/**
 * Language preference for the moss app shell UI.
 */
export type AppLanguage = "en" | "de" | "zh_hans" | "zh_hant"

/**
 * A closed set of in-app operations an `Action::InApp` can request. Build
 * only ever emits `Action::None` / `Action::Command`, but the full set is
 * defined here so other producers (deploy, account) share one vocabulary.
 */
export type AppOp = 
/**
 * Move/relocate a file to resolve the advisory.
 */
"MoveFile" | 
/**
 * Open the billing/subscription surface.
 */
"OpenBilling" | 
/**
 * Prompt the user to sign in.
 */
"SignIn" | 
/**
 * Re-run a DNS check for a custom domain.
 */
"RecheckDns" | 
/**
 * Download a command-line tool moss needs, after the user asked for it; `args` is `{"tool": "ffmpeg"|"git"}`.
 */
"DownloadTool" | 
/**
 * Run a plugin's process hook as the person's own act, which may write the folder; `args` is `{"plugin": "<id>"}`, and the app names the plugin from its manifest.
 */
"RunPluginProcess"

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

/**
 * Asset ready notification for placeholder swapping
 * Tauri events for instant updates
 */
export type AssetReady = { 
/**
 * Relative path to the asset
 */
path: string; 
/**
 * Asset type: "image" | "video"
 */
asset_type: string }

/**
 * One authoritative "background asset variants have materialized" sweep,
 * emitted ONCE after a build seals (see `advertise_sealed`). Carries the set
 * of image/video output paths that are new-or-changed versus the previous
 * on-disk manifest. The frontend relays each to the iframe's page-agnostic
 * srcset cache-bust (the same swap `AssetReady` drives), turning an LQIP
 * placeholder into the real variant WITHOUT a full page reload.
 * 
 * Why this exists (2026-07-02): the watch refresh-diff runs on a pre-
 * background snapshot that can never contain the freshly-encoded `.webp`, so
 * dragging an image into the editor left the preview showing the placeholder
 * until a manual reload. This event fires from the one place that holds the
 * materialized manifest, after all background workers drain. Empty `changed`
 * → not emitted (preserves the no-op-when-nothing-changed contract).
 */
export type AssetsSettled = { 
/**
 * Image/video variants that are new-or-changed, each tagged with its
 * authoritative `asset_type` (see `SettledAsset`).
 */
changed: SettledAsset[] }

export type AttachmentDirArgs = { pageRelativePath: VaultPath }

/**
 * Background progress update for non-blocking tasks
 * Separate from blocking ProgressUpdate for clean separation
 */
export type BackgroundProgress = { 
/**
 * Task identifier from the emitting subsystem (e.g. "markdown",
 * "html_pages", "assets", "videos", "images", "notebooks", "rss"). The
 * frontend panel keys task rows by `bg:${task}` and renders `message`
 * as the label, so this set is open-ended — emitters add new ones as
 * needed.
 */
task: string; 
/**
 * Current item number
 */
current: number; 
/**
 * Total items to process
 */
total: number; 
/**
 * Formatted message (uses format_progress_message)
 */
message: string; 
/**
 * Whether this task is complete
 */
completed: boolean; 
/**
 * Soft-failure advisories for the *current* iteration of this task.
 * 
 * Producer contract: one human-readable string per item that the
 * pipeline encountered but did not fully process this build (e.g.
 * a notebook whose synchronous I/O timed out and was skipped). The
 * advisory string should name the offending item and the cause —
 * the frontend renders it verbatim in a non-progress task row.
 * 
 * Asymmetry with `completed`: the LAST iteration of a task batch
 * may carry `completed: true` AND a populated `advisories` vec when
 * the final item is the one that soft-failed. Consumers MUST
 * inspect both fields independently — `completed` answers "is this
 * task batch done?" and `advisories` answers "what should the user
 * know about items that didn't fully succeed?". Neither field
 * alone disambiguates clean-success from end-with-advisories.
 * 
 * Empty `vec![]` is the steady-state value; populated only when a
 * soft failure occurred in the iteration this event reports.
 */
advisories: Advisory[] }

export type BootstrapArgs = { 
/**
 * A bare boot opens no file.
 */
sourcePath: VaultPath | null }

/**
 * Build completion event - emitted when ALL tasks complete
 * 
 * # Architecture: Maximum Decoupling
 * Backend owns completion detection by tracking DetachedRegistry state.
 * Frontend simply listens and displays - no phase awareness needed.
 */
export type BuildComplete = { videos_converted: number; total_time_ms: number; 
/**
 * Number of symlinks (and macOS Finder Aliases) that were silently
 * skipped during the asset copy phase — e.g. broken targets, escape
 * attempts, or absolute-path targets that would 404 on deploy.
 * Zero when all symlinks were either preserved or not present.
 */
skipped_symlinks: number }

/**
 * The resting answer to "what will publishing do?".
 */
export type ChangeSet = { 
/**
 * False when there is no local record of the last publish. The verb counts
 * are then all zero and meaningless; `flat_upload`/`flat_remove` carry the
 * server's own count instead, and the UI must say it is not classified.
 */
classified: boolean; added: number; edited: number; deleted: number; restyled: number; 
/**
 * Changed output files with no source mapping: OG cards, webp variants,
 * video renditions, feeds, the sitemap. Counted, never enumerated, never
 * part of the ring — they are consequences of page changes, not peers.
 * 
 * Only *changed* ones. Counting every unmapped output would report
 * "1,847 assets" on a publish that changes nothing.
 */
assets: number; 
/**
 * Outputs that existed at the last publish and are gone now, with no page
 * behind them: a dropped image variant, a removed video rendition, an
 * orphaned OG card. The publish will send a `remove` for each.
 * 
 * Counted separately from `assets` because `assets` walks the CURRENT
 * output set and so is structurally blind to anything that disappeared —
 * without this, a publish whose only effect was removing files reported
 * itself as empty.
 */
removed_assets: number; 
/**
 * Degraded mode only: the server's own `need` count.
 */
flat_upload: number; 
/**
 * Degraded mode only: the server's own `remove` count.
 */
flat_remove: number; 
/**
 * Per-page detail, for the progress panel's second disclosure level.
 * Empty in degraded mode.
 */
pages: ChangedPage[]; 
/**
 * Public addresses (outside `_moss/`) the last publish on any target
 * served that this build does not, sorted by path. Unlike the verbs it is
 * filled in degraded mode too: it needs only a record to compare files
 * against, not a page-level baseline. See [`removed_addresses`].
 */
removed: RemovedAddress[] }

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

export type CreateTemplateArgs = { sourcePath: VaultPath; name: string; 
/**
 * The tree's own verdict that the source is a folder's home file.
 */
isHome: boolean; 
/**
 * `(language, page path)` pairs saved beside it as that language's file.
 */
translations: ([string, VaultPath])[] | null }

export type CreateTranslationArgs = { sourceAbs: VaultPath; lang: string }

/**
 * moss asking for one credential again, on a plugin's behalf.
 * 
 * Emitted when a plugin calls `rejectSecret`: the stored value is already
 * forgotten by the time this goes out, and the plugin's hook is parked until
 * [`crate::types::events::MossEvent::CredentialRequest`]'s answer comes back.
 * The plugin supplies `key` and `detail` and nothing else — every word that
 * describes the field comes from the manifest, and every pixel from moss.
 */
export type CredentialRequest = { 
/**
 * The parked question's id. The answer must quote it, so a click on a
 * stale modal cannot resolve a newer question.
 */
token: number; 
/**
 * Which plugin's store the answer is written to — resolved from the
 * dispatch seam, never from anything the plugin said.
 */
plugin: string; 
/**
 * Its display name, for the modal's title.
 */
plugin_name: string; key: string; 
/**
 * From `setup.credentials`, falling back to the key when the plugin
 * declared none: a field labelled `pinata_jwt` is bad, an unanswerable
 * modal is worse.
 */
label: string; help_url: string | null; 
/**
 * The plugin's one sentence saying why the old value stopped working.
 * Rendered as given — moss neither translates it nor trusts it as markup.
 */
detail: string | null }

/**
 * Provenance of a resolved publish date. Distinguishes explicit dates
 * (`Frontmatter`, `FilenamePrefix`) from implicit fallbacks (`Ctime`),
 * so consumers that care (e.g. file-tree zoning, RSS feed pubDate)
 * can treat them differently.
 */
export type DateSource = "frontmatter" | "filename_prefix" | "ctime" | "none"

/**
 * One way to reach the published site.
 * 
 * A publish produces several: the CID that names these exact bytes, the IPNS
 * name that will name the next ones too, the gateway URL that makes either
 * reachable from a browser. They are FACTS ABOUT THE SITE, not news about the
 * publish — which is why they live in the deployment record and render
 * standing in the deploy tab, instead of evaporating with the toast that
 * first announced them.
 */
export type DeployAddress = { kind: AddressKind; 
/**
 * Human label, the plugin's own words ("IPFS CID", "Gateway").
 */
label: string; 
/**
 * Openable in a browser. Present for `gateway`/`domain`, usually absent
 * for a bare `cid`.
 */
url?: string | null; 
/**
 * The literal string to copy — beside `url`, or on its own.
 */
value?: string | null; 
/**
 * One line of context the plugin wants shown beside it.
 */
note?: string | null }

/**
 * Progress event emitted during push.
 * Frontend listens on "deploy-progress" event.
 */
export type DeployProgress = { stage: DeployStage; current: number; total: number; message: string; 
/**
 * Cumulative bytes uploaded so far this deploy. `Some` only during the
 * Uploading stage; `None` on every other stage so the frontend falls back
 * to the file-count fraction. (File-count progress is
 * actively misleading for video-heavy sites — it reaches ~97% before the
 * videos start uploading.)
 * 
 * `f64`, not `u64`, deliberately: specta types `u64` as a TS `string`
 * (precision-safety), but serde emits a JSON number at runtime — a type/
 * runtime mismatch that breaks the frontend's `bytes_uploaded/bytes_total`
 * arithmetic. `f64` types as `number` and is exact for byte counts up to
 * 2^53 (≈ 9 PB). The hot accumulator stays `AtomicU64`; we widen at emit.
 */
bytes_uploaded?: number | null; 
/**
 * Total bytes to upload this deploy (sum of `diff.need` file sizes).
 */
bytes_total?: number | null; 
/**
 * Files this deploy will DELETE from the live site (`diff.remove.len()`).
 * 
 * Here rather than left to the resting change set, which is the answer to
 * a different question ("what would publishing change?"). Sending both
 * down one channel is what forced the frontend to guess which of the two
 * it was holding.
 */
removing?: number | null; 
/**
 * The largest upload currently in flight, as a site-relative path.
 * 
 * "The current file" is a choice, not a fact: 20 files upload at once. The
 * largest is the one whose seconds the author is actually waiting on — a
 * 40 MB video holds this slot for as long as it really takes, while the
 * hundreds of small files that flash past were never readable anyway.
 * 
 * Sampled at the ticker's 4 Hz. Minimum dwell and the swap animation are
 * display policy and live in the frontend; this reports only what is true
 * at the instant it is read.
 */
current_file?: string | null }

/**
 * Stages of a push operation, emitted as Tauri events.
 * 
 * Ordered roughly in the sequence they fire, though `Preparing` and
 * `Rebuilding` are skipped when the pre-deploy rebuild is a no-op (see
 * `should_skip_rebuild` in `deploy.rs`). `Finalizing` fires after the commit
 * for post-commit bookkeeping (redirect snapshot, analytics sync, domain
 * orchestrator) that used to run silently.
 */
export type DeployStage = 
/**
 * Waiting for in-flight background tasks (video conversion, etc.)
 * and checking whether a rebuild is needed.
 */
"preparing" | 
/**
 * Running the pre-deploy rebuild (skipped when hashes match).
 */
"rebuilding" | "syncing" | "uploading" | "committing" | 
/**
 * Post-commit bookkeeping: redirects, analytics, domain orchestrator.
 */
"finalizing" | 
/**
 * The upload committed and moss is confirming the site is actually
 * serving it (OnionPress publishes only). Emitted by `deploy_site` right
 * after the publish guard's `Complete`, so the LAST stage a verifying
 * publish shows is this one — the green success moment belongs to the
 * `PublishVerdict` event, not to upload completion.
 */
"verifying" | "complete" | "failed"

/**
 * What the author chose for a folder, as the UI names it.
 */
export type DerivationDecision = "undecided" | "show" | "keep"

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
 * The wire shape every host's editor reads: [`Diagnostic`] with an
 * LSP-compatible severity number (1 error, 2 warning, 3 info, 4 hint).
 */
export type EditorDiagnostic = { severity: number; message: string; 
/**
 * Frontmatter field path (e.g. "title", "also_in[0]").
 */
path: string | null; 
/**
 * 1-based line in the whole file, when known.
 */
line: number | null; 
/**
 * 1-based column, when known.
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
 * None on a site with no URL map, whose links answer `resolved_path`.
 */
url: string | null; 
/**
 * For Link kinds with a `#fragment`: the anchor.
 */
anchor: string | null; 
/**
 * The source FILE this reference opens when followed: root-relative from
 * moss-core, absolute once a desktop host's adapter has joined it to the
 * site root. Set for:
 * * `Link` and `Moved`: the page's source, from the URL map, or the page
 * file itself on a site with no map;
 * * `FolderListing`: the folder's index source, NOT the folder directory;
 * with no index source in the map, the folder's home file;
 * * `FolderIndexIframe`: the folder's static `index.html`;
 * * `Transclusion`, `Notebook` and `Table`: the embedded file.
 * 
 * Always a listed file, never a directory — every consumer opens it. A
 * folder with neither an index source nor a home file leaves this `None`,
 * so the editor shows no follow affordance rather than one that lands
 * nowhere.
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

export type ExpandDerivedArgs = { pagePath: VaultPath; 
/**
 * The editor's text, which may be ahead of the file.
 */
content: string }

/**
 * A page's text with its ready derived embeds written out.
 */
export type ExpandedDerived = { content: string; 
/**
 * How many embed lines were replaced.
 */
replaced: number }

/**
 * The render family a non-folder, non-link target belongs to, by extension.
 */
export type ExtKind = "Image" | "Iframe" | "Pdf" | "Video" | "Audio" | "Model" | "Transclusion" | "Notebook" | "Table" | "Other"

/**
 * A file the plugin could not read the last time it tried.
 */
export type FailedDerivedFile = { file: string; message: string }

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
 * A top-level frontmatter key, with one-based line and UTF-8 byte offsets.
 */
export type FieldSpan = { key: string; line: number; from: number; to: number }

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
 * File change event for live development mode.
 * 
 * Provides information about file system changes during file watching
 * to enable smart frontend responses (refresh vs redirect vs skip).
 * 
 * Two consumer paths, each reading a disjoint subset of fields:
 * 
 * **Output-domain** (consumed by `preview-actions.ts`):
 * - `changed_output_files`: refresh decisions
 * - `deleted_paths`: triggers redirect-home when current page is deleted
 * - `moved_output_paths`: triggers URL update when current page's output moved
 * 
 * **Source-domain** (consumed by `EntryRegistry` in the action panel — added
 * by the entry-id architecture):
 * - `source_creates`: new source files appearing this rebuild
 * - `source_deletes`: source files removed this rebuild
 * - `source_renames`: `(old_source_path, new_source_path)` pairs for the
 * same file moving to a new path (inode-paired by the watcher)
 * - `modified_paths`: page sources, plus `.moss/config.toml`,
 * `.moss/places.toml` and the theme's style.css/script.js, whose bytes
 * changed in place — what the editor reloads an open file from
 */
export type FileChangeEvent = { 
/**
 * Output files whose content hash changed (relative to the staging dir),
 * named in the event sent when their new bytes become servable: a page
 * the render wrote, at the show; a file the background writes (a copied
 * asset, an image variant, a video), at the seal tail. A key can be named
 * again: a page the tail repaired, or a key a newer render's tail lists
 * when an older render's tail left to it. The refresh signal: a client
 * refreshes when this names the page it shows or anything that page loads.
 */
changed_output_files: string[] | null; 
/**
 * Output paths (relative to the staging dir) that were deleted in
 * the rebuild. Frontend triggers redirect-home when the current iframe
 * path matches one of these. Same domain as `changed_output_files` and
 * `moved_output_paths` — see `pathMatchesCurrentPage` in
 * the desktop app's preview path-matcher for the matching rules.
 */
deleted_paths: string[] | null; 
/**
 * `(old_output_path, new_output_path)` pairs for any iframe target that
 * should follow rather than redirect-home. Two producers feed in here:
 * (1) filesystem-level renames stitched by `notify-debouncer-full` from
 * inode-paired Modify(Name(From/To)) events, and (2) source-stable URL
 * changes where the source filename is unchanged but its slug moved
 * (e.g., the user removed frontmatter `title:`). Same output domain as
 * `deleted_paths`. Frontend navigates the iframe to the new URL when
 * the current path matches an old path. Populated by
 * `build_rebuild_event_with_renames` in `watch.rs`.
 */
moved_output_paths: ([string, string])[] | null; 
/**
 * **Source-domain.** New source files appearing this rebuild (project-
 * root-relative paths, forward-slash separators). Deduped against
 * `source_renames` — a path that's the NEW side of a rename does not
 * appear here. Consumed by `EntryRegistry.upsert` to mint EntryIds for
 * newly-visible files.
 */
source_creates: string[] | null; 
/**
 * **Source-domain.** Source files removed this rebuild. Deduped against
 * `source_renames` — a path that's the OLD side of a rename does not
 * appear here. Consumed by `EntryRegistry.delete` to retire EntryIds.
 */
source_deletes: string[] | null; 
/**
 * **Source-domain.** `(old_source_path, new_source_path)` pairs for FS
 * renames stitched by `notify-debouncer-full`. Consumed by
 * `EntryRegistry.rename` to preserve EntryId across rename. Note this
 * is the SOURCE-domain analogue of `moved_output_paths`; the same FS
 * rename event populates both fields (the registry consumes source
 * pairs; the preview consumes the resolved output pairs).
 */
source_renames: ([string, string])[] | null; 
/**
 * **Source-domain.** Page sources (markdown, slot files included), plus
 * `.moss/config.toml`, `.moss/places.toml` and the theme's
 * `style.css`/`script.js` — the only other vault sources read
 * synchronously enough for this diff to be race-free (see
 * `manifest::is_reload_tracked_source_key`) — whose bytes
 * changed in place this rebuild: another app's edit, or moss's own save.
 * Deduped against `source_creates` and `source_renames`. The editor
 * reloads the file it has open from this (deferred while it holds unsaved
 * edits); the preview ignores it — `changed_output_files` is the refresh
 * signal. Populated by `build_rebuild_event_with_renames` in `watch.rs`,
 * which emits it even when no output changed.
 * See `MossEvent::SourceContentChanged` for the pre-rebuild announcement.
 */
modified_paths: string[] | null; 
/**
 * The render this event belongs to (`lifecycle::show_render`'s number):
 * a render sends one event when it is shown and one from its seal tail.
 * Nothing dedupes on it: a key named twice is a second change, or at
 * worst a second refresh. `None` when no render was minted.
 */
render_seq: number | null }

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
 * One folder and one derivation.
 */
export type FolderDerivation = { 
/**
 * Site-relative with a trailing slash; `/` is the site root.
 */
folder: string; derivation_id: string; noun_one: string; noun_other: string; count: number; decision: DerivationDecision; 
/**
 * Pages the folder is still missing (0 unless shown).
 */
stubs_missing: number; skipped: SkippedDerivedFile[]; failed: FailedDerivedFile[] }

/**
 * The author's answer for a folder: show its files as pages with this
 * derivation, or keep them as files.
 */
export type FolderDerivationChoice = { show: string } | "keep"

/**
 * What the LAST BUILD listed under a folder, read from the build's URL map.
 * Attached to `FolderListing` embeds so the editor's card can state what will
 * be listed and how, without reimplementing the build's selection.
 * 
 * The fidelity limits of the underlying derivation are documented on
 * [`super::urls::FolderFacts`]; the editor renders these
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

export type FolderPathArgs = { folderPath: VaultPath }

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

export type InstantiateTemplateArgs = { id: string; targetDir: VaultPath; name: string }

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
 * The link syntax the caret is inside. Decides how a target is written and
 * which kinds rank first.
 */
export type LinkSyntax = 
/**
 * `[[…]]` — Obsidian form: pages by stem, assets by filename.
 */
"wikilink" | 
/**
 * `![[…]]` — same forms, assets first.
 */
"embed" | 
/**
 * `[text](…)` — the one syntax with two address spaces: a leading `/`
 * means the published site, anything else means the source tree.
 */
"inline" | 
/**
 * `![alt](…)`, `image=…`, a gallery body line — an asset path, never a page.
 */
"asset-path"

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
 * The reason the login browser-panel triggered the failure view.
 * Serialized to the frontend via `MossEvent::LoginBrowserFailed`. The probe
 * that produces it stays app-side (`plugins/login_probe.rs`).
 */
export type LoginFailureReason = 
/**
 * Both control hosts (1.1.1.1:443, 8.8.8.8:443) unreachable: no internet.
 */
"offline" | 
/**
 * Controls reachable; target TCP RST arrived in < 1 s (likely blocked).
 */
"likely_restricted" | 
/**
 * Controls reachable; target TCP timed out (≥ 5 s).
 */
"unreachable" | 
/**
 * Controls reachable; TCP connected but TLS handshake failed.
 */
"tls_error" | 
/**
 * Network appears fine, but the webview page loaded with an empty body.
 */
"loaded_blank"

/**
 * A login the browser-panel can show; the shell draws each in its own words.
 */
export type LoginKind = "matters" | "ghost"

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

export type MossEvent = { kind: "BackgroundProgress"; payload: BackgroundProgress } | { kind: "AssetReady"; payload: AssetReady } | 
/**
 * Post-seal sweep of materialized image/video variants (page-agnostic
 * srcset swap relay). See `crate::build::progress::AssetsSettled`.
 */
{ kind: "AssetsSettled"; payload: AssetsSettled } | { kind: "VideoConversionProgress"; payload: VideoConversionProgress } | { kind: "BuildComplete"; payload: BuildComplete } | 
/**
 * A previously deferred source walk or editor listing settled. This is
 * independent of build completion and byte-download admission.
 */
{ kind: "SourceStructureSettled"; payload: { folder_path: string; path: string; outcome: SourceStructureOutcome } } | 
/**
 * `current` now points at this generation. `BuildComplete` says the build
 * finished; this says the result is what a preview would load. They are
 * not the same instant, and the gap between them is where a UI can
 * truthfully-but-wrongly report a change as live — see `SitePromoted`.
 */
{ kind: "SitePromoted"; payload: SitePromoted } | { kind: "PluginProgress"; payload: PluginProgressEvent } | { kind: "FileChanged"; payload: FileChangeEvent } | { kind: "DeployWaitingForBuild" } | 
/**
 * What publishing right now would change on the live site, recomputed at
 * every seal and once more when a deploy discovers there is no local
 * record to classify against.
 * 
 * On the bus rather than a `Channel<T>` (ownership row 21's chooser)
 * because it has several independent subscribers — the publish button's
 * composition ring, its tooltip, the progress panel, and later the globe
 * badge — which are three renderings of ONE change set. It is also a
 * resting, low-frequency value, not a progress stream: `deploy-progress`
 * stays off the bus for exactly the opposite reason.
 */
{ kind: "PublishChangeSet"; payload: ChangeSet } | 
/**
 * Preview server could not bind / verify ready. The build pipeline still
 * completes (with `port: None` in the eventual `ProgressUpdate`), but
 * without this signal the frontend's `preview-state.ts` silent-warns on
 * the portless completion and leaves the panel stuck at the last
 * `BackgroundProgress` value. This event surfaces the real failure so
 * the user sees an error instead of a hung progress bar.
 * 
 * `open_in_another_app`: another process's desktop app is serving the
 * folder.
 */
{ kind: "PreviewServerFailed"; payload: { message: string; open_in_another_app: boolean } } | { kind: "ActionPanelLoaded" } | { kind: "ActionPanelOpened"; payload: ActionPanelOpened } | { kind: "ActionPanelClosed" } | { kind: "DividerPositionChanged"; payload: { position: number } } | 
/**
 * Preview-tiling mode flip (wry#175 cursor constraint — see
 * system/preview_tiling.rs). `tiled: true` → the preview webview is
 * natively tiled beside the panel; the shell zeroes its
 * `--action-panel-width` inset (the webview origin replaces it).
 * `tiled: false` → full-window webview; `inset` carries the px the
 * shell should write (the live panel width during a divider gesture,
 * 0 when no panel is visible). divider.ts `applyPreviewTiling` is the
 * frontend sink.
 */
{ kind: "PreviewTiling"; payload: { tiled: boolean; inset: number } } | { kind: "OpenSettingsModal" } | { kind: "FullscreenChanged"; payload: { is_fullscreen: boolean } } | 
/**
 * Fires at the START of a native fullscreen ENTER/EXIT transition (macOS
 * NSWindowWillEnter/ExitFullScreen), before macOS reveals/hides the traffic
 * lights — so the frontend can move chrome (the corner Edit button) first.
 * `FullscreenChanged` (emitted later, on Resized) remains the backstop.
 */
{ kind: "FullscreenWillChange"; payload: { will_be_fullscreen: boolean } } | { kind: "ShowToast"; payload: ToastPayload } | 
/**
 * Mutate an existing toast in place. `id` (in payload) selects which.
 */
{ kind: "ShowToastUpdate"; payload: ToastPayload } | 
/**
 * `PanelTask` lifecycle event. Emitted by `TaskRegistry`
 * after every spawn / progress / awaiting / terminal transition.
 * Frontend renderers (`breadcrumb-hairline.ts`, the progress panel,
 * future Inline/Narrated/Awaiting renderers) filter on `payload.tone`
 * and `payload.scope` to decide whether to react.
 * 
 * `PanelTask` contains `Instant`, so the wire carries
 * `PanelTaskWire`, which strips it. See the
 * module docstring in `tasks.rs` for the full rationale.
 */
{ kind: "PanelTaskUpdate"; payload: PanelTaskWire } | 
/**
 * A source image file was created, modified (in-place), deleted, or
 * renamed on the filesystem. Emitted by the file watcher BEFORE the
 * rebuild-eligibility gate — fires even if the change does not trigger a
 * rebuild (e.g. the content-hash gate suppresses it). The editor's
 * reference resolver uses this to bump the per-asset cache-bust token so
 * inline image widgets repaint with fresh source bytes, and to clear its
 * resolved cache so a deleted/renamed source re-resolves to not-found.
 * 
 * `path` is project-root-relative with a leading `/`
 * (e.g. `/图片/摄影/X.jpeg`), matching the `request_url` the editor uses
 * for its `moss-source://localhost<request_url>?_t=<token>` image URLs.
 */
{ kind: "SourceAssetChanged"; payload: { 
/**
 * Project-root-relative path with a leading `/`, e.g. `/assets/photo.jpg`.
 */
path: string } } | 
/**
 * Reload-tracked source bytes changed according to a watcher batch, before
 * any rebuild gate. Includes pages and the four synchronous `.moss` sources
 * selected by `manifest::is_reload_tracked_source_key`; not hash-confirmed.
 * The editor listens to reload its open file (deferred while it has unsaved
 * edits). This is not a build result and carries no output facts; tree and
 * preview consumers do not subscribe. A build's `FileChanged.modified_paths`
 * may repeat these paths.
 */
{ kind: "SourceContentChanged"; payload: { 
/**
 * Project-relative keys, forward slashes, no leading slash: the
 * `FileChangeEvent.modified_paths` domain.
 */
paths: string[] } } | 
/**
 * A file was created on disk via a RAW FS event (outside a process-hook build).
 * Emitted before the rebuild-eligibility gate so the editor can flash the deepest
 * visible ancestor folder even when the file doesn't trigger a rebuild.
 * `paths` are project-root-relative, forward-slash, NO leading slash (the
 * `path_to_relative_key` domain — NOT the leading-slash moss-source:// domain).
 */
{ kind: "RawFileCreated"; payload: { paths: string[] } } | 
/**
 * The editor saved the person's text over lines another writer had
 * changed, and kept that writer's whole page first as `version_id` in the
 * site's history (`moss history <page> --restore --at <version_id>`
 * brings it back). `path` is the page's path inside the site. Sent so an
 * agent that wrote the page learns its edit is no longer in the file.
 */
{ kind: "OutsideEditKept"; payload: { path: string; version_id: string } } | 
/**
 * Ask the app shell (main webview) to open the plugin
 * catalog, which rests on channels. Produced by the editor webview's
 * "Plugin" card; consumed by `app.ts`'s `onMossEvent("OpenPluginsView", …)`
 * listener which opens the settings modal at the `channels` section.
 * 
 * **Routed via `emit_to("main", …)` — NOT broadcast.** Tauri webviews
 * are isolated `Window` instances; a `window.dispatchEvent` from the
 * editor webview never reaches the main webview's listener, so a
 * `CustomEvent` would silently no-op.
 * `app.emit_to("main", …)` is the supported cross-webview path.
 * See project memory: "NEVER use `app.emit()` broadcast".
 */
{ kind: "OpenPluginsView"; payload: { 
/**
 * Absolute folder path the user is onboarding — surfaced so the
 * catalog UI can pre-fill any "install into this site" affordance
 * without re-resolving the active project.
 */
folder_path: string } } | 
/**
 * Ask the app shell to restore the editor in the action panel after a
 * login flow cancel/failure. Produced by the plugin (via the
 * `return_to_editor` Tauri command) when `promptLogin()` returns false;
 * consumed by `app.ts`'s `onMossEvent("ReturnToEditor", …)` which clears
 * the onboarding latch and re-mounts the editor (empty-folder onboarding
 * cards). Routed via `emit_to("main", …)` — NOT broadcast.
 */
{ kind: "ReturnToEditor" } | 
/**
 * The login browser-panel (matters.town) failed to load meaningful content:
 * either the navigation timed out or the page body was empty after load.
 * Carries the probe-based classification so the shell can show a calm,
 * reason-specific failure overlay with a single **Retry** button.
 * 
 * On receipt the shell HIDES the browser-panel webview (exposing the shell
 * region) and renders the failure overlay.  Retry re-shows the webview and
 * triggers a reload.  Routed via `emit_to("main", …)` — NOT broadcast.
 * 
 * Design: C3.
 */
{ kind: "LoginBrowserFailed"; payload: { reason: LoginFailureReason } } | 
/**
 * Matters is installed in the current project but has no valid session
 * (no `boundUserName` in config.json) and the user has not intentionally
 * dismissed the auto-open for this folder.
 * 
 * Emitted by `spawn_process_hooks` on the first background build/preview
 * run for a folder (before `execute_process` runs). The app shell
 * (`app.ts`) responds by calling `connect_account("matters")` ONCE
 * per folder session — a per-folder latch in TS prevents re-opening on
 * every incremental rebuild.
 * 
 * `project_path` lets the frontend tie the signal to the correct folder
 * and verify it hasn't been superseded by a folder switch.
 */
{ kind: "MattersNeedsConnection"; payload: { project_path: string } } | 
/**
 * The app UI language changed at runtime (live switch, no restart).
 * 
 * `locale` is the FRONTEND locale tag ("en" | "zh-hans" | "zh-hant") from
 * `AppLanguage::as_locale_str()` — hyphenated, matching the frontend
 * `AppLocale`. This is deliberately NOT the specta-generated `AppLanguage`
 * TS type (which is snake_case: "zh_hans"). Every app-chrome webview listens
 * (`onMossEvent('LocaleChanged')`) and re-localizes; preview/plugin webviews
 * have no listener and ignore it.
 */
{ kind: "LocaleChanged"; payload: { locale: string } } | 
/**
 * A publish-verification transition: `checking` when a publish arms (or
 * resumes on folder open), then exactly one of `live` (the single
 * success moment — the publish flow's green celebration keys off THIS,
 * never off upload completion) or `unreachable` (a resting diagnosis
 * that keeps watching and is reversed by a later `live`). Emitted on
 * TRANSITIONS ONLY — the supervisor's fold returns at most one per
 * probe; steady states are silent, and `get_publish_verdict` is the
 * pull-side catch-up for a webview that mounted mid-check.
 */
{ kind: "PublishVerdict"; payload: PublishVerdict } | 
/**
 * The Rust-assembled record of a finished publish, replacing the
 * `deploy_success_toast`/`announce_deploy_success` pair. Delivered
 * through the workspace feedback queue
 * (`queue_workspace_feedback_and_ping`, pull-only) to the preview
 * surface — the same routing `ShowToast` uses today.
 */
{ kind: "PublishReceipt"; payload: PublishReceipt } | 
/**
 * The nested-site guard is scanning the folder the user just opened —
 * the threshold screen's checking narration. Emitted once before a scan
 * starts (the bounded walk is synchronous and reports no live count
 * yet), and only when a preview webview exists to render it. `token`
 * names the guard pass; the frontend ignores events older than the
 * newest pass it has seen. Routed via `emit_to("preview", …)` — NOT
 * broadcast.
 */
{ kind: "ThresholdChecking"; payload: { token: number; dirs_visited: number } } | 
/**
 * The guard's one question (nested-site threshold screen, cases
 * (a)/(b)/(d)). The transport acks receipt
 * (`acknowledge_threshold_prompt`) and the guard then awaits exactly
 * one `submit_threshold_decision` quoting the prompt's token; a closed
 * window resolves the wait to Cancel, and an un-acked prompt falls back
 * to the dialog. Wire contract: the threshold types above, mirrored by
 * the desktop app's threshold seam.
 * Routed via `emit_to("preview", …)` — NOT broadcast.
 */
{ kind: "ThresholdPrompt"; payload: ThresholdPrompt } | 
/**
 * The guard pass named by `token` ended. `cancelled: true` → the
 * frontend restores what the overlay showed before the question;
 * `false` → the screen stays up for the build's own overlay to paint
 * over (no interstitial flash). A resolve for a pass the frontend is
 * not showing is ignored — a predecessor's late teardown must not wipe
 * its successor's live question. Routed via `emit_to("preview", …)` —
 * NOT broadcast.
 */
{ kind: "ThresholdResolved"; payload: { token: number; cancelled: boolean } } | 
/**
 * A plugin rejected a stored credential and moss is asking for the
 * replacement. The plugin's hook is parked on the answer, so the shell
 * must resolve every one of these exactly once — with a value, or with a
 * cancel. Routed via `emit_to("preview", …)`, like the threshold prompt
 * and for the same reason: only the shell may answer it
 * (`submit_credential_prompt`).
 */
{ kind: "CredentialRequest"; payload: CredentialRequest } | 
/**
 * The open folder's health verdicts, surfaced (phase 3 of the
 * watcher-reliability design). Re-judged by the folder's sweep every
 * tick (~2s) and emitted on TRANSITIONS ONLY — a healthy folder is
 * silent, and both flags going quiet is itself a transition, so the
 * surfaces always hear the all-clear.
 * 
 * - `unavailable` — the sweep's "project unavailable" verdict
 * (`FolderSession::is_unavailable`): consecutive root-unreadable or
 * deadline-blown passes. The preview shows a full-window state; no
 * rebuilds run until the folder comes back.
 * - `degraded` — the rebuild worker's overdue-build watchdog
 * (`WorkerHandle::is_degraded`): a build past its 300s deadline is
 * still running and the preview may lag the latest edits. Diagnostics
 * level: a corner-panel advisory, never a window grab.
 * 
 * Routed via `emit_to("preview", …)` — NOT broadcast.
 */
{ kind: "FolderHealthChanged"; payload: { folder: string; unavailable: boolean; degraded: boolean } } | 
/**
 * Windows only: the invisible `HTMAXBUTTON` Snap Layouts overlay
 * (`platform/windows/snap_overlay.rs`, Tasks 6-7 of the windows custom
 * caption design) was entered
 * or left. The overlay covers the HTML maximize button and answers
 * non-client hit-testing itself, so ordinary CSS `:hover` never fires on
 * that button; this is the substitute signal. `hovering: true` on
 * enter, `false` on leave — emitted only on a CHANGE, never repeated
 * while the pointer sits still inside the overlay.
 */
{ kind: "SnapOverlayHover"; payload: { hovering: boolean } } | 
/**
 * Companion to `SnapOverlayHover`: the overlay was clicked
 * (`WM_NCLBUTTONUP`). No payload — the actual maximize/restore toggle is
 * the frontend's job (`Window::toggleMaximize()`), not this event's;
 * the overlay is a child window, and `DefWindowProcW`'s own
 * non-client button handling has no effect on a child (only a real
 * top-level caption maximizes itself that way).
 */
{ kind: "SnapOverlayClick" } | 
/**
 * One starter's whole row changed in the desktop app's starter picker:
 * a download began, progressed, finished or failed. Broadcast to every
 * webview, since the picker and the preview frame live in different
 * ones; each keeps the row with the higher `rev`.
 */
{ kind: "StarterState"; payload: StarterStateRow }

export type MoveEntriesArgs = { paths: VaultPath[]; targetDir: VaultPath }

/**
 * One nested site the bounded walk found under the opened root.
 */
export type NestedRootInfo = { 
/**
 * '/'-separated, root-relative (build convention).
 */
rel_path: string; abs_path: string; 
/**
 * Folder basename = the future site name (`VaultRoot::name` semantics).
 */
name: string; 
/**
 * true = published (site_id present) · false = preview-only ·
 * null = state unreadable (evicted).
 */
published: boolean | null; site_id: string | null }

/**
 * The detector's report for one opened root, wire form.
 */
export type NestedRootsReport = { root: string; root_class: RootClass; 
/**
 * ≤ 5, shallowest-first.
 */
nested: NestedRootInfo[]; 
/**
 * A depth/dir/report cap was hit — the walk may have missed sites.
 */
truncated: boolean; 
/**
 * Telemetry, and the live count the checking narration shows.
 */
dirs_visited: number }

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
 * One page of the served build.
 */
export type PageInfo = { 
/**
 * The page's file under the served directory, e.g. `posts/article.html`.
 */
url_path: string; 
/**
 * Its `<title>`, or a name made from its path when it has none.
 */
title: string }

/**
 * Where a page sits in its site: an ordinary page, a folder's index, or a
 * home (the site root's index, or the root index of a language tree such as
 * `en/`).
 */
export type PagePosition = "page" | "folder" | "home"

/**
 * A one-page preview as hosts receive it: a full document, its title and the
 * renderer's diagnostics. Page checks come from [`check_page`].
 */
export type PagePreview = { 
/**
 * [`content_hash`] of the text this render came from.
 */
revision: string; html: string; title: string; diagnostics: EditorDiagnostic[] }

/**
 * Result of resolving a preview URL to its source file and page type.
 */
export type PageSource = { 
/**
 * Absolute path to the file behind the URL, if found: a page's markdown
 * source, or the source of any other file the build serves (a PDF, a picture).
 * It is not always a page: a caller that needs one moss renders (the
 * build's preview requirement takes only a page source) checks that itself.
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
 * The channels this article's copies go to, its folder's cascade applied:
 * what `syndicated` will list once they are sent. `None` when neither the
 * page nor a folder names any; empty for `syndicate: false`.
 */
syndicate?: string[] | null; 
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
source_pending?: boolean; 
/**
 * False when whether this URL has a source is not known yet: the build's
 * page map is missing or unreadable and the disk found none, and only the
 * map tells a generated page from a source under a lossy slug. The next
 * build settles it. `source_path: None` is a claim only while this is true.
 */
source_known?: boolean }

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
 * One page row's own post-landing reachability verdict — task 4-6's
 * per-URL burst judges each row by what its own kind promises (design
 * §4a): Added wants the new URL live with matching bytes, Moved wants the
 * old URL still serving the redirect stub, Removed wants the old URL gone.
 * `path` always echoes the row's own `PublishReceiptPage.path` (the URL
 * the receipt already shows for it), never the URL that was actually
 * fetched, so the frontend can match a verdict back to its row by
 * `(kind, path)` alone.
 */
export type PageVerdict = { kind: PublishReceiptPageKind; path: string; ok: boolean }

/**
 * Serializable snapshot of a `PanelTask` for the frontend event bus.
 * 
 * `PanelTask` itself isn't `Serialize` because `started_at: Instant` cannot
 * cross the FFI boundary. This struct mirrors every other field plus a
 * `started_at_ms` derived from `Instant::elapsed()` (milliseconds since
 * spawn — frontend uses it to compute "this task is N ms old" without
 * needing wall-clock alignment with Rust).
 * 
 * Emitted via `MossEvent::PanelTaskUpdate` after every state transition.
 * Frontend renderers (Ambient hairline, Inline badge, Narrator) filter on
 * `tone` and `scope` to decide whether to react.
 */
export type PanelTaskWire = { id: string; window_id: WindowId; scope: TaskScope; kind: TaskKind; tone: TaskTone; title: string | null; 
/**
 * `Some(ms)` for Inline terminal receipts ("Saved · 2s ago" =
 * `fade_after_ms: Some(30000)`). `None` for persistent live status.
 */
fade_after_ms: number | null; 
/**
 * Milliseconds elapsed since the task was spawned (derived from
 * `Instant::elapsed()`). Renderers use this for fade-after-N-ms logic.
 */
started_at_ms: number; 
/**
 * The Job's past-tense verb for the receipt header (Step 3 R2).
 */
verb: Verb | null; 
/**
 * The success quantity (R2); present iff terminal `Succeeded` (#4).
 */
amount: Amount | null; 
/**
 * The frozen terminal span in millis (R-a) — the receipt's elapsed,
 * NOT the live-ticking `started_at_ms`. `None` until terminal.
 */
elapsed_ms: number | null; 
/**
 * Parent Job id for sub-jobs (media under Build).
 */
parent: string | null; state: TaskState }

/**
 * Frontmatter parsing result exposed to the frontend.
 */
export type ParsedFrontmatter = { 
/**
 * [`crate::page::content_hash`] of the file's bytes as read, before any newline
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
 * Set only by [`super::classify_source_read`], which is the one place in the
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
 * Plugin hook progress payload — carried by `MossEvent::PluginProgress`.
 * 
 * Fields mirror the legacy inline JSON shape that frontends already consume.
 */
export type PluginProgressEvent = { 
/**
 * Display name of the plugin emitting progress (e.g. `"Enhance"`).
 */
plugin_name: string; 
/**
 * Hook the progress relates to (e.g. `"enhance"`, `"process"`,
 * `"deploy"`, `"syndicate"`).
 */
hook_name: string; 
/**
 * Free-form phase identifier: `"collecting"`, `"running"`, `"done"`, etc.
 */
phase: string; 
/**
 * Current item index. `0` when total is `0`.
 */
current: number; 
/**
 * Total items. `0` when progress is indeterminate.
 */
total: number; 
/**
 * Optional human-readable status message.
 */
message: string | null; 
/**
 * `true` on the final emit for this hook.
 */
completed: boolean }

/**
 * Real-time progress update for build process.
 * 
 * Provides structured progress information for the frontend
 * to display during website build and server startup.
 */
export type ProgressUpdate = { 
/**
 * Current step being executed
 */
step: string; 
/**
 * Detailed message about current operation
 */
message: string; 
/**
 * Progress completion percentage (0-100)
 */
percentage: number; 
/**
 * Whether this step is completed
 */
completed: boolean; 
/**
 * Preview server port when server is ready (optional)
 */
port: number | null; 
/**
 * Whether the site is empty (no content files found)
 */
is_empty: boolean | null }

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

/**
 * The single record a finished publish hands to the frontend, replacing
 * the `deploy_success_toast`/`announce_deploy_success` pair (step 2 of the
 * publish-receipt plan).
 * `generation_id` joins the receipt to the publish-history record
 * so the two never grow a second, competing "what did I just publish"
 * answer.
 */
export type PublishReceipt = { target: PublishTarget; url: string; host: string; first_publish: boolean; generation_id: string; uploaded: PublishReceiptUploaded; pages: PublishReceiptPage[]; live: PublishReceiptLive; newsletter: PublishReceiptNewsletter | null; domain: PublishReceiptDomain | null; 
/**
 * Where the site is reachable (its URL forms, such as a gateway or an
 * identifier) — standing facts, not what this publish removed; see
 * `removed_public_addresses` for that.
 */
addresses: DeployAddress[]; 
/**
 * Public addresses (served form, e.g. `/feed.xml`) this publish stopped
 * serving that no removed or moved page row in `pages` already names:
 * files, and pages whose address went offline without the page being
 * deleted. Only the ones the receipt shows (they share the page rows'
 * cap); `hidden_rows` counts the rest. Empty until the page-change
 * summary is folded in.
 */
removed_public_addresses: string[]; 
/**
 * How many receipt rows, page rows and removed-address rows together,
 * did not fit the cap and are not shown.
 */
hidden_rows: number }

/**
 * The custom domain this publish is reachable under, when one is
 * configured and folded in (step 6). `None` otherwise.
 */
export type PublishReceiptDomain = { host: string; confirmed: boolean }

/**
 * The receipt's own reachability verdict for the published URL. Distinct
 * from `PublishVerdictState` (the live-updating toast/panel state machine):
 * this is a snapshot taken once, at receipt-build time — a moss-hosted
 * publish starts at `Checking` (`PublishReceipt::from_moss_push`) and the
 * frontend patches it in place from the `PublishVerdict` event task 3b's
 * burst emits once the check settles (task 3c). `Unverifiable`
 * is the only value a non-moss target (or a target step 3 never wires) ever
 * carries.
 */
export type PublishReceiptLive = "unverifiable" | "checking" | "live" | "unreachable_here" | "offline" | "incomplete"

/**
 * The armed-article newsletter send this publish carries, when the
 * frontend patched one on (step 5). `None` when no article is armed.
 */
export type PublishReceiptNewsletter = { armed: boolean; subscribers: number }

/**
 * One row of the receipt's page list. Empty until step 4 backfills it —
 * see `PublishReceipt::from_moss_push`.
 */
export type PublishReceiptPage = { kind: PublishReceiptPageKind; title: string; path: string; old_path: string | null }

/**
 * What happened to one page in this publish.
 */
export type PublishReceiptPageKind = "added" | "moved" | "removed"

/**
 * File-count summary of what a push moved. `pages_*` stay `0` until step 4
 * of the publish-receipt plan backfills them from the change summary.
 */
export type PublishReceiptUploaded = { files_uploaded: number; files_removed: number; pages_added: number; pages_updated: number; pages_moved: number; pages_removed: number }

/**
 * Where a publish landed. `Other` covers a plugin target moss has no
 * dedicated case for yet — rendered as label + value, never dropped.
 * 
 * `Deserialize` joined the derive at task 5.4: `get_publish_verdict` grew a
 * `target` parameter so the email composer (a separate webview) can ask for
 * the moss-hosting verdict specifically, instead of always reading the
 * OnionPress supervisor slot — every other use of this type is still
 * outbound-only (it rides `PublishVerdict`/`PublishReceipt` to the
 * frontend), so this is its first inbound crossing.
 */
export type PublishTarget = "moss" | "onionpress" | "github-pages" | "other"

/**
 * One `publish-verdict` wire payload. `state` transitions are the ONLY thing
 * emitted — elapsed is a frontend-local timer, and the mount-time drain pulls
 * the same shape through `get_publish_verdict`. The verification supervisor
 * that produces it stays app-side (`system/stack_serving/verify.rs`).
 */
export type PublishVerdict = { 
/**
 * Which publish this verdict is about — `handlePublishVerdict`
 * (`deploy-handler.ts`) dispatches on this instead of reading `state`
 * values as an implicit target signal (task 3e; `PublishVerdict` had no
 * target at all until this field, so a moss-hosted `live`/`checking`
 * fell into OnionPress's branches by default).
 */
target: PublishTarget; 
/**
 * The folder (site root) this verdict is about. A verification burst is
 * fire-and-forget and can land after the author has switched to a
 * different folder, so every listener that applies a verdict to visible
 * state must first check this against whatever folder it is currently
 * showing and ignore a mismatch — otherwise a stray verdict for the
 * folder just left behind is indistinguishable from one about the
 * folder now open.
 */
folder: string; state: PublishVerdictState; 
/**
 * What produced this verdict: a publish that just landed, or moss
 * re-checking an already-published folder on open. A resume is not news
 * — the folder was already published, and this only reconfirms it — so
 * a listener may update state from it silently but must never toast or
 * otherwise interrupt on it; only a `Publish`-origin verdict may.
 */
origin: PublishVerdictOrigin; generation: string; url: string; 
/**
 * The receiver's last effective reachability code (`"200"`, `"takeover"`,
 * `"000:rc=28"`, …), or moss's own `"down"` when the receiver itself is
 * not answering. `None` when there is no fresh verdict at all.
 */
http_code: string | null; 
/**
 * Tor's bootstrap percentage, when the receiver reported one — feeds the
 * "Tor is still connecting ({pct}%)" escalation line.
 */
bootstrap_pct: number | null; 
/**
 * Per-URL verdicts for the page rows the receipt shows (task 4-6,
 * design §4a extended to page rows), verified in the same burst once
 * the control probe confirms this generation. Always empty for
 * OnionPress — the per-URL burst is moss-hosting only — and for a
 * moss-hosted verdict whose control probe never confirmed (`Offline`,
 * `Incomplete`): with no confirmed generation there is nothing
 * trustworthy to say about any one page either.
 */
pages: PageVerdict[] }

/**
 * What armed the verification session a [`PublishVerdict`] reports on: a
 * publish that just landed, or moss re-checking a folder that was already
 * published, done on open so a hosted site's status survives a relaunch or
 * a folder switch. Every transition announced while one arming stands
 * carries that same origin — a `Live`/`Unreachable` that follows a `Resume`
 * arm is itself a `Resume`, because it is answering the same "is this still
 * true" question the resume asked, not a fresh publish event.
 */
export type PublishVerdictOrigin = "publish" | "resume"

/**
 * The verification state machine's announcable states. `Checking`/`Live`/
 * `Unreachable` are OnionPress's; the other three are moss-hosting's,
 * produced by `classify_moss_verification` — `Live` is shared rather than
 * duplicated between the two.
 * `rename_all` is `snake_case`, not the old `lowercase`, so `UnreachableHere`
 * serializes as `"unreachable_here"`; every pre-existing variant is a single
 * word, so its wire value is unchanged by the switch.
 * 
 * There is deliberately no "the public address is serving an older version"
 * state. A CDN that rewrites HTML on the way out makes the served bytes
 * differ from the published bytes on every request, so that state was
 * unfalsifiable from the client and fired on healthy sites.
 */
export type PublishVerdictState = "checking" | "live" | "unreachable" | 
/**
 * Control probe confirms the new generation; the public fetch itself
 * failed (timeout, DNS, TLS, a non-2xx answer) — a connection problem on
 * this computer, not the site.
 */
"unreachable_here" | 
/**
 * The control probe itself is unreachable: this computer looks offline,
 * and nothing about the site can be concluded.
 */
"offline" | 
/**
 * The control probe reports moss's backend is still serving an older
 * generation — the publish did not complete.
 */
"incomplete"

export type ReadVersionArgs = { id: string; path: string }

/**
 * One reference to resolve: the raw target text plus whether it appeared in an
 * embed context (`![[...]]` / `![](...)`) or a plain link (`[[...]]` / `[](...)`).
 * `is_embed` is what drives the classifier's Link-vs-embed fork.
 */
export type RefTarget = { text: string; is_embed: boolean }

export type ReferenceCardArgs = { fromFile: VaultPath; target: string }

export type ReferenceCardInfo = { title: string; description: string | null; source_path: string | null; generated: boolean; heading: string | null }

export type ReferenceKind = { kind: "link"; data: { anchor: string | null } } | { kind: "image" } | { kind: "iframe" } | { kind: "pdf" } | { kind: "video" } | { kind: "audio" } | { kind: "model" } | { kind: "folder-listing" } | { kind: "folder-index-iframe" } | { kind: "transclusion" } | { kind: "notebook" } | { kind: "table" } | { kind: "external"; data: { url: string } } | { kind: "anchor" } | { kind: "ambiguous" } | { kind: "not-found" } | 
/**
 * An internal link whose URL no longer exists: the page it named now
 * lives at `canonical` (a claimed term's generated URL). `url` on the
 * resolved reference carries `canonical` so following it lands there.
 */
{ kind: "moved"; data: { canonical: string } }

/**
 * Why a public address that was live is not in this build.
 */
export type RemovalReason = 
/**
 * The source that produced it no longer exists in the site folder.
 */
"author_removed" | 
/**
 * The page's source says `draft: true`: it is built for preview and never
 * published, an author's act like deleting it.
 */
"withheld" | 
/**
 * Anything else: a generated file nobody produces now, a source that
 * still exists whose address changed, a file whose bytes moved to
 * another address, or a record too old to say.
 */
"unexplained" | 
/**
 * The last publish served the address to members only and this build
 * serves it to everyone.
 */
"opened"

/**
 * A public address the last publish served that this build will not.
 */
export type RemovedAddress = { 
/**
 * Build output path, e.g. `feed.xml` or `a/index.html`.
 */
path: string; reason: RemovalReason; 
/**
 * Where what was served here is served now, with no redirect: a page
 * whose source still exists under a new slug or permalink, or a file
 * whose bytes moved to another address. Lets a later step offer the
 * redirect.
 */
moved_to?: string | null; 
/**
 * The source file the last publish recorded for this address: a page's
 * or a folder file's. Absent for a generated file, and for a record too
 * old to name folder files. It is what tells "still in your folder but no
 * longer published" from "no longer produced".
 */
source?: string | null; 
/**
 * The site setting that stopped this generated file, when one did. Such
 * a removal is `AuthorRemoved`: the owner's act, like deleting a source.
 */
withdrawn_by?: WithdrawnBy | null }

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

export type RenameTemplateArgs = { id: string; newName: string }

export type ResolveAssetArgs = { target: string; fromFile: VaultPath }

export type ResolveReferencesArgs = { targets: RefTarget[]; fromFile: VaultPath }

export type ResolvedAsset = { 
/**
 * The asset's file: root-relative from moss-core, absolute once a desktop
 * host's adapter has joined it to the site root (and filled in its size
 * and dimensions).
 */
absolute_path: string; request_url: string; mime_type: string; width: number | null; height: number | null; size_bytes: number; 
/**
 * How the path was resolved: literal, bare-fuzzy, separator-fallback, or
 * case-mismatch. Carried from the shared engine.
 * 
 * Read by the editor off the unified reference envelope
 * (`EditorReferenceResolution`), which marks a separator-fallback or
 * case-mismatch path as a problem and offers the file's real path.
 * The single-asset lookup (`resolve_asset`) returns it too.
 */
provenance: AssetProvenance; 
/**
 * The ladder widths the current build holds for this image and can still
 * vouch for, narrowest first: each is what `request_url?w=<width>` serves.
 * Empty when there is no fresh rung or the host has no build; the original
 * is never listed.
 */
rung_widths: number[]; 
/**
 * The ladder stopped short of this image: a rung is built, and the source
 * is no wider than the ladder's widest step, so no rung reaches its full
 * width. The editor may then offer the original as its widest `srcset`
 * candidate. A camera-sized original never qualifies.
 */
offers_original: boolean }

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

/**
 * String-shape classification of the opened root, wire form. Mirrors
 * [`crate::nested_roots::RootClass`]; the screen renders it, it never
 * re-derives it.
 */
export type RootClass = { kind: "normal" } | { kind: "cloud_provider_root"; provider: string } | { kind: "home_dir" } | { kind: "system_special"; which: string } | { kind: "filesystem_root" }

export type SaveEditorContentArgs = { filePath: VaultPath; frontmatter: JsonValue; body: string; expectedHash: string; keepDisplaced?: boolean }

/**
 * What an editor save did. A failure to write is an `Err`; this is the
 * outcome of a save that did not fail.
 */
export type SaveResult = 
/**
 * Written. `hash` is the [`crate::page::content_hash`] of the bytes now on disk, the
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
 * Where an advisory lives — which axis of the system it's about. Lets the
 * frontend group/branch advisories by their domain (a missing file vs. a
 * missing tool vs. an account problem) without parsing free text.
 */
export type Scope = 
/**
 * A specific content/asset file (carries `item: Some(site_relative_path)`).
 */
"File" | 
/**
 * The project/site configuration.
 */
"Config" | 
/**
 * The local machine/environment (e.g. a missing tool like FFmpeg).
 */
"Environment" | 
/**
 * A remote target (deploy host, DNS, etc.).
 */
"Remote" | 
/**
 * The user's account/subscription.
 */
"Account"

export type SetFolderDerivationArgs = { 
/**
 * Site-relative, as [`FolderDerivation::folder`] names it.
 */
folder: string; choice: FolderDerivationChoice }

export type SetImageWidthArgs = { imageMd: string; 
/**
 * `None`, or a token the syntax does not know, removes the width.
 */
width: string | null }

export type SetTemplateFolderArgs = { id: string; 
/**
 * `None` clears it.
 */
folder: string | null }

/**
 * One settled asset: its output path plus the `asset_type` the iframe-bridge
 * swap branches on ("image" | "video" | "thumbnail"). The type is
 * authoritative — derived in `crate::build::diff_settled_assets` from the
 * manifest bucket the path lives in, NOT guessed from the extension on the
 * frontend. Mirrors the `AssetReady` wire contract (`path` + `asset_type`) so
 * the frontend relays a `SettledAsset` straight into the same swap handler.
 */
export type SettledAsset = { path: string; asset_type: string }

/**
 * How serious an advisory is — drives the surface's tier (a bare dot vs. an
 * actionable banner vs. a blocking stop).
 */
export type Severity = 
/**
 * The build shipped, but this thing was not done optimally (e.g. a video
 * published full-size). Non-blocking, informational.
 */
"ShippedDegraded" | 
/**
 * The user should do something to fully resolve this (e.g. install a
 * tool, re-check DNS), but the build still produced output.
 */
"NeedsAction" | 
/**
 * The operation could not proceed — nothing shipped for this item.
 */
"Blocking"

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
 * The site's languages: the default language, and the language trees (folders
 * named by a language code) beside it.
 */
export type SiteLanguages = { default_lang: string; 
/**
 * Folder names of the non-default language trees, e.g. `["zh-hans"]`.
 */
trees: string[] }

/**
 * The generation at `current` was just swapped — the bytes a preview loads
 * from now on are this build's.
 * 
 * This is the ONLY event that means "the change is on the page". Note the
 * ordering it exists to expose: `BuildComplete` is emitted from
 * `await_completion`, which returns the sealed manifest BEFORE the detached
 * seal task promotes it. So a UI that sealed its progress on `BuildComplete`
 * was telling the user their change was live while `current` still pointed at
 * the previous generation.
 * 
 * Not a `PipelineEvent`: it is emitted from `advertise_sealed`, past the end
 * of the pipeline, and it reports a filesystem fact rather than progress.
 */
export type SitePromoted = { 
/**
 * The generation now serving at `.moss/build.nosync/current`.
 */
generation_id: string }

/**
 * A file the plugin looked at and declined.
 */
export type SkippedDerivedFile = { file: string; reason: string; message: string }

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
 * Terminal result of an actual structural read; consumers retry their full
 * operation for the current candidate instead of accepting cached entries.
 */
export type SourceStructureOutcome = "Ready" | "Failed"

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
 * Why a starter download failed, as far as the person can act on it.
 */
export type StarterFailure = 
/**
 * The registry could not be reached.
 */
"connection" | 
/**
 * Not enough free disk space.
 */
"disk" | 
/**
 * The bytes did not match the published hash.
 */
"verify" | "other"

/**
 * One download's progress in bytes, as `f64` for the reason [`StarterState`]
 * gives.
 */
export type StarterProgress = { done: number; total: number }

/**
 * What a starter is doing. One owner app-side holds these; nothing else
 * decides whether a starter's preview is there.
 * 
 * Byte counts and the retry time are `f64`: specta renders 64-bit integers
 * as JS strings, and the frontend does arithmetic on these.
 */
export type StarterState = 
/**
 * Nothing fetched: the card and the frame show the bundled poster.
 */
{ kind: "poster" } | 
/**
 * A download is running. `done == total` while the archive unpacks.
 */
{ kind: "downloading"; done: number; total: number } | 
/**
 * The preview is unpacked and can be served.
 */
{ kind: "ready" } | 
/**
 * The last attempt failed. `retry_after` (Unix milliseconds) is when a
 * fresh show of interest may try again on its own; `None` means only an
 * explicit retry does.
 */
{ kind: "failed"; reason: StarterFailure; retry_after: number | null } | 
/**
 * The starter needs a newer moss than this one.
 */
{ kind: "unavailable"; needs: string }

/**
 * One starter's whole state in the desktop app's starter picker, emitted as
 * `MossEvent::StarterState` on every change. `rev` rises with every emit
 * across all starters, so a webview that also asked for every row on mount
 * keeps whichever copy of a row is newer and needs no replay.
 */
export type StarterStateRow = { id: string; rev: number; state: StarterState; 
/**
 * "Use this starter" is copying this starter into a folder: its source
 * download's progress, `done == total` while it unpacks. Kept apart from
 * `state`, which goes on saying what the preview is doing.
 */
applying: StarterProgress | null }

/**
 * Amendment 2's guess, wire form: which side the decision layer believes is
 * the real site. `Nested` names the candidate by absolute path (the wire has
 * no index vocabulary — the screen keys cards by path).
 */
export type SuggestedSite = { kind: "outer" } | { kind: "nested"; abs_path: string }

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
 * A target's kind, as the dropdown paints it (`.cm-completionIcon-<kind>`).
 */
export type TargetKind = "page" | "asset" | "folder" | "generated" | "heading"

/**
 * Operation kind. **Closed enum, no `Custom` variant** — internal moss
 * code picks one explicitly; plugin tasks derive `TaskKind` from
 * `PluginHook` via `From<PluginHook>`. Adding a variant is a deliberate
 * platform decision, not a free-for-all per the WordPress
 * `admin_notices` failure mode.
 */
export type TaskKind = "save" | "validate" | "lint" | "format" | "resolve" | "build" | "rebuild" | "asset_transform" | 
/**
 * Content brought in from outside by moss itself (a URL scrape).
 */
"import" | "publish" | "deploy" | "syndicate" | "process"

/**
 * Where a `PanelTask` surfaces in the UI. Combined with `WindowId`, it
 * keys the `TaskRegistry` — multi-window safety is built in from day one.
 */
export type TaskScope = 
/**
 * Action panel (left webview): editor, file tree, save badges.
 */
"action_panel" | 
/**
 * Preview (main webview): build, deploy, publish progress.
 */
"preview" | 
/**
 * Workspace-wide background work: imports, syndications, scheduled jobs.
 */
"workspace"

/**
 * Lifecycle state of a `PanelTask`. The state machine is:
 * 
 * ```text
 * spawn → Running → Awaiting → Running → Succeeded
 * │           │         │     ↘ Failed
 * │           │         ↘ Cancelled
 * ↘ Succeeded / Failed / Cancelled
 * ```
 * 
 * `Awaiting` transitions back to `Running` on the next non-terminal
 * progress event: there is no explicit `resumed()` method. Terminal
 * states (Succeeded, Failed, Cancelled) are final.
 */
export type TaskState = { state: "running"; fraction: number | null; message: string | null } | 
/**
 * Paused awaiting an out-of-band user action (sign in, click email
 * link, accept device pairing). Distinct from Running because the
 * system is idle and the user has work to do elsewhere.
 * 
 * `action` (Step 3 R3, typed) is the recovery affordance the Awaiting
 * renderer paints next to the directive. `Action::None` is a bare
 * cancel; `InApp`/`Command`/`Link` carry a labelled recovery control.
 */
{ state: "awaiting"; directive: string; action: Action } | 
/**
 * Terminal: success. `receipt` is the human-readable result ("Saved",
 * "Imported 12 files"); `advisories` carry done-with-advisories
 * (invariant #2). Empty `advisories` = non-surfacing success (#6).
 */
{ state: "succeeded"; receipt: string | null; advisories?: Advisory[] } | 
/**
 * Terminal: failure. `advisory` is the root cause (invariant #3) — it
 * carries everything the renderer and toast subscriber need: `what` (the
 * human message, formerly `error`), `severity` (a `Blocking` advisory is
 * the unrecoverable case that fires the toast — formerly `recoverable:
 * false`), `scope`, and the recovery `action`. There is no separate
 * `error`/`recoverable` field: every failure IS an advisory (design §9).
 */
{ state: "failed"; advisory: Advisory } | 
/**
 * Terminal: explicit user cancellation.
 */
{ state: "cancelled" }

/**
 * Which renderer subscribes to this task, of the four UI
 * surfaces. `Receipt` was merged into `Inline` per round-3 review —
 * the behavior split is the per-task `fade_after` field, not a separate
 * tone variant.
 */
export type TaskTone = 
/**
 * Hairline at the panel/preview edge — high-frequency, no text in
 * chrome (tooltip on hover reveals numerals).
 */
"ambient" | 
/**
 * Status anchored adjacent to the affected element. Both terminal
 * receipts (`fade_after: Some(30s)`) and persistent live status
 * (`fade_after: None`) use this tone.
 */
"inline" | 
/**
 * Titlebar status thread (the `moss-titlebar-plugin-section` slot).
 */
"narrated" | 
/**
 * Titlebar status thread + hollow-ring pulse + escape affordance.
 * Only for out-of-band waits (NOT when the user's action surface
 * is rendered in the panel below them).
 */
"awaiting"

export type TemplateIdArgs = { id: string }

/**
 * What a saved template produces when instantiated. Decided at capture time
 * from the tree's own home-file election (a folder's home file captures as
 * `Folder`), so the template store and the tree never disagree about it.
 * Serialized as `page` / `folder` in the store and across the Tauri seam.
 */
export type TemplateKind = 
/**
 * A single new file.
 */
"page" | 
/**
 * A new folder with a seeded, self-named home file.
 */
"folder"

/**
 * A saved template, as listed for the "New from Template" menu.
 */
export type TemplateMeta = { id: string; name: string; kind: TemplateKind; created_at: string; 
/**
 * The folder its pages belong in (`work/`), when the template names one.
 */
folder: string | null; 
/**
 * The languages it has a file for besides the default, e.g. `zh-hans`.
 */
langs: string[] }

/**
 * The one question the guard puts on the wire.
 */
export type ThresholdPrompt = { 
/**
 * The guard pass's generation token. The screen's answer (and its ack)
 * must quote it, so a click on an older prompt can never land in a
 * newer prompt's slot.
 */
token: number; kind: ThresholdPromptKind; report: NestedRootsReport; 
/**
 * Present only for 'owned' prompts with a confident guess; the screen
 * makes the suggested card the focused primary. A guess is a default,
 * never a silent decision.
 */
suggestion: SuggestedSite | null }

/**
 * Which question the screen asks — see `seam.ts` for the two cases'
 * semantics ('unowned' = rows 4/5, 'owned' = the case-(d) double vault).
 */
export type ThresholdPromptKind = "unowned" | "owned"

/**
 * Toast action button (for toasts that surface a follow-up command).
 */
export type ToastAction = { label: string; 
/**
 * What the button does. `open-url:<url>` opens that link; anything else
 * is a key the host app resolves against its own fixed list of actions,
 * so a toast can only ask for what the host already knows how to do.
 */
event: string; 
/**
 * How much weight the button carries. None (the default) is a plain
 * follow-up link.
 */
variant?: ToastActionVariant | null }

/**
 * Visual weight of a [`ToastAction`], for a toast that offers a choice: the
 * one thing it asks the user to do, and what else they could do instead.
 */
export type ToastActionVariant = "primary" | "secondary"

/**
 * Colored status dot for [`ToastPayload::dot`]. Yellow = pending/in-flight,
 * green = confirmed — the pair a two-phase toast flow cycles through.
 */
export type ToastDot = "yellow" | "green"

/**
 * A link rendered INSIDE the toast's message, on the message's own line —
 * for a toast whose sentence ends in a thing you can open ("Published to
 * sample-site.mosspub.com"). An action button would put the same words on a
 * second row and say them twice.
 */
export type ToastInlineLink = { 
/**
 * The link's visible text — usually the thing itself (a host, a filename).
 */
label: string; 
/**
 * Opened via the host runtime (system browser, or Tor for an onion URL).
 */
url: string }

/**
 * Payload for `MossEvent::ShowToast` (and `ShowToastUpdate`, with non-null `id`).
 * 
 * Step 3 §12b R1: **a toast is an Advisory rendered ephemerally** — there is no
 * private toast severity enum. The visual tier derives from the shared
 * [`Severity`](crate::advisory::Severity) value (`ShippedDegraded`→info,
 * `NeedsAction`→warning, `Blocking`→error). A pure confirmation ("Sent",
 * "Site is live!") is NOT an advisory — it is the design's `Ack`; set
 * `ack: true` and the frontend renders it success-styled via `showAck`.
 * 
 * Fields mirror `ToastAdvisory` + the options bag in
 * the desktop app's toast manager. Per architecture decision 9:
 * `Option<T>` serializes as
 * `T | null` required, not `T?` optional. The TS listener accepts `null` the
 * same as `undefined`.
 */
export type ToastPayload = { 
/**
 * What happened — the toast's message text (the advisory's `what`).
 */
what: string; 
/**
 * Recovery effort; drives the icon/class. Ignored when `ack` is true.
 */
severity: Severity; 
/**
 * True for a pure confirmation (`Ack`) — rendered success-styled, ignoring
 * `severity`. False (default) for an advisory.
 */
ack: boolean; 
/**
 * Action buttons. None or empty for plain toasts.
 */
actions: ToastAction[] | null; 
/**
 * A link on the message's own line — see [`ToastInlineLink`].
 */
inline_link?: ToastInlineLink | null; 
/**
 * Auto-dismiss duration in ms. None for persistent.
 */
duration: number | null; 
/**
 * If true, the auto-dismiss countdown only advances while the moss window
 * is visible. A confirmation that lands while the user is in another app is
 * therefore still on screen when they come back, instead of having expired
 * unwitnessed — which is the failure `persistent` used to be set to avoid,
 * at the cost of a toast that never leaves.
 * 
 * Deliberately NOT gated on focus: moss's shell hosts sibling child
 * webviews (action panel, plugin browser), and one of them taking focus
 * blurs the shell while the toast is still fully visible to the user.
 */
await_attention?: boolean | null; 
/**
 * If true, no auto-dismiss timer (overrides `duration`).
 */
persistent: boolean | null; 
/**
 * If false, hide the X close button (default: true).
 */
dismissible: boolean | null; 
/**
 * Stable id — required for `ShowToastUpdate` and toast deduplication.
 */
id: string | null; 
/**
 * Overrides the icon with a small colored status dot — for a two-phase
 * "pending → confirmed" flow (send logs, check for updates) where the
 * generic checkmark can't distinguish "still working" from "done".
 * None (the default) keeps the existing check/triangle icon logic; a
 * one-shot `ack` toast ("Copied", "Site published") should leave this
 * unset.
 */
dot?: ToastDot | null; 
/**
 * A progress bar under the message, for a toast that reports work with a
 * measurable end (a download). `0`–`100` is a determinate fill; a
 * negative value is indeterminate, for work whose total is not known
 * yet. None (the default) draws no bar, and on a `ShowToastUpdate`
 * removes one.
 */
progress?: number | null }

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
 * winner of `crate::home::detect_home_file_in_folder_marked` (order:
 * `home: true` frontmatter marker › index stems › self-named folder note ›
 * first document alphabetically), computed per folder in
 * `build_tree_root` with the folder's REAL basename (so the project
 * root's self-named home is recognized too). At most one child per folder
 * is `true`; always `false` for directories. The frontend consumes this
 * flag (`entries.find(e => e.is_home)`) instead of re-deriving the
 * election — the core owns the home decision.
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
 * `super::policy::special_file_kind`. Always `None` for folders.
 */
special?: SpecialFileKind | null }

export type UndoRenameArgs = { undoId: string }

/**
 * The result of [`undo_applied`].
 */
export type UndoResult = { restored_files: string[]; skipped: SkippedFile[] }

/**
 * A path a caller names. Its only way to a real path is [`resolve`], so no
 * command and no host can skip the confinement.
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
 * The past-tense outcome a Job reports: "Built", "Published", "Deployed",
 * "Connected", "Sent", "Synced", "Prepared", or a plugin-supplied verb.
 * A `Verb(String)` newtype (§12b R2) — core constants below, open tail for
 * plugins — never a closed per-producer enum. moss normalizes plugin verbs
 * (R13): a plugin proposes the word, moss owns capitalization/length/glyphs.
 */
export type Verb = string

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
 * reuses [`HistoryStore::site_version_pages`] in the same body as the list.
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
 * Fine-grained video conversion progress for real-time UI updates.
 * 
 * Emitted during background video conversion to show per-video progress
 * with the fraction complete.
 * 
 * The overall_fraction formula for video `i` (0-based) of N total:
 * overall = (i + video_fraction) / N
 */
export type VideoConversionProgress = { 
/**
 * Filename of the video being converted (e.g., "clip.mov")
 */
filename: string; 
/**
 * Current video number (1-based)
 */
current_video: number; 
/**
 * Total number of videos to convert
 */
total_videos: number; 
/**
 * Conversion phase, always "encoding". Kept because the desktop app's
 * event contract carries it; the "analyzing" value it also handles was
 * the first pass of a two-pass encode that no longer exists.
 */
phase: string; 
/**
 * Progress fraction within current video (0.0..1.0)
 */
video_fraction: number; 
/**
 * Progress fraction across all videos (0.0..1.0)
 */
overall_fraction: number }

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

/**
 * One completion row, ready to render.
 */
export type WikilinkCompletion = { 
/**
 * The exact text accepting the row writes into the link.
 */
insert: string; 
/**
 * The thing's name: page title, filename, `dir/`, term text, heading.
 */
label: string; 
/**
 * The dimmer second column. The insert text when it differs from the
 * label (so the author learns the address on the spot); the heading's
 * level (`H2`) when the two coincide.
 */
detail: string | null; kind: TargetKind }

export type WikilinkCompletionsArgs = { prefix: string; fromFile: VaultPath; syntax: LinkSyntax; 
/**
 * A root-relative page: complete its headings instead of the folder's pages and files.
 */
page: string | null; allowedKinds: ExtKind[] | null }

/**
 * Window identifier for multi-window task isolation. Wraps a `String` so
 * callers can pass either a Tauri `WebviewWindow::label()` (already
 * `&str`) or a synthetic id (tests, future launcher window).
 * 
 * A newtype is preferred over a bare `String` alias because tasks key
 * their registry by `(WindowId, TaskScope)` — the type system catches
 * accidental swaps with other string identifiers (site_id, label, etc.).
 */
export type WindowId = string

/**
 * A site setting that stops a generated file on purpose.
 */
export type WithdrawnBy = 
/**
 * AI answers are asked not to use the pages, so there is no `llms.txt`.
 */
"ai_answers" | 
/**
 * No search or AI switch is set any more, so there is no `tdmrep.json`.
 */
"ai_switches" | 
/**
 * `[site].selection_tools` is off, so there is no Share and no QR code
 * for its picture.
 */
"selection_tools"

export type WriteEditorFileBinaryArgs = { relativePath: VaultPath; dataBase64: string }

export type EngineCommands = {
	scan_shortcodes: { args: ScanShortcodesArgs; out: EditorScanResult };
	editor_resolve_asset: { args: ResolveAssetArgs; out: ResolvedAsset | null };
	editor_resolve_references: { args: ResolveReferencesArgs; out: EditorReferenceResolution[] };
	set_image_width: { args: SetImageWidthArgs; out: string };
	editor_bootstrap: { args: BootstrapArgs; out: EditorBootstrap };
	parse_frontmatter: { args: FileArgs; out: ParsedFrontmatter };
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
	editor_reference_card: { args: ReferenceCardArgs; out: ReferenceCardInfo };
	wikilink_completions: { args: WikilinkCompletionsArgs; out: WikilinkCompletion[] };
	list_section_folders: { args: NoArgs; out: string[] };
	resolve_url_for_folder: { args: FolderPathArgs; out: string | null };
	resolve_expected_url_for_file: { args: FileArgs; out: string | null };
	resolve_asset_url_for_file: { args: FileArgs; out: string | null };
	get_all_pages: { args: NoArgs; out: PageInfo[] };
	read_source_text: { args: FileArgs; out: string };
	list_templates: { args: NoArgs; out: TemplateMeta[] };
	site_languages: { args: NoArgs; out: SiteLanguages };
	derivation_state: { args: NoArgs; out: FolderDerivation[] };
	create_files: { args: CreateFilesArgs; out: string[] };
	create_folder: { args: CreateFolderArgs; out: string };
	delete_entry: { args: PathArgs; out: null };
	delete_entries: { args: PathsArgs; out: null };
	duplicate_entry: { args: PathArgs; out: string };
	rename_entry_with_refs: { args: RenameArgs; out: RenameRefsSummary };
	move_entries: { args: MoveEntriesArgs; out: RenameRefsSummary };
	scan_references_for_delete: { args: PathsArgs; out: FileReferenceHit[] };
	clean_references_and_delete: { args: PathsArgs; out: null };
	undo_rename: { args: UndoRenameArgs; out: UndoResult };
	save_editor_content: { args: SaveEditorContentArgs; out: SaveResult };
	restore_version: { args: RestoreVersionArgs; out: RestoreReport };
	save_version: { args: SaveVersionArgs; out: VersionRow };
	write_editor_file_binary: { args: WriteEditorFileBinaryArgs; out: null };
	expand_derived: { args: ExpandDerivedArgs; out: ExpandedDerived };
	set_folder_derivation: { args: SetFolderDerivationArgs; out: string[] };
	create_template: { args: CreateTemplateArgs; out: string };
	rename_template: { args: RenameTemplateArgs; out: null };
	delete_template: { args: TemplateIdArgs; out: null };
	set_template_folder: { args: SetTemplateFolderArgs; out: null };
	instantiate_template: { args: InstantiateTemplateArgs; out: string };
	create_translation: { args: CreateTranslationArgs; out: string };
};

export type EngineCommand = keyof EngineCommands;

export const ENGINE_TIER = {
	scan_shortcodes: "public_read",
	editor_resolve_asset: "public_read",
	editor_resolve_references: "public_read",
	set_image_width: "public_read",
	editor_bootstrap: "session_read",
	parse_frontmatter: "session_read",
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
	editor_reference_card: "session_read",
	wikilink_completions: "session_read",
	list_section_folders: "session_read",
	resolve_url_for_folder: "session_read",
	resolve_expected_url_for_file: "session_read",
	resolve_asset_url_for_file: "session_read",
	get_all_pages: "session_read",
	read_source_text: "session_read",
	list_templates: "session_read",
	site_languages: "session_read",
	derivation_state: "session_read",
	create_files: "write",
	create_folder: "write",
	delete_entry: "write",
	delete_entries: "write",
	duplicate_entry: "write",
	rename_entry_with_refs: "write",
	move_entries: "write",
	scan_references_for_delete: "write",
	clean_references_and_delete: "write",
	undo_rename: "write",
	save_editor_content: "write",
	restore_version: "write",
	save_version: "write",
	write_editor_file_binary: "write",
	expand_derived: "write",
	set_folder_derivation: "write",
	create_template: "write",
	rename_template: "write",
	delete_template: "write",
	set_template_folder: "write",
	instantiate_template: "write",
	create_translation: "write",
} as const;

// Each command's arguments in declaration order, for a client that takes them positionally:
// an Args struct's field order is its client's argument order, so reordering fields is a breaking change.
export type EngineParams = {
	scan_shortcodes: [text: string];
	editor_resolve_asset: [target: string, fromFile: VaultPath];
	editor_resolve_references: [targets: RefTarget[], fromFile: VaultPath];
	set_image_width: [imageMd: string, width: string | null];
	editor_bootstrap: [sourcePath: VaultPath | null];
	parse_frontmatter: [filePath: VaultPath];
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
	editor_reference_card: [fromFile: VaultPath, target: string];
	wikilink_completions: [prefix: string, fromFile: VaultPath, syntax: LinkSyntax, page: string | null, allowedKinds: ExtKind[] | null];
	list_section_folders: [];
	resolve_url_for_folder: [folderPath: VaultPath];
	resolve_expected_url_for_file: [filePath: VaultPath];
	resolve_asset_url_for_file: [filePath: VaultPath];
	get_all_pages: [];
	read_source_text: [filePath: VaultPath];
	list_templates: [];
	site_languages: [];
	derivation_state: [];
	create_files: [files: NewFile[]];
	create_folder: [parentDir: string, name: string];
	delete_entry: [path: VaultPath];
	delete_entries: [paths: VaultPath[]];
	duplicate_entry: [path: VaultPath];
	rename_entry_with_refs: [oldPath: VaultPath, newPath: VaultPath];
	move_entries: [paths: VaultPath[], targetDir: VaultPath];
	scan_references_for_delete: [paths: VaultPath[]];
	clean_references_and_delete: [paths: VaultPath[]];
	undo_rename: [undoId: string];
	save_editor_content: [filePath: VaultPath, frontmatter: JsonValue, body: string, expectedHash: string, keepDisplaced?: boolean];
	restore_version: [id: string, path: string | null, mode: string];
	save_version: [label: string | null];
	write_editor_file_binary: [relativePath: VaultPath, dataBase64: string];
	expand_derived: [pagePath: VaultPath, content: string];
	set_folder_derivation: [folder: string, choice: FolderDerivationChoice];
	create_template: [sourcePath: VaultPath, name: string, isHome: boolean, translations: ([string, VaultPath])[] | null];
	rename_template: [id: string, newName: string];
	delete_template: [id: string];
	set_template_folder: [id: string, folder: string | null];
	instantiate_template: [id: string, targetDir: VaultPath, name: string];
	create_translation: [sourceAbs: VaultPath, lang: string];
};

export const ENGINE_PARAMS = {
	scan_shortcodes: ["text"],
	editor_resolve_asset: ["target", "fromFile"],
	editor_resolve_references: ["targets", "fromFile"],
	set_image_width: ["imageMd", "width"],
	editor_bootstrap: ["sourcePath"],
	parse_frontmatter: ["filePath"],
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
	editor_reference_card: ["fromFile", "target"],
	wikilink_completions: ["prefix", "fromFile", "syntax", "page", "allowedKinds"],
	list_section_folders: [],
	resolve_url_for_folder: ["folderPath"],
	resolve_expected_url_for_file: ["filePath"],
	resolve_asset_url_for_file: ["filePath"],
	get_all_pages: [],
	read_source_text: ["filePath"],
	list_templates: [],
	site_languages: [],
	derivation_state: [],
	create_files: ["files"],
	create_folder: ["parentDir", "name"],
	delete_entry: ["path"],
	delete_entries: ["paths"],
	duplicate_entry: ["path"],
	rename_entry_with_refs: ["oldPath", "newPath"],
	move_entries: ["paths", "targetDir"],
	scan_references_for_delete: ["paths"],
	clean_references_and_delete: ["paths"],
	undo_rename: ["undoId"],
	save_editor_content: ["filePath", "frontmatter", "body", "expectedHash", "keepDisplaced"],
	restore_version: ["id", "path", "mode"],
	save_version: ["label"],
	write_editor_file_binary: ["relativePath", "dataBase64"],
	expand_derived: ["pagePath", "content"],
	set_folder_derivation: ["folder", "choice"],
	create_template: ["sourcePath", "name", "isHome", "translations"],
	rename_template: ["id", "newName"],
	delete_template: ["id"],
	set_template_folder: ["id", "folder"],
	instantiate_template: ["id", "targetDir", "name"],
	create_translation: ["sourceAbs", "lang"],
} as const;
