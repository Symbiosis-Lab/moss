// Editor webview entry point — coordinates the Properties form and CM6 editor.
// Listens for file load events, manages auto-save with a 2-second debounce, and
// reconciles external file changes (git, external editor) in place without
// tearing down the editor view.

// Install the console + uncaught-error bridge FIRST so anything that errors
// during action-panel init still lands in moss.log.
import { installLogBridge } from "../lib/install-log-bridge";
import type { ParsedFrontmatter } from '@symbiosis-lab/moss-ui';
import { engine } from '../engine';
installLogBridge('action-panel');

// [diag] Energy + layout-shift diagnostics → moss.log. Dev-default-on; set
// localStorage['moss.diag']='0' to silence. See @symbiosis-lab/moss-ui/ui/lib/diag.ts.
import { startDiagSummary } from "../lib/diag";
import { initLayoutDiag } from "../lib/diag-editor";
startDiagSummary('action-panel');
initLayoutDiag();

import { info as logInfo, error as logError, debug, warn } from "../log";
import { installHostToastRuntime } from "../utils/open-external";
import type { ContentSchema, JsonValue } from '@symbiosis-lab/moss-ui';
import { dispatchEditorFileChanged } from './editor-file-changed';
import { PendingRenames } from './session/pending-renames';
import { EditorFileEvents } from './editor-file-events';
import { ChipBar, type SlotRole } from './chip-bar';
import { NO_PAGE_FACTS } from './field-applies';
import { toNameList } from "../utils/name-list";
import { FrontmatterStore } from './frontmatter-store';
import { CmEditor } from './cm-editor';
import { DocumentHistory, deepEqual, type HistorySnapshot } from './document-history';
import { isClean, mergeExternal, outsideWins, type DocumentText } from './external-merge';
import { createKeptOutsideEdit } from './session/kept-outside-edit';
import { DocStateCluster } from './doc-state-cluster';
import { installEditorFocusProbe } from "../lib/diag-editor";
import { routeHistoryKey, computeHistoryInputs } from './history-routing';
import { matchesKeySpec, isMacPlatform, UNDO_KEY, REDO_KEY } from "../utils/kbd-label";
import { CmFindController } from './cm-find';
import { isMarkdownFile, detectFileCategory } from './file-types';
import { showFileViewer, destroyFileViewer, fileViewerIsOpen } from './file-viewer';
import { FileTree, createOneFile } from './tree';
import { EntryRegistry } from './entry-registry';
import { PreviewFollower } from './preview-follower';
import { setPreviewNote } from './preview-note';
import { onMossEvent } from "../utils/moss-events";
import { ChipDiagnostics } from './chip-diagnostics';
import { installFullscreenBodyClass, applyOwnTitlebarClass, wireMainShellCaptionButtons, applyCaptionOwnerClass } from "../utils/fullscreen-class";
import type { EntryId } from './entry-registry';
import { mayOpenSpecialFile } from './session/special-file';
import { askAboutSpecialFile } from './session/special-file-modal';
import { toAbsolute, toRelative, childPath, baseName } from './path-domain';
import { wireLayoutButton } from './editor-layout-btn';
import { initEdgeRestore } from './edge-restore';
import { getInternalToggleState } from './internal-toggle';
import { initTooltips } from "../components/tooltip";
import { initEditHistoryMenu } from "../utils/edit-history";
import { initLocale } from "../i18n/locale-detect";
import { followTheme } from "../shared/theme";
import { getLocale, t } from "../i18n/index";
import { applyStaticStrings } from "../i18n/apply-static-strings";
import { installLocaleReactivity } from "../i18n/live-locale";
import { mountInlineStatus } from './inline-status';
import { wireEditorHeading, type HeadingController } from './editor-heading';
import { getHeadingState, readDiagnostics, type EditorDiagnostic } from './moss-debug';
import { forceLinting } from '@codemirror/lint';
import { routeHeadingCommit } from './heading-routing';
import { CreateFlow, revealAndOpenNewEntry } from './create-flow';
import { host, type Composer, type Unlisten } from "../host";
import { createVersionsMode } from './versions-mode';
import { createOsDragInController, afterVaultImport, pickMediaForBar, attachmentDirFor, reportImportSkips, importIntoVault } from './os-drag-in';
import type { OsDragInController } from './os-drag-in';
import { acceptOnDrop } from './asset-registry.generated';
import { installControlCharGuard } from "../shared/ui/control-char-guard";
import { setOsDropCaret, snapToBlockBoundary } from './cm-os-drop-caret';
import { insertEntryEmbedsAtCoords, embedFor, type DestinationFor } from './cm-drop';
import { mountNoContentPane, SOURCE_PENDING_EVENT, type SourcePendingPayload } from './session/no-content-pane';
import { removeToastById, showToast } from "../shared/ui/toast/toast-manager";
import { createOutsideFile } from './session/outside-file';
import { EDITOR_GOTO_LINE, EDITOR_GOTO_RULE, EDITOR_OPEN_FIELD, EDITOR_OPEN_PUBLISH_PREFLIGHT } from './session/editor-position';
import { runAction } from "../shared/ui/run-action";
import { mountFolderMode, unmountFolderMode } from './folder-mode/folder-mode';
import { makeCardClickHandler, dispatchCard } from './folder-mode/folder-mode-handlers';
import { decideEditorSurface, shouldLoadContentPage, editorQueryFromNode, resolveNodeForEdit, folderHomeTakeover, editorNodeFromQuery, readEditorNode, type EditorNodeWire, type EditorSurface, type Takeover } from './session/node-class';
import { mountPaneState, unmountPaneState } from './pane-state';
import { dismissNavSkeleton, dismissEditorSkeleton } from './editor-skeleton';
import {
  findNearestHeading,
  headingToAnchor,
  getFocusLine,
  scrollEditorToLine,
  scrollEditorToTop,
  scrollEditorToBottom,
  computeContentOffset,
  gotoLine,
  findRuleLine,
  selectTextNearLine,
} from './cm-scroll-sync';
import { preflightHints, revealLines, revealPublishPreflight, shouldPreserveEditorPosition, type PublishPreflightOpen } from './cm-reveal-flash';
import { readProblems, setEvidenceProblems } from './cm-problems';
import { loadEditorFontSize, stepEditorFontSize } from "../shared/editor-font-size";
import { createGenerationToken } from "../shared/utils/generation-token";
import { initEditorFontScale, setEditorFontSize } from "../utils/editor-font-scale";
import { PublishPreflightStore, readResult, primePublishPreflight } from "../shared/preflight-store";
import { mountToasts } from "../shared/ui/toast/app-toast-render";
import type { PublishPreflightProjection } from '@symbiosis-lab/moss-ui';
import { host as uiHost } from "../host";

// ── Editor Font Scale ────────────────────────────────────────────────
//
// ⌘+/−/0 here and the aA stepper (Settings ▸ Editor, shell webview) are steps
// of the SAME persisted value — see shared/editor-font-size.ts. Each change
// persists + applies `--moss-editor-font-scale` locally, then broadcasts so
// the other webview follows live; the listener applies without re-emitting.

function adjustEditorScale(delta: number): void {
  setEditorFontSize(stepEditorFontSize(loadEditorFontSize(), delta), document.documentElement);
}

// AbortController lets us remove all keydown listeners on beforeunload
const scaleAbort = new AbortController();

// Restore saved font size immediately (before CM6 mounts) + follow the shell.
initEditorFontScale(document.documentElement);

// Cmd+=/- to adjust font scale, Cmd+0 to reset
document.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && (e.key === '=' || e.key === '+')) {
    e.preventDefault();
    adjustEditorScale(1);
  }
  if ((e.metaKey || e.ctrlKey) && e.key === '-') {
    e.preventDefault();
    adjustEditorScale(-1);
  }
  if ((e.metaKey || e.ctrlKey) && e.key === '0') {
    e.preventDefault();
    adjustEditorScale(0);
  }

  // ── Settings ──────────────────────────────────────────────────────────
  // In full-width the shell webview is hidden and gets no keys, so without
  // this ⌘, does nothing at all. Relay what the ⋯ "Settings…" row emits.
  if ((e.metaKey || e.ctrlKey) && e.key === ',') {
    e.preventDefault();
    uiHost.bus.emit('editor:open-settings', {});
  }

  // ── Find ──────────────────────────────────────────────────────────────
  if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
    const view = cmEditor?.getView();
    const editorCm = document.getElementById('moss-editor-cm');
    if (view && editorCm) {
      e.preventDefault();
      findController.open(view, editorCm);
    }
  }

  // ── Undo / Redo ───────────────────────────────────────────────────────
  // The keymap in cm-editor.ts answers for the BODY ONLY: CodeMirror attaches
  // its handlers to `contentDOM`, so a keydown with focus on a chip, on the
  // heading, or on nothing at all never reaches it. On macOS the Edit menu
  // catches those (it claims the accelerator before the webview sees it and
  // broadcasts `menu:edit-undo`), but `app_menu.rs` is macOS-only — so on Linux
  // and Windows Ctrl+Z outside the body did nothing at all, and in the heading
  // it did something worse: the browser's native contenteditable undo reverted
  // visible text that the store never heard about, because the heading commits
  // on blur. That is verbatim the divergence history-routing.ts exists to
  // prevent; the route simply never ran on those platforms.
  //
  // `defaultPrevented` is the no-double-fire guard: a CM6 keymap handler that
  // returns true preventDefaults the event, and the event still bubbles here.
  if (!e.defaultPrevented) {
    // Ctrl+Y is the Windows/Linux redo convention and is offered there as a
    // second spelling. NOT on macOS, where ⌘Y is nobody's redo and the Edit
    // menu — the platform's own statement of what the shortcuts are — does not
    // list it.
    const dir = matchesKeySpec(UNDO_KEY, e)
      ? 'undo'
      : matchesKeySpec(REDO_KEY, e) || (!isMacPlatform() && matchesKeySpec('Mod-y', e))
        ? 'redo'
        : null;
    if (dir) {
      // routeHistory returning false means "the focused field's own history
      // owns this" — leave the event alone and let the browser serve it.
      if (routeHistory(dir)) e.preventDefault();
    }
  }
}, { signal: scaleAbort.signal });

// Guard every plain input/textarea in this (child) webview against the macOS
// multiwebview arrow-key control-char insertion bug. See control-char-guard.ts.
installControlCharGuard();

// ── State ──────────────────────────────────────────────────────────────

function currentEntryId(): EntryId | null {
  return registry?.activeEntryId ?? null;
}
/** Live registry-derived active path; never cache across an await. */
function currentFilePath(): string | null {
  const id = currentEntryId();
  if (id === null) return null;
  return registry?.getById(id)?.path ?? null;
}

function activeDocumentPath(): string | null { return currentFilePath(); }

/** Tell the chip bar where the open page sits and what the build picks for its fields. */
async function refreshPageFacts(): Promise<void> {
  const bar = chipBar, path = activeDocumentPath();
  if (!bar || !path || !projectRoot) return;
  // A failure offers every field, with no hints.
  const result = await engine.pageFacts(toAbsolute(path, projectRoot))
    .catch((e: unknown) => { console.warn('[editor] pageFacts failed:', e); return null; });
  if (bar === chipBar) bar.setPageFacts(result?.status === 'ok' ? result.data : NO_PAGE_FACTS);
}

/**
 * A project file's standard-form destination from the open page: the engine's
 * own path and encoding (`asset_ref_for_path` → `asset_ref_in`, the text its
 * link completion writes). Every insert that calls this has already landed
 * the file, which the command requires.
 */
const destinationFor: DestinationFor = async (target) => {
  const page = activeDocumentPath();
  if (!page || !projectRoot) return null;
  return (await host.assetRefForPath?.(toAbsolute(target, projectRoot), page))?.insert ?? null;
};
let registry: EntryRegistry | null = null;
let projectRoot: string | null = null;
let pendingPreflightOpen: (PublishPreflightOpen & { path: string }) | null = null;
let publishPreflightProjection: PublishPreflightProjection | null = null;
let publishPreflightHintRequest = 0;
let publishPreflightRoot: string | null = null;
function invalidatePublishPreflightPresentation(clearEvidence = false): void {
  ++publishPreflightHintRequest;
  pendingPreflightOpen = null;
  replacePublishPreflightHints([]);
  if (clearEvidence) {
    publishPreflightProjection = null;
    fileTree?.setPublishPreflight?.(null);
  }
}
function preflightSourceIsCurrent(): boolean { return saveState === 'clean' && !frontmatterStore?.hasPendingWrite(); }
function replacePublishPreflightHints(hints: Awaited<ReturnType<typeof preflightHints>>): void {
  const view = cmEditor?.getView();
  if (typeof view?.dispatch !== 'function') return;
  view.dispatch({ effects: setEvidenceProblems.of(hints) });
}

async function renderPublishPreflightHints(): Promise<void> {
  const request = ++publishPreflightHintRequest;
  const editor = cmEditor;
  const path = currentFilePath();
  if (!preflightSourceIsCurrent() || !editor || !path || !publishPreflightProjection?.missing_references.some((occurrence) => occurrence.source_path === path)) {
    replacePublishPreflightHints([]);
    return;
  }

  try {
    const source = await engine.readSourceText(path);
    if (request !== publishPreflightHintRequest || editor !== cmEditor || path !== currentFilePath()) return;
    const hints = source.status === 'ok'
      ? await preflightHints(publishPreflightProjection, path, source.data, editor.getContent())
      : [];
    if (request === publishPreflightHintRequest && editor === cmEditor && path === currentFilePath()) {
      replacePublishPreflightHints(hints);
    }
  } catch {
    if (request === publishPreflightHintRequest) replacePublishPreflightHints([]);
  }
}

async function revealPendingPreflight(): Promise<void> {
  const request = pendingPreflightOpen;
  const path = currentFilePath();
  if (!request || !path || path !== request.path || !cmEditor) return;
  pendingPreflightOpen = null;
  if (!preflightSourceIsCurrent()) {
    replacePublishPreflightHints([]);
    showToast({ what: 'This file changed since moss checked it. Build again to update problem locations.', severity: 'ShippedDegraded' });
    return;
  }
  try {
    const source = await engine.readSourceText(path);
    if (source.status !== 'ok' || !cmEditor || path !== currentFilePath()) return;
    if (await revealPublishPreflight(cmEditor.getView(), source.data, cmEditor.getContent(), request) === 'mismatch') {
      showToast({ what: 'This file changed since moss checked it. Build again to update problem locations.', severity: 'ShippedDegraded' });
    }
  } catch {
    // The document is already open; a failed provenance read must not block it.
  }
}
const publishPreflightStore = new PublishPreflightStore(readResult((folder) => engine.getPublishPreflight(folder)));
let rootName: string | null = null;
let composerHandle: Composer | null = null;
let lastSurface: Exclude<EditorSurface, { kind: 'content' }> | null = null;
const versionsMode = createVersionsMode({
  container: () => document.getElementById('moss-editor-cm'),
  projectRoot: () => projectRoot,
  rootName: () => rootName,
  currentFilePath,
  lastSurface: () => lastSurface,
  flushPendingSave: () => flushPendingSave(),
  clearSlot: (cm) => { teardownFileMode(); unmountFolderMode(); unmountPaneState(cm); },
  hideChrome: () => hideEditorChromeForSurface(),
  showChrome: () => showEditorChromeForSurface(),
  setTitleBar: (content) => fileTree?.setTitleBar(content ?? null),
  loadFile: (path) => loadFile(path),
  applyEditorSurface: (surface) => applyEditorSurface(surface),
  resolveRootSurface: () => resolveAndApplyRootSurface(),
  logInfo: (m) => void logInfo(m),
});
let frontmatterStore: FrontmatterStore | null = null;
let chipBar: ChipBar | null = null;
let cmEditor: CmEditor | null = null;
/**
 * One undo stack for the whole document — body text and frontmatter chips in
 * the order the edits happened. It stores markers and delegates the body half
 * to CodeMirror rather than keeping a second copy of the text, so a native undo
 * (the macOS Edit menu, a trackpad gesture, Ctrl+Z on Linux) moves the same
 * pointer the buttons do. See document-history.ts.
 */
const documentHistory = new DocumentHistory({
  view: () => cmEditor?.getView() ?? null,
  generation: () => docGeneration.current(),
  readField: (key) => frontmatterStore?.getValues()[key],
  writeField: (key, value) => {
    if (!chipBar) return;
    // undefined REMOVES — undoing an add has to delete the key, not write
    // undefined over it, which the store's equality guard would drop.
    chipBar.setFieldValue(key, value);
  },
  surface: (key, removed, restored) => {
    // A chip that no longer exists cannot be promoted or flashed, so a removal
    // — the case that most needs feedback — would otherwise be invisible.
    if (removed) chipBar?.flashAddButton(key);
    // A restored field is pulled to the front, not merely washed: it was just
    // re-created, so it can be sitting inside the overflow `+N` where the wash
    // would play on something invisible.
    else chipBar?.revealField(key, { promote: restored });
  },
});
/**
 * Does the chip bar hold an edit its popover has not committed yet?
 *
 * ONE function, deliberately, because two consumers ask it for reasons that
 * must not drift: the router (which flushes rather than walking the log) and
 * the cluster's buttons (whose mousedown must not let `document` dismiss that
 * popover out from under the click). Written twice, they can disagree — and the
 * disagreement is silent, because each side is defensible alone.
 */
const chipOverlayPending = (): boolean => chipBar?.hasOpenEditOverlay() ?? false;

const docStateCluster = new DocStateCluster({
  // Through the ROUTER, not straight to the log. A button that walked the log
  // unconditionally would disagree with the key that carries its own tooltip:
  // with a chip popover open the key flushes the visit and undoes nothing,
  // while a direct call would undo that just-flushed edit. Same gesture, two
  // outcomes, which is the half of "whether from UI or hotkey" that is easy to
  // miss because each side is defensible alone.
  undo: () => applyHistory('undo'),
  redo: () => applyHistory('redo'),
  canUndo: () => documentHistory.canUndo(),
  canRedo: () => documentHistory.canRedo(),
  // Literally the router's own predicate, so the button's mousedown suppresses
  // exactly the dismissal that would turn one click into flush-then-undo, and
  // leaves every other document-level listener its event.
  holdsUncommittedOverlay: chipOverlayPending,
});

/**
 * Run one history step and repaint everything that reports its state.
 *
 * Every path in — the two buttons, the Mod-z keymap, the macOS Edit menu —
 * lands here, so there is one answer to "what does undo do" rather than three
 * that drift.
 */
/**
 * Which DOCUMENT the log's entries belong to — deliberately not `loadGeneration`.
 *
 * That counter identifies the in-flight `loadFile()` call, and it is bumped on
 * entry, before the read. A read that FAILS returns 'failed' with the previous
 * document still fully mounted and nothing destroyed, so borrowing that counter
 * expired every entry of a document the user is still looking at: click a file
 * that cannot be read, dismiss the toast, press ⌘Z, and the whole history is
 * silently gone. This one moves only where the surface is actually replaced,
 * which is exactly where the log is emptied.
 */
const docGeneration = createGenerationToken();
const headingGeneration = createGenerationToken();

/** Documents left for another, by entry, so going back restores their undo history. */
/**
 * Documents the author moved away from. A clean one is resumed only when the
 * disk still holds its text. An `unsaved` one (no save could write it: the
 * file left, or another writer's text could not be kept) is resumed whatever the disk
 * holds, on its own base, and reconciled again: switching files must not
 * drop text that is nowhere else. Only clean entries are evicted.
 */
const parked = new Map<EntryId, { cm: unknown; log: HistorySnapshot; fm: Record<string, unknown>; base: DocumentBase | null; unsaved: boolean }>();
const PARKED_MAX = 20;
function parkActiveDocument(): void {
  const id = currentEntryId();
  if (id === null || !cmEditor) return;
  parked.delete(id);
  const unsaved = unsavedAwareSaveState() === 'dirty';
  parked.set(id, { cm: cmEditor.toJSON(), log: documentHistory.snapshot(), fm: frontmatterStore?.getValues() ?? {}, base: documentBase, unsaved });
  if (parked.size > PARKED_MAX) {
    const oldestClean = [...parked].find(([, doc]) => !doc.unsaved)?.[0];
    if (oldestClean !== undefined) parked.delete(oldestClean);
  }
}

/** Empty the log and retire the outgoing document's entries, in that order. */
function resetDocumentHistory(): void {
  docGeneration.bump();
  documentHistory.reset();
}

function runHistory(direction: 'undo' | 'redo'): boolean {
  const moved = direction === 'undo' ? documentHistory.undo() : documentHistory.redo();
  docStateCluster.sync();
  return moved;
}

/**
 * Decide who owns ⌘Z, and act on the decision.
 *
 * The decision itself is a pure function in history-routing.ts, tested as a
 * table; this half only reads the DOM for it and carries out the verdict.
 * Returns false to mean "let the focused field's own history answer", which the
 * caller does with execCommand.
 */
function routeHistory(direction: 'undo' | 'redo'): boolean {
  // ⌘Z from the Edit menu reaches here without a keydown in the page, and the
  // caret it moves is the user's work all the same.
  leadFromEditor();
  // Every surface that can hold an edit the log has not seen, and how to make
  // it seen. Adding one here is the whole cost of adding a surface — the router
  // itself never learns another name.
  //
  // The heading counts only in Title mode: a Filename commit is a rename, which
  // is not expressible as a log entry, so flushing it would put nothing in the
  // log and spend the keypress. Before the state loads the mode is a guess, and
  // the safe guess is "not ours".
  const headingOwnsLog = headingController?.saved()?.source === 'Title';
  const pending: { is: boolean; flush: () => void }[] = [
    {
      is: chipOverlayPending(),
      flush: () => chipBar?.closeEditOverlay(), // flushes the visit as one entry
    },
    {
      is: headingOwnsLog && (headingController?.hasPendingEdit() ?? false),
      flush: () => headingController?.flushPendingEdit(), // blur → commit → one entry
    },
  ];

  // Assembly only — the reading of it is `computeHistoryInputs`, which is where
  // the CM6-body and Title-heading exclusions are stated and tested. The
  // heading is log-owned while bound to Title: a native undo there would rewind
  // the DOM behind the log's back and then re-commit the rewound text as a NEW
  // entry on the next blur.
  const route = routeHistoryKey(
    computeHistoryInputs({
      activeElement: document.activeElement,
      cmBodyHasFocus: cmEditor?.getView()?.hasFocus ?? false,
      pending: pending.map((p) => p.is),
      logOwned: [headingOwnsLog ? document.getElementById('editor-heading') : null],
    }),
  );
  if (route === 'flush') {
    for (const p of pending) if (p.is) p.flush();
    return true;
  }
  if (route === 'native') return false;
  runHistory(direction);
  // TRUE regardless of whether anything moved. An empty combined history means
  // "nothing left to undo", not "someone else try": returning the `moved` flag
  // lets the key fall through to CM6's own historyKeymap (and the Edit menu to
  // execCommand), which undoes the BODY ALONE — the exact split this feature
  // exists to remove. It is reachable whenever the log is empty but CM6's is
  // not, e.g. after a failed file open.
  return true;
}

/**
 * Route, and serve the fall-through ourselves.
 *
 * `routeHistory` returning false means "the focused field's own history owns
 * this". A keystroke can simply be left un-prevented and the browser does it;
 * a BUTTON click has no such event to leave alone, so the fall-through has to
 * be performed explicitly — the same `execCommand` the Edit menu uses.
 */
function applyHistory(direction: 'undo' | 'redo'): void {
  if (routeHistory(direction)) return;
  document.execCommand(direction);
}

/**
 * Mount the cluster on the first edit of the session and keep it. A freshly
 * opened document has nothing to undo, so the controls do not exist while the
 * user is only navigating the tree.
 */
function noteDocumentEdited(): void {
  const host = document.getElementById('moss-editor-cm');
  if (host) docStateCluster.mount(host);
  docStateCluster.sync();
}
let fileTree: FileTree | null = null;
let osDragIn: OsDragInController | null = null;
let fileEvents: EditorFileEvents | null = null;
let previewFollower: PreviewFollower | null = null;
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
/** The open document when its file leaves outside moss (session/outside-file.ts). */
const outsideFile = createOutsideFile({
  hasUnsavedText: () => saveState !== 'clean' || saveTimeout !== null,
  cancelPendingSave: () => { if (saveTimeout) { clearTimeout(saveTimeout); saveTimeout = null; } },
  liveText: () => ({ frontmatter: frontmatterStore?.getValues() ?? {}, body: cmEditor?.getContent() ?? '' }),
  createFile: async (relPath) => {
    const r = await engine.createFiles([{ dir: toAbsolute(parentDirOf(relPath), projectRoot ?? ''), name: baseName(relPath), frontmatter: {} }]);
    return r.status === 'ok' ? { ok: true } : { ok: false, error: r.error };
  },
  // Saving back is the author's explicit choice to put this text at the path,
  // so the base is whatever is there now.
  writeText: async (relPath, text, keep) => {
    const abs = toAbsolute(relPath, projectRoot ?? '');
    const disk = await engine.parseFrontmatter(abs);
    if (disk.status !== 'ok') return false;
    const r = await engine.saveEditorContent(abs, text.frontmatter as JsonValue, text.body, disk.data.hash, keep);
    return r.status === 'ok' && r.data?.status === 'saved';
  },
  reopen: async (relPath) => { await loadFile(toAbsolute(relPath, projectRoot ?? '')); },
  markClean: () => setSaveState('clean'),
  showFolderOf: (relPath) => showFolderOfDeleted(relPath),
});
const otherWriters = createKeptOutsideEdit({ currentEntryId, editor: () => cmEditor, store: () => frontmatterStore });
// Set by setEditorCommitLock while a rename, move or undo commit is writing
// through the file tree — see saveActiveDocument's own guard, which is the
// actual enforcement; this flag is only the signal it reads.
let editorCommitLocked = false;
let bodyFocusOwed = false;

/** Folder navigation uses Rust's build-independent route and never falls back to '/'. */
async function navigatePreviewToFolder(absPath: string, awaitingBuild: boolean): Promise<void> {
  try {
    const result = await engine.resolveUrlForFolder(absPath);
    if (result.status === 'ok' && result.data) {
      uiHost.bus.emit('editor:navigate-preview', { urlPath: result.data, awaitingBuild });
    }
  } catch (err) {
    console.warn('[navigatePreviewToFolder] resolve threw:', err);
  }
}

let saveInFlight: Promise<void> | null = null;
let saveQueued = false;
// Outcome of the most recent write `saveActiveDocument` actually forced:
// `true` after an Ok ack, `false` after an Err/throw, `null` before any write.
// Read by `flushPendingSave` so the answer to the host's save request reflects
// the real write result, not a post-hoc `saveState` sample.
let lastWriteOk: boolean | null = null;
let schema: ContentSchema | null = null;
const unlistenFns: Unlisten[] = [];
const findController = new CmFindController();

// Save state machine: clean -> dirty -> saving -> clean | error
type SaveState = 'clean' | 'dirty' | 'saving' | 'error';
let saveState: SaveState = 'clean';

/** A pending frontmatter debounce is unsaved even before the body state changes. */
function unsavedAwareSaveState(): SaveState {
  return frontmatterStore?.hasPendingWrite() ? 'dirty' : saveState;
}

// The document's base: the disk text the buffer was last reconciled with, and
// its content hash. Every save names it and the backend refuses a file that no
// longer has it; disk observations compare by hash, never mtime.
/** The disk text the buffer was last reconciled with. `keep`: it holds another
 *  writer's lines the person's text won over, which the save writing over them
 *  keeps in history first; riding with the base, a parked document keeps it. */
interface DocumentBase extends DocumentText { hash: string; keep?: true }
let documentBase: DocumentBase | null = null;

function baseFrom(parsed: ParsedFrontmatter): DocumentBase {
  const fm = parsed.frontmatter;
  return {
    hash: parsed.hash,
    // CM6 holds LF, so the base body is compared in that form.
    body: parsed.body.replace(/\r\n?/g, '\n'),
    frontmatter: fm && typeof fm === 'object' && !Array.isArray(fm) ? fm as Record<string, unknown> : {},
  };
}

// ── Heading element + rename round-trip coordination ──────────────────

/** Renames moss issued that the watcher has not yet echoed back. Retired by
 *  the matching echo; see pending-renames.ts for why that is not a clock. */
const pendingRenames = new PendingRenames();

/**
 * Flash the breadcrumb segment for `path` with the shared just-committed
 * confirmation (the green `.moss-just-committed` pulse — see editor.css).
 * Restart-safe. No-op if no breadcrumb segment matches `path` (only the
 * active file's path appears in the breadcrumb, so a rename of any other
 * file is naturally ignored).
 */
function flashBreadcrumbSegment(path: string): void {
  const seg = document.querySelector(`.bc[data-path="${CSS.escape(path)}"]`) as HTMLElement | null;
  if (!seg) return;
  seg.classList.remove('moss-just-committed');
  void seg.offsetWidth; // force reflow so the next class add restarts the animation
  seg.classList.add('moss-just-committed');
}

/**
 * Heading controller — exposed by `wireEditorHeading`. Lets `loadFile` and the
 * frontmatter onChange handler push the canonical heading text into the DOM.
 */
let headingController: HeadingController | null = null;

/** Untitled-first create flow (create-flow.ts) — constructed in init after the
 *  heading is wired; null until then, so every call site is `createFlow?.`. */
let createFlow: CreateFlow | null = null;

// The heading's saved text, its routing source and "has the first compute
// landed" used to live here as two module variables plus a bit inside the
// controller. They described one event and were assigned in three statements,
// which is how they came apart — see the file header of editor-heading.ts.
// They are now one record, owned there: `headingController.saved()`.

/**
 * The EntryId the heading currently reflects. The active-path subscription
 * uses it to distinguish RENAME from OPEN: on rename the active EntryId is
 * unchanged (only the path moved) so `currentEntryId() === headingEntryId`
 * and the heading recomputes; on open the active EntryId differs, so the
 * subscriber skips and lets loadFile (the sole authority for the open
 * sequence: teardown -> rebuild frontmatterStore + CM6 -> applyHeadingState)
 * own the recompute. Recomputing on open would read the PREVIOUS file's
 * still-mounted frontmatterStore (loadFile fires setActive before building
 * the new store), and the stale-guard cannot catch it because setActive has
 * already advanced currentEntryId.
 */
let headingEntryId: EntryId | null = null;

/**
 * Apply the editor heading state for the current file/frontmatter.
 *
 * Calls the Rust `compute_heading_state` command — single source of truth
 * shared with the build pipeline. The result drives the heading element's
 * text + visibility, and caches the source for routing future commits.
 *
 * `filePath` is expected in the registry domain (project-root-relative).
 * `toAbsolute` is idempotent, so absolute paths are accepted too.
 */
async function applyHeadingState(
  filePath: string | null,
  frontmatter: Record<string, unknown>,
): Promise<void> {
  const headingEl = document.getElementById('editor-heading') as HTMLElement | null;
  if (!headingEl) return;
  if (!filePath) {
    headingEl.dataset.empty = 'true';
    delete headingEl.dataset.headingSource;
    headingController?.clear();
    return;
  }
  const body = cmEditor?.getContent() ?? '';
  // Captured before the await: docGeneration moves only where the document
  // surface is actually replaced (see its declaration), so this drops an
  // in-flight result for a document that has since been replaced — by a new
  // open, INCLUDING a reopen of this same path — without also dropping a
  // result for a document a failed load left standing.
  const gen = docGeneration.current();
  const heading = headingGeneration.bump();
  // Tauri command boundary: registry stores relative, Rust opens via std::fs.
  const result = await engine.computeHeadingState(
    toAbsolute(filePath, projectRoot ?? ''),
    frontmatter as unknown as JsonValue,
    body,
  );
  if (docGeneration.isStale(gen) || headingGeneration.isStale(heading)) return;
  if (result.status !== 'ok') {
    console.warn('computeHeadingState failed:', result.error);
    headingEl.dataset.empty = 'true';
    delete headingEl.dataset.headingSource;
    headingController?.clear();
    return;
  }
  const { visible, text, source } = result.data;
  // Single-owner: the editor heading shows EXACTLY what moss-core resolved
  // (title: when set, else the filename-derived text) — byte-identical to
  // the <h1> the build pipeline injects. No second derivation here; that
  // would drift from the preview.
  // `source` routes heading commits (Title -> frontmatter, Filename ->
  // rename) and is the moss-core-owned discriminant, never decided in TS.
  // Record which file the heading now reflects, so the active-path
  // subscriber can tell a rename (same id) from an open (new id).
  headingEntryId = currentEntryId();
  headingEl.dataset.empty = visible ? 'false' : 'true';
  // Reflect the moss-core-owned HeadingSource to the DOM (lowercased) so the
  // agent-driven harness can read it via __moss_debug.getHeadingState(). This
  // is the ONLY place source enters the DOM — TS never re-decides it.
  headingEl.dataset.headingSource = source.toLowerCase();
  // One call: it records the text, the routing source and the fact that the
  // first compute has landed, and decides for itself whether putting the text
  // on screen would stomp an edit in progress, an IME conversion or the caret.
  // Those used to be three statements with three different guards.
  headingController?.setSaved(text, source);
}

// ── Initialization ─────────────────────────────────────────────────────

async function init(): Promise<void> {
  // This panel shows Rust-sent toasts when it is the surface on screen, so a
  // toast shown here must reach the log bundle and a link on it must open.
  installHostToastRuntime();
  initLocale();        // must be first: locale needed before any UI renders
  // Drive CJK font/justify CSS (html[lang^="zh"], :lang(zh-*)) in the editor webview.
  // Without this the editor renders Latin fonts for zh users (launcher.ts does the same).
  document.documentElement.lang = getLocale();
  // Localize the static editor.html chrome (layout-toggle tooltip + the
  // layout-button tooltips) via their data-i18n* attributes. Must run
  // after initLocale() resolves the table and before initTooltips() reads
  // data-tooltip. Mirrors launcher.ts applyStaticStrings().
  applyStaticStrings();
  // React to a live language switch (no restart). Webview-level: the editor
  // webview is created once (pre-warmed) and its JS context persists across
  // show/hide, so registering once here does not accumulate.
  void installLocaleReactivity();
  // Portal tooltip (data-tooltip=) — must be mounted before any element is hovered.
  initTooltips();
  applyOwnTitlebarClass(); void wireMainShellCaptionButtons(); // own-titlebar/controls-trailing + own-caption wiring — see utils/fullscreen-class.ts

  // Edit ▸ Undo/Redo arrive as an event, not a keystroke (edit-history.ts).
  // With the caret in the document CodeMirror's history answers — it is the
  // only one that knows about the edits moss makes for the author. Elsewhere
  // in the panel (chip-bar fields) the field's own history does.
  initEditHistoryMenu((direction) => routeHistory(direction));

  // Mount the Inline renderer here. It MUST live in the action-panel
  // webview because its primary anchor — the titlebar's `#moss-save-status`
  // (`data-inline-task-anchor="save"` in editor.html) — exists in this
  // webview's DOM, not the main shell's. Mounting in app.ts (main webview)
  // would force every Save badge through the body-overlay fallback in the
  // wrong window.
  // The other tone renderers (Narrator/Awaiting/Toast) stay in app.ts
  // where their anchors live (titlebar slot, toast container).
  void mountInlineStatus();

  // Observability: report when a click fails to focus an editable editor — a
  // covering overlay would otherwise leave the editor silently un-clickable
  // (the v0.7.9 "cannot place cursor" class of bug) with nothing in the log.
  const focusProbeTarget = document.getElementById('moss-editor-cm');
  if (focusProbeTarget) {
    installEditorFocusProbe(focusProbeTarget, () => cmEditor?.getView(), (m) => void logInfo(m));
  }

  // URL params are read early ONLY to compute the editorBootstrap argument
  // below. The composer branch and the initial-load block keep their
  // positions at the END of init (after all listener registrations) —
  // moving them up would change event-arrival semantics during boot
  // (the host's save request, editor:seal, FileChanged coordinator).
  const params = new URLSearchParams(window.location.search);
  // ?compose=<article-id> → the email composer owns the panel.
  // A host without a composer ignores `?compose=`.
  const composeArticleId = host.composer ? params.get('compose') : null;
  const initialSource = params.get('source');
  // Paint-only, so it may run this early (the composer BRANCH stays at the
  // end of init — see the comment above): hide the editor-only chrome now
  // rather than after the bootstrap await, or the breadcrumb strip paints at
  // t=0 and is then torn down (boot flash).
  if (composeArticleId) hideChromeForComposer();

  // ── EditorFileEvents coordinator ─────────────────────────────────────────
  // Owns ONE onMossEvent('FileChanged') and ONE onMossEvent('AssetReady') for
  // the entire editor webview. FileTree, CmEditor's reference resolver, and the
  // dispatchEditorFileChanged handler all register with this coordinator instead
  // of self-subscribing — so there is exactly ONE cross-webview subscription per
  // event type in the action-panel webview.
  //
  // Started BEFORE FileTree and CmEditor are constructed so they can receive the
  // coordinator at construction time. Same fire-and-forget pattern as the previous
  // direct `void onMossEvent(...)` subscription; no event can arrive during the
  // listen() registration window (Tauri delivers over IPC, not synchronously).
  fileEvents = new EditorFileEvents();
  void fileEvents.start();

  // Keep the editor breadcrumb / email-composer titlebar aligned with the macOS
  // traffic lights: collapse the left inset in fullscreen, matching the shell's
  // Edit button. Same shared helper the app shell uses (utils/fullscreen-class).
  // Fire-and-forget (like `fileEvents.start()` above) — must NOT block init. The
  // helper's initial isFullscreen() pull is a fast IPC that resolves well before
  // the file-tree spine allocator's first run, which is gated behind the slower
  // editorBootstrap RPC below; so a fullscreen cold-load still sets
  // `fullscreen-mode` before the spine reads --moss-traffic-light-inset. Awaiting
  // it instead perturbs init's async timing enough to break the green-flash /
  // import-refresh tests, for no real benefit.
  void installFullscreenBodyClass();

  // Single editor FileChanged handler — merges the import/onboarding refresh and
  // the open-file body/frontmatter reload (previously two listeners, one of them
  // wired to the dead `'file-changed'` Tauri event). Routes through the extracted,
  // tested dispatcher. Fires on every rebuild; the dispatcher's dirty/pending/echo
  // guards keep the editor's own auto-save writes safe.
  //
  // Priority 0 so the registry sync (inside dispatchEditorFileChanged) runs before
  // the reference-resolver (priority 50) and file-tree (priority 50) handlers.
  // No need to push the unregister fn to unlistenFns — fileEvents.stop() tears
  // down all registrations including this one.
  fileEvents.onFileChanged(async (fce) => {
    if (!fce) return;
    await dispatchEditorFileChanged(fce, {
      registry,
      projectRoot,
      currentFilePath,
      currentEntryId,
      pendingRenames,
      reloadExternalDocument,
      // A deleted file's unsaved text is offered back by its own notice.
      documentDeletedOutside: (path) => { otherWriters.withdrawNotKept(); return outsideFile.deletedOutside(path); },
      holdsDetachedDocument: () => outsideFile.holdsDocument(),
      // Preserve the original `!fileTree` guard: the old onboarding listener
      // early-returned when fileTree was null, so flash only fires with a tree.
      flashDownloadTargets: (created) => { if (fileTree) flashDownloadTargets(created); },
      onboardingRefresh: async (deletedSource) => {
        if (deletedSource) { await showFolderOfDeleted(deletedSource); return; }
        const request = loadGeneration.current();
        await fileTree?.refreshAndRender();
        if (loadGeneration.isStale(request)) return;
        if (lastSurface?.kind === 'root-empty') {
          await applyFolderFromDisk(lastSurface.folder);
        } else if (lastSurface?.kind === 'takeover') {
          await applyFolderFromDisk(lastSurface.takeover);
        } else if ((fce.source_creates?.length ?? 0) > 0) {
          await resolveAndApplyRootSurface();
        }
      },
    });
    // A save found the file gone and the event re-keyed the document to where
    // it went: resume saving there.
    if (outsideFile.followRenames(fce.source_renames ?? []) && saveState === 'dirty') scheduleSave();
  }, 0);

  // Source-agnostic green flash for raw FS file-create events (e.g. Matters
  // download creating files outside a process-hook build). Additive to the
  // source_creates flash path — the two are complementary (source_creates covers
  // the re-open blocking path; RawFileCreated covers the watcher-live path).
  //
  // Note: on a first-connect Matters import, BOTH RawFileCreated (this handler)
  // AND source_creates (FileChanged handler) may fire for the same new files.
  // This is intentional and benign — both routes feed the same 400ms accumulator
  // in flashDownloadTargets, so they coalesce into a single green pulse.
  //
  // The unregister return from onRawFileCreated is intentionally discarded:
  // fileEvents.stop() wipes ALL registered handlers at once (same pattern as
  // the onFileChanged registration above). No per-handler teardown is needed.
  fileEvents.onRawFileCreated((e) => {
    if (fileTree) flashDownloadTargets(e.paths);
  });

  // One bootstrap round trip: schema + project path + best-effort initial
  // parse of the ?source= file. Replaces the serial getSchema →
  // getProjectPath → (initial) parseFrontmatter chain on cold start. The
  // FileChanged coordinator above stays registered BEFORE this await so any
  // external write that lands between the bootstrap's server-side parse and
  // the CM6 mount is replayed by the existing reconciliation.
  const boot = await engine.editorBootstrap(composeArticleId ? null : initialSource);
  if (boot.status !== 'ok') {
    console.error('Failed to bootstrap editor:', boot.error);
    return;
  }
  schema = boot.data.schema; applyCaptionOwnerClass(!boot.data.project_path); // owns-caption seed for document mode — see applyCaptionOwnerClass's own doc
  // App-level toasts land here when this panel is what the user sees.
  if (uiHost.toasts) void mountToasts({ label: 'action-panel', listen: uiHost.engine.on, ...uiHost.toasts }).then((fns) => unlistenFns.push(...fns)).catch(() => {});

  // Initialize file tree (renders inside nav-surface grid)
  const navSurface = document.getElementById('nav-surface');
  const navGrid = document.getElementById('nav-grid');
  const dragHandle = document.getElementById('divider');
  if (navGrid && dragHandle && navSurface) {
    registry = new EntryRegistry();
    // Manual smoke-testing hook (matches the window.__moss_* convention used
    // elsewhere). Exposes the live registry, selection, and active entry so
    // dev users and manual verification scripts can inspect state from
    // the browser console / mcp-server-tauri webview_execute_js.
    const w = window as unknown as {
      __moss_debug?: {
        registry: EntryRegistry;
        getSelection?: () => { selected: EntryId[]; anchor: EntryId | null; focus: EntryId | null } | null;
        getCurrentEntry?: () => { entryId: EntryId | null; path: string | null };
        getFrontmatterStore?: () => unknown;
        saveActiveDocument?: (reason?: 'autosave' | 'forced') => Promise<void>;
        flushPendingSave?: () => Promise<boolean>;
        setEditorCommitLock?: (locked: boolean) => void;
        getHeadingState?: () => { text: string | null; source: string | null; visible: boolean };
        getDiagnostics?: () => EditorDiagnostic[];
        getProblems?: () => ReturnType<typeof readProblems>;
        forceLint?: () => void;
        getWarmState?: () => { visibility: DocumentVisibilityState; cmMounted: boolean; pendingOpen: boolean };
      };
    };
    w.__moss_debug = w.__moss_debug ?? {} as NonNullable<typeof w.__moss_debug>;
    w.__moss_debug.registry = registry;
    // fileEvents is always non-null post-init; ?? undefined satisfies the optional-param type (module field is EditorFileEvents | null).
    fileTree = new FileTree(registry, fileEvents ?? undefined, {
      flushPendingSave: () => flushPendingSave(),
      setEditorCommitLock: (locked) => setEditorCommitLock(locked),
      reloadActiveFileFromDisk: reloadExternalDocument,
    });
    osDragIn = createOsDragInController(fileTree);
    w.__moss_debug.getSelection = () => {
      if (!fileTree) return null;
      const snap = fileTree.getSelectionSnapshot();
      return {
        selected: [...snap.selected],
        anchor: snap.anchor,
        focus: snap.focus,
      };
    };
    // __moss_debug.getCurrentEntry returns the active EntryId and its current
    // path FROM THE REGISTRY (project-root-relative, e.g. "works/post.md", NOT
    // "/Users/.../works/post.md").
    // Playwright / integration tests that compare against absolute paths must
    // convert via toAbsolute().
    w.__moss_debug.getCurrentEntry = () => {
      const id = fileTree?.getActiveEntryId() ?? null;
      return {
        entryId: id,
        path: id !== null ? registry?.getById(id)?.path ?? null : null,
      };
    };
    w.__moss_debug.getFrontmatterStore = () => frontmatterStore;
    w.__moss_debug.saveActiveDocument = saveActiveDocument;
    // Drain hook for test harnesses: lets a test settle THIS module
    // graph's pending save machinery (debounce timer + in-flight chain) inside
    // its own window, so a surviving autosave can't fire into a later test.
    w.__moss_debug.flushPendingSave = flushPendingSave;
    // Same testability rationale: lets a test drive the rename/move/undo
    // read-only lock directly, without booting the tree, to exercise
    // saveActiveDocument's guard against autosave while a commit holds the lock.
    w.__moss_debug.setEditorCommitLock = setEditorCommitLock;
    // Pure DOM reader of the rendered heading ({text, source, visible}) for the
    // agent-driven e2e harness. Reads #editor-heading's
    // textContent / data-heading-source / data-empty — see moss-debug.ts.
    w.__moss_debug.getHeadingState = getHeadingState;
    // Agent-driven lint verification (testability): forceLint() runs the lint
    // sources immediately (bypassing the throttled debounce that never fires in
    // an unfocused webview), and getDiagnostics() reads the settled diagnostics
    // as structured data — so lint warnings in a JSON file are verifiable
    // headlessly, with no window focus / paint / rAF. Pattern: forceLint() →
    // poll getDiagnostics() until stable. See moss-debug.ts. A markdown
    // document's problems are records, read synchronously by getProblems().
    w.__moss_debug.forceLint = () => {
      const v = cmEditor?.getView();
      if (v) forceLinting(v);
    };
    w.__moss_debug.getDiagnostics = () => {
      const v = cmEditor?.getView();
      return v ? readDiagnostics(v) : [];
    };
    w.__moss_debug.getProblems = () => {
      const v = cmEditor?.getView();
      return v ? readProblems(v.state) : [];
    };
    // Pure reader of hidden/warm state for the e2e
    // probes. WebKit flips document.visibilityState to 'hidden' inside a
    // hidden child webview (spike-verified), mirroring Webview::hide().
    w.__moss_debug.getWarmState = () => ({
      visibility: document.visibilityState,
      cmMounted: cmEditor !== null,
      pendingOpen: !!(window as unknown as { __moss_pending_open?: unknown }).__moss_pending_open,
    });
    // The editor's path domain needs A root; a *site* is a separate question.
    // `project_path` is the site; `editor_root` is the loose file's parent in
    // document mode, where there is no site at all. Exactly one is set.
    //
    // Gating this block on `project_path` alone is what left document mode
    // broken: `projectRoot` stayed null, so `toRelative(abs, '')` returned the
    // absolute path unchanged and `EntryRegistry.upsert` threw — the panel
    // opened and the document never rendered.
    //
    // Truthy check is behavior-equivalent to the old get_project_path status
    // gate: the bootstrap yields null when no project is open —
    // keep the truthy check, do not "fix" it back to a status check.
    const editorRoot = boot.data.project_path ?? boot.data.editor_root;
    if (editorRoot) {
      // Swap the static cold-start nav skeleton for the real breadcrumb
      // strip: mount() APPENDS the (identically-sized, hairlined) strip
      // synchronously in the same task, so dismissing first is seamless —
      // never a double strip, never a gap. Runs on every project-open boot
      // (composer included — tree mount precedes the composer branch).
      if (projectRoot !== editorRoot) {
        projectRoot = editorRoot;
        publishPreflightRoot = editorRoot;
        invalidatePublishPreflightPresentation(true);
      }
      dismissNavSkeleton();
      await fileTree.mount(navGrid, editorRoot, getInternalToggleState(), boot.data.project_path !== null);
      fileTree.setupDragHandle(dragHandle, navSurface);
      rootName = boot.data.root_name;
      const applyPreflight = (folder: string, projection: PublishPreflightProjection | null) => {
        if (folder !== projectRoot || folder !== publishPreflightRoot) return;
        publishPreflightProjection = projection;
        fileTree?.setPublishPreflight?.(projection);
        void renderPublishPreflightHints();
      };
      const unsubscribePreflight = publishPreflightStore.subscribe(applyPreflight);
      unlistenFns.push(unsubscribePreflight);
      // Publish preflight is a question about a site; a loose document's
      // folder is not one, and nothing builds there to refresh it.
      if (boot.data.project_path) {
        primePublishPreflight(publishPreflightStore, projectRoot);
        void onMossEvent('BuildComplete', () => {
          if (projectRoot) primePublishPreflight(publishPreflightStore, projectRoot);
          void refreshPageFacts(); // an edit such as `nav: true` changes what the build picks
        }).then((unlisten) => unlistenFns.push(unlisten));
      }

      // Single subscriber for editor → preview navigation. Owns the entire
      // editor:navigate-preview decision: subscribes to the registry's
      // active-path signal, resolves URL via resolve_url_for_file, emits when
      // known, subscribes to FileChanged and retries when not. Never falls
      // back to '/' — that fallback was the root of the post-rename home-jump
      // regression.
      //
      // Gated on the SITE, not on `editorRoot`: document mode has no preview
      // webview to follow, and resolve_url_for_file has no built site to answer
      // from. Every consumer below is already `previewFollower?.`-optional.
      if (boot.data.project_path) {
        previewFollower = new PreviewFollower({
          registry,
          projectRoot: projectRoot!,
          resolveUrlForFile: engine.resolveUrlForFile,
          // What it says (navigate the preview, reveal a slot) is for the shell: the bus.
          emit: async (name, payload) => uiHost.bus.emit(name, payload),
          onMossEvent,
          describeSourceRole: engine.describeSourceRole,
          setNote: setPreviewNote,
        });
        previewFollower.start();
      }

      // Heading reacts to active-path changes. The heading is a projection of
      // (activePath, frontmatter); frontmatter is already wired via
      // frontmatterStore.subscribe, this is the path half. It fires on RENAME
      // of the active file (registry.rename moves the active id's path) and on
      // OPEN (setActive to a new id). We only recompute on rename: same
      // EntryId, frontmatter unchanged. On open the EntryId differs and
      // loadFile owns the recompute with the new file's frontmatter (see
      // headingEntryId). This is the structural fix for the rename-staleness
      // bug — no enumerated call site to forget.
      registry?.subscribeActivePath((newPath) => {
        if (!newPath) return;
        if (currentEntryId() !== headingEntryId) return; // open, not rename
        const fm = frontmatterStore ? frontmatterStore.getValues() : {};
        void applyHeadingState(newPath, fm);
      });

      // Cross-pane focus routing (keyboard navigation).
      // editor:focus-tree — dispatched by FileTree.handleKey on Cmd+1 or
      // when handleKey routes a cross-pane action from inside the tree.
      document.addEventListener('editor:focus-tree', () => {
        fileTree?.focus();
      }, { signal: scaleAbort.signal });
      // editor:focus-editor — dispatched by FileTree.handleKey on Cmd+2 or
      // on Cmd+Down (open file + transfer focus to editor pane).
      document.addEventListener('editor:focus-editor', () => {
        cmEditor?.getView()?.focus();
      }, { signal: scaleAbort.signal });
      // Leaving the editor hands the lead to the preview.
      window.addEventListener('blur', followPreview, { signal: scaleAbort.signal });
      // The panel's grow animation and a divider drag both resize this window.
      window.addEventListener('resize', followPreviewPosition, { signal: scaleAbort.signal });

      // Listen for file selection from tree or Cmd+Click navigation.
      //
      // `e.detail.filePath` is
      // PROJECT-ROOT-RELATIVE (the file-tree dispatches `entry.path` from the
      // registry; editor:load-source forwards `sourcePath` from resolve_page_source
      // which is wire-relative).
      document.addEventListener('editor:file-selected', ((e: CustomEvent<{ filePath: string; isNewFile?: boolean }>) => {
        void openSelectedEntry(e.detail.filePath, e.detail.isNewFile === true);
      }) as unknown as EventListener, { signal: scaleAbort.signal });

      // A plain tree or breadcrumb folder click (and the keyboard 'open' key) must not
      // change the editor surface: it only toggles expand/collapse and selection. The
      // "create folder page" CTA appears only from preview or URL navigation
      // (applyResolvedNode → decideEditorSurface → applyEditorSurface). See selectFolder()
      // in file-tree.ts.

      // editor:folder-navigate — the gated surface event for a HOME-LESS folder,
      // fired by file-tree's dispatchFolderSurface from the delete-fallback
      // (applyDeleteLocally) and folder creation. This is a FORCED navigation
      // (NOT a plain user folder click — those only expand/collapse + select), so
      // it must explicitly resolve the editor surface to show the "create a
      // folder page" CTA and clear any stale content, and move the preview to the
      // folder's own page. Plain selectFolder calls do NOT dispatch this.
      document.addEventListener('editor:folder-navigate', ((e: CustomEvent<{ folderPath: string; folderName: string; justCreated?: boolean }>) => {
        const { folderPath, folderName } = e.detail;
        void applyFolderFromDisk(folderHomeTakeover(folderPath, folderName, folderPath === projectRoot));
        // Move the preview to the folder's OWN page too. Every folder is now
        // published as an index page (empty folders included), so resolve the
        // folder's served URL and emit editor:navigate-preview through the same
        // shared build-independent seam the new-file branch uses — folder-create
        // AND the delete-fallback both land the preview on the folder page.
        // folderPath is already absolute (the file-tree dispatches folderAbs),
        // which is what resolveUrlForFolder expects. Null-safe, never '/'.
        // Only a JUST-CREATED folder waits: the delete-fallback shares this
        // event, and its folder page is already in the build output.
        void navigatePreviewToFolder(folderPath, e.detail.justCreated === true);
      }) as unknown as EventListener);

      // editor:create-home — the explicit "Create home file" folder context-menu
      // item (file-tree showFolderCtx, shown only for a home-less folder). Routes
      // to the SAME shared create-home action the preview-nav "Create folder
      // page" CTA uses. `folderPath` is the absolute directory; `folderName` is
      // the root-aware display name (getName), so the root gets `<rootName>.md`
      // and a subfolder `<folderName>.md`.
      document.addEventListener('editor:create-home', ((e: CustomEvent<{ folderPath: string; folderName: string }>) => {
        void takeOver(folderHomeTakeover(e.detail.folderPath, e.detail.folderName, e.detail.folderPath === projectRoot));
      }) as unknown as EventListener, { signal: scaleAbort.signal });

      // editor:open-versions — "Versions…" / "Site versions…" from a tree row's
      // context menu (tree/controller.ts). Handled HERE, in the webview the
      // tree lives in: the route asked the main window's panel manager
      // to switch views, but module state is per-webview, so the tree's call
      // always found nothing registered and did nothing.
      document.addEventListener('editor:open-versions', ((e: CustomEvent<{ scope: 'page' | 'site'; path: string; isRoot: boolean }>) => {
        void versionsMode.open(e.detail.scope, e.detail.path, e.detail.isRoot);
      }) as unknown as EventListener, { signal: scaleAbort.signal });

      // Tree rename — fires from the file-tree's startRename flow when the
      // currently-viewed file is the one being renamed. We pre-register the
      // round-trip (so the watcher's file-changed echo is silenced) and,
      // after the rename completes, slot the new path into currentFilePath
      // without reloading or navigating the preview. The latter chain is
      // what jumps the editor to home: resolveUrlForFile returns null
      // pre-rebuild, navigate-preview falls back to '/', and the preview→
      // editor sync re-loads the home file.
      document.addEventListener('editor:file-renaming', ((e: CustomEvent<{ oldPath: string; newPath: string }>) => {
        pendingRenames.register(e.detail.oldPath, e.detail.newPath);
      }) as unknown as EventListener);
      document.addEventListener('editor:file-renamed', ((e: CustomEvent<{ oldPath: string; newPath: string }>) => {
        // Ordering invariant:
        // this handler runs SYNCHRONOUSLY from FileTree.commitRename BEFORE
        // refreshTree. registry.rename moves the active EntryId's path
        // from oldPath to newPath so the subsequent reconcileSnapshot in
        // refreshTree sees newPath bound to X and preserves identity.
        //
        // DO NOT add work to this handler that depends on the file-tree
        // (rendered rows, breadcrumb) reflecting the rename. Tree state is rebuilt
        // by the AFTER-this-handler refreshTree call; reading it here
        // would observe the pre-rename layout. Side-effects that need
        // tree state belong on the canonical `editor:rename-complete`
        // event instead (file-tree.ts:commitRename dispatches it after
        // refreshTree).
        registry?.rename(e.detail.oldPath, e.detail.newPath);
        // currentEntryId() stays the same — that's the point of preserving
        // identity. After the rename above, currentFilePath() now returns
        // e.detail.newPath via the canonical owner.
      }) as unknown as EventListener);

      // PreviewFollower + the heading recompute use the active-path signal
      // (fires synchronously during registry.rename). The just-committed
      // breadcrumb flash, however, must run AFTER refreshTree rebuilds the
      // segment DOM — so it hooks editor:rename-complete (dispatched by
      // file-tree post-refresh, file-tree.ts:2790). The selector naturally
      // scopes to the active file: only its path appears in the breadcrumb,
      // so a rename of any other file no-ops.
      document.addEventListener('editor:rename-complete', ((e: CustomEvent<{ oldPath: string; newPath: string }>) => {
        flashBreadcrumbSegment(e.detail.newPath);
      }) as unknown as EventListener, { signal: scaleAbort.signal });
    } else {
      // The bootstrap carries no error string for the no-project case —
      // project_path is simply null.
      console.warn('No project open, file tree not initialized');
    }
  }

  // Listen for internal-files toggle changes (dispatched from root folder context menu).
  // Registered outside the projectResult guard intentionally: fileTree uses optional
  // chaining (?.) so firing when no tree is mounted is safe, and the handler must
  // survive folder switches that re-initialize fileTree inside the guard.
  document.addEventListener('editor:internal-toggle-changed', (async (e: CustomEvent<boolean>) => {
    await fileTree?.toggleInternal(e.detail);
  }) as unknown as EventListener);

  // Escape does not park the file tree from here: it dismisses what the user
  // opened, and a hidden one-way gesture on the whole document would shadow no
  // visible control. The divider double-click toggles the tree; Cmd+Shift+← is
  // the keyboard accelerator for the breadcrumb chevron.
  //
  // The tree's OWN Escape handling (clear selection, file-tree.ts handleKey
  // 'clear') is untouched — that one dismisses something the user did open.

  // ── Concurrent listener registration ─────────────────────────────────────
  // Every registration below is an independent event channel; handlers read
  // module state at fire time, not registration time, and nothing depends on
  // a prior registration. Each listen() is its own IPC round trip — awaited
  // serially they dominated the cold-start critical path — so they are
  // collected here and awaited in ONE Promise.all batch below. The combined
  // await keeps this position in init(): every listener is still registered
  // before the initial loadFile.
  const pendingListeners: Array<Promise<Unlisten> | Unlisten> = [];

  // Follow the shell's theme: the OS directly, and the moon toggle through the
  // shared-origin `storage` event. editor.html's pre-paint covers first paint.
  followTheme();

  // Listen for direct file load events
  pendingListeners.push(
    uiHost.bus.on<{ filePath: string }>('editor:load-file', async (payload) => {
      await loadFile(payload.filePath);
    }),
  );

  // Drop pending renames when the project folder changes — they are keyed by
  // paths from the PREVIOUS project and would suppress a real event in the new
  // one for a file that happens to share a relative path (e.g. "index.md" in
  // both). app.emit('build-config-changed') broadcasts to all webviews —
  // including this one — every time the user switches folders.
  pendingListeners.push(
    uiHost.engine.on('build-config-changed', () => {
      pendingRenames.clear();
    }),
  );

  // Listen for pre-resolved source path from the shell (Preview → Editor sync).
  // The shell calls resolve_page_source once and forwards the result here.
  // The wire payload carries an ABSOLUTE path because resolve_page_source
  // (the host editor commands) builds it via `folder_path.join(...)`.
  // Normalize to the registry domain (project-root-relative) once so the
  // getByPath lookup hits and the dedup against currentFilePath() compares
  // in the same domain.
  pendingListeners.push(
    uiHost.bus.on<EditorNodeWire>('editor:load-source', async (payload) => {
      await applyResolvedNode(payload);
    }),
  );

  // Wire layout toggle button (split / full-width)
  wireLayoutButton();

  // The right edge's "back to split" accelerator, live only while the editor
  // is full-width — the one place the shell cannot paint, because the preview
  // webview (which IS the shell document) is natively hidden there.
  // The layout button above stays the primary exit; this only accelerates it.
  initEdgeRestore();

  // Listen for preview-to-editor scroll events (line-based or heading-based)
  pendingListeners.push(
    uiHost.bus.on<{ sealed: boolean }>('editor:seal', (payload) => {
      // The ship conductor seals the editor while publishing.
      // - CM6 read-only: a live Compartment reconfigure (no remount).
      // - Scrim: a translucent overlay with "Publishing your site…" caption.
      // - File tree: locked so the user can't switch files mid-ship (R13).
      const sealed = !!payload?.sealed;
      // Observability (issue #5b): the seal/unseal cycle is the prime suspect for
      // a stuck non-editable editor. Logging every RECEIVED event makes a dropped
      // unseal (sealed=true with no following sealed=false) visible in moss.log,
      // and records the editor's editable state — which nothing else does.
      void logInfo(`[editor:seal] received sealed=${sealed} → editor editable=${!sealed}, hasEditor=${!!cmEditor}`);
      cmEditor?.setReadOnly(sealed);
      fileTree?.setSealed(sealed);
    }),
  );

  pendingListeners.push(
    uiHost.bus.on<{ line?: number; atTop?: boolean; atBottom?: boolean; answering?: string }>('scroll-sync:preview-to-editor', (payload) => {
      onPreviewScroll(payload);
    }),
  );

  // Click-to-source from the preview, two strengths:
  //
  // Single click → reveal-line: scroll the mapped source into view + a brief
  // wash. No focus, no caret — the user is orienting, not editing. And never
  // intercept the expert: when the editor itself has focus with a pending
  // edit, the flash still shows but their scroll position is left alone.
  //
  // Every handler from here to open-field answers a gesture made IN THE
  // PREVIEW, so each starts with followPreview(): the editor scrolls or moves
  // its caret in reply, and that must not be sent back to move the page the
  // user just clicked on.
  pendingListeners.push(
    uiHost.bus.on<{ startLine: number; endLine: number }>('editor:reveal-line', (payload) => {
      if (!cmEditor) return;
      followPreview();
      const view = cmEditor.getView();
      revealLines(view, payload.startLine, payload.endLine, {
        preserveScroll: shouldPreserveEditorPosition(view.hasFocus, saveState),
      });
    }),
  );

  // Double click → the full jump: editor focused, caret at the mapped position.
  // The click landed in the preview's webview, so a DOM focus() here would
  // move the caret without the keyboard — this webview asks for it natively
  // too, or the caret line would neither blink nor reveal its source.
  pendingListeners.push(
    uiHost.bus.on<{ path: string; sourceRevision: string; sourceSpan: { start_byte: number; end_byte: number; line: number } }>(EDITOR_OPEN_PUBLISH_PREFLIGHT, (payload) => {
      pendingPreflightOpen = payload;
      void revealPendingPreflight();
    }),
  );
  pendingListeners.push(
    uiHost.bus.on<{ line: number }>(EDITOR_GOTO_LINE, (payload) => {
      if (!cmEditor) return;
      followPreview();
      uiHost.focusView?.();
      gotoLine(cmEditor.getView(), payload.line);
    }),
  );
  pendingListeners.push(
    uiHost.bus.on<{ selector: string }>(EDITOR_GOTO_RULE, (payload) => {
      const view = cmEditor?.getView();
      const line = view && findRuleLine(view.state.doc.toString(), payload.selector);
      if (view && line) { followPreview(); uiHost.focusView?.(); gotoLine(view, line); }
    }),
  );

  // Listen for text selection sync from preview. A read gesture: the keyboard
  // stays in the preview, where the user may be about to copy what they chose.
  pendingListeners.push(
    uiHost.bus.on<{ text: string; startLine: number; endLine: number }>('editor:select-text', (payload) => {
      if (!cmEditor) return;
      followPreview();
      const { text, startLine, endLine } = payload;
      selectTextNearLine(cmEditor.getView(), startLine, endLine, text);
    }),
  );

  // A SINGLE click in the preview on something rendered from frontmatter
  // (title, date, byline, colophon). Point at the chip; do not open it — the
  // preview is a view, and a single click asked where the value lives, not to
  // edit it.
  pendingListeners.push(
    uiHost.bus.on<{ field: string }>('editor:reveal-field', (payload) => {
      if (!chipBar) return;
      followPreview();
      chipBar.revealField(payload.field);
    }),
  );

  // A DOUBLE click on the same chrome commits to the edit: open the chip's
  // editor, exactly as clicking the chip itself would — keyboard included.
  pendingListeners.push(
    uiHost.bus.on<{ field: string; seed?: unknown }>(EDITOR_OPEN_FIELD, (payload) => {
      if (!chipBar) return;
      followPreview();
      uiHost.focusView?.();
      chipBar.openField(payload.field, payload.seed);
    }),
  );

  // The host asks before the panel closes or the app quits, so a pending
  // auto-save is written rather than killed with its debounce.
  if (uiHost.onSaveRequest) pendingListeners.push(
    uiHost.onSaveRequest(async () => {
      // A document left with text no save could write is parked, not
      // written: closing must not report it saved.
      const ok = await flushPendingSave() && ![...parked.values()].some((doc) => doc.unsaved);
      // The composer owns the panel on the ?compose= path; its draft rides the
      // same request (panel switch, app shutdown).
      await composerHandle?.flush();
      return ok;
    }),
  );

  // External file drag-and-drop (Finder → moss), through the host's OS drag
  // rather than HTML5 dataTransfer.files, in this view's CSS pixels. Two zones:
  //   1. CM editor area: handleEditorFileDrop() embeds every kind as ![](path); a
  //      destination the backend refuses falls back to ![[name]].
  //   2. File tree area: os-drag-in module does position-based hit-testing to
  //      find the target folder, then calls copyFilesToProject.
  if (uiHost.onFileDrop) pendingListeners.push(
    uiHost.onFileDrop(async (payload) => {
      if (payload.type === 'enter') {
        // Don't highlight both zones at once — wait for 'over' to determine position
      } else if (payload.type === 'over') {
        const logicalX = payload.x;
        const logicalY = payload.y;
        routeDropIndicator(logicalY, logicalX);
        // Tauri's drag_drop_handler (required for OS-drag-in, see os-drag-in.ts)
        // intercepts the ENTIRE native drag session at the webview level —
        // including ones that originate from our own in-app draggable rows —
        // so an in-app row/breadcrumb drag NEVER reaches DOM dragover/drop
        // Route those through FileTree's own
        // canDropInto-aware logic instead of os-drag-in's Finder-import path,
        // which doesn't gate canDropInto and would highlight invalid targets
        // (e.g. an item's own current parent) for an internal drag.
        if (fileTree?.hasActiveInternalDrag()) {
          fileTree.handleInternalDragOver(logicalX, logicalY);
        } else if (osDragIn) {
          // Highlight the folder row under the cursor during OS drag-over
          osDragIn.handleOsDragOver(logicalX, logicalY);
        }
        // Drive file-tree edge-scroll for OS drags from the file manager.
        // OS drags go through Tauri's onDragDropEvent ('over') and never fire
        // HTML5 dragover on navBody, so the navBody dragover listener in mount()
        // is dead for this path. updateEdgeScroll uses the same logical-pixel Y
        // that navBody's getBoundingClientRect() uses, so coordinate spaces match.
        fileTree?.updateEdgeScroll(logicalY);
        // Update the OS-drop insertion caret when over the editor.
        // dropCursor() cannot see OS drags; osDropCaretField (cm-os-drop-caret.ts)
        // drives the caret manually from Tauri 'over' coords.
        {
          const cmRect = document.getElementById('moss-editor-cm')?.getBoundingClientRect();
          const isOverEditor = cmRect && logicalX >= cmRect.left && logicalX <= cmRect.right && logicalY >= cmRect.top && logicalY <= cmRect.bottom;
          const view = cmEditor?.getView();
          if (isOverEditor && view) {
            const rawPos = view.posAtCoords({ x: logicalX, y: logicalY }) ?? null;
            // Snap to block boundary so the horizontal indicator lands between
            // paragraphs (not mid-word). Pass logicalY for top/bottom-half detection.
            const pos = rawPos != null
              ? snapToBlockBoundary(view, rawPos, logicalY)
              : null;
            view.dispatch({ effects: setOsDropCaret.of(pos) });
          } else if (view) {
            view.dispatch({ effects: setOsDropCaret.of(null) });
          }
        }
      } else if (payload.type === 'drop') {
        showDropZone(false);
        // Stop edge-scroll on OS drop.
        fileTree?.stopEdgeScroll();
        const logicalX = payload.x;
        const logicalY = payload.y;

        // Clear the OS-drop insertion caret on drop.
        {
          const view = cmEditor?.getView();
          if (view) view.dispatch({ effects: setOsDropCaret.of(null) });
        }

        const cmRect = document.getElementById('moss-editor-cm')?.getBoundingClientRect();
        const isOverEditor = cmRect && logicalX >= cmRect.left && logicalX <= cmRect.right && logicalY >= cmRect.top && logicalY <= cmRect.bottom;

        if (isOverEditor && cmEditor) {
          osDragIn?.clearOsDragHighlight();
          // A drop is work in the editor, and Tauri swallows the DOM drop the
          // pane's lead listeners would otherwise see.
          leadFromEditor();
          if (fileTree?.hasActiveInternalDrag()) {
            // Flow 3 (tree→editor embed): an in-app row/breadcrumb drag
            // released over the editor. payload.paths is EMPTY for an
            // internal drag (no real OS file to copy), and cm-drop.ts's DOM
            // dragover/drop handlers are subject to the same
            // drag_drop_handler interception as file-tree's (see the 'over'
            // branch above), so this Tauri-driven route is the ONLY way the
            // gesture ever reaches the editor. Insert only into markdown
            // documents — a `![[name]]` embed dropped into e.g. an open
            // .css file would just be garbage bytes.
            const activePath = currentFilePath();
            if (activePath && isMarkdownFile(activePath)) {
              const targets = fileTree.resolveDropEntries(
                fileTree.activeInternalDragEntryIds(),
              );
              if (await insertEntryEmbedsAtCoords(cmEditor.getView(), targets, destinationFor, logicalX, logicalY)) {
                // Persist the embed IMMEDIATELY (same contract as the OS-drop
                // twin handleEditorFileDrop): the drop is a completed user
                // gesture, so hit disk now — one watcher rebuild sees the
                // embed — instead of waiting out the autosave debounce.
                await flushPendingSave();
              }
            }
            // Consume the drag session even when nothing was inserted
            // (non-markdown doc / stale-unresolvable IDs): the gesture is
            // over, and an armed session would misroute a following drop
            // inside the drag session's dragend grace window. Also
            // clears the tree's drop-target visuals — the same cleanup the
            // tree-target path gets via handleInternalDrop → performDrop.
            fileTree.clearInternalDragSession();
          } else {
            await handleEditorFileDrop(payload.paths, { x: logicalX, y: logicalY });
          }
        } else if (fileTree?.hasActiveInternalDrag()) {
          // See the 'over' branch above for why this takes priority over
          // osDragIn: an in-app drag's native session is intercepted by
          // Tauri before DOM drop ever fires, and osDragIn.handleOsDrop
          // would silently no-op on it (payload.paths is empty).
          fileTree.handleInternalDrop(logicalX, logicalY);
        } else if (osDragIn) {
          // Position-based hit-testing → copyFilesToProject (not legacy handleExternalDrop)
          await osDragIn.handleOsDrop(payload.paths, logicalX, logicalY);
        }
      } else if (payload.type === 'leave') {
        showDropZone(false);
        // Stop edge-scroll when OS drag leaves the window.
        fileTree?.stopEdgeScroll();
        osDragIn?.clearOsDragHighlight();
        // Clear the OS-drop insertion caret on leave.
        {
          const view = cmEditor?.getView();
          if (view) view.dispatch({ effects: setOsDropCaret.of(null) });
        }
      }
    })
  );

  // The other half of the same conversation: the user opened a chip, so tell
  // the shell to point the preview at whatever that field renders as. The chip
  // bar raises a DOM event (it knows nothing about the shell); this is the one
  // place that turns it into a cross-webview message.
  document.addEventListener('editor:chip-opened', (e) => {
    const field = (e as CustomEvent<{ field: string }>).detail?.field;
    if (field) uiHost.bus.emit('editor:reveal-field-in-preview', { field });
  });

  // The glance strength of the same conversation: hovering a chip (past the
  // chip bar's intent delay) flashes what the field renders as in the preview;
  // `field: null` is hover-out and clears the wash. Same relay shape as
  // chip-opened — the chip bar raises a DOM event, this turns it into the
  // cross-webview message.
  document.addEventListener('editor:chip-hover', (e) => {
    const field = (e as CustomEvent<{ field: string | null }>).detail?.field ?? null;
    uiHost.bus.emit('editor:flash-field-in-preview', { field });
  });

  // One round trip for the whole batch (see the pendingListeners comment
  // above). All listeners are registered once this await resolves — before
  // the composer mount / initial loadFile at the end of init.
  unlistenFns.push(...(await Promise.all(pendingListeners)));

  // The eval-driven open entry point. Rust's
  // open_editor evals `__moss_editor_open(node)` against the (just-shown)
  // warm panel instead of navigating — eval executes even in a hidden/
  // suspended WKWebView (spike-verified), unlike spontaneous emit-driven
  // timer work. If the eval lands BEFORE this line
  // runs, the Rust-side IIFE stashed the node on window.__moss_pending_open
  // (same document — globals survive into init); consumed at the end of init.
  (window as unknown as { __moss_editor_open?: (node: EditorNodeWire) => void })
    .__moss_editor_open = (node: EditorNodeWire) => { void openFromWarmReopen(node); };

  // Wire the editor heading element. Dual-mode: commits route by
  // the HeadingSource captured from the last compute() — Title-source edits
  // write the `title:` frontmatter (mirroring the preview), Filename-source
  // edits rename the file. The saved
  // record is cleared on loadFile and on compute error; commits dispatched
  // before the initial compute return an error (nothing has told the heading
  // which file it describes, so its source is not yet trustworthy).
  // Shared by the heading commit below AND the create-flow session commit:
  // route renames through the SAME FileTree path the inline-tree-rename uses
  // (renameThroughTree → commitRename → renameEntryWithRefs + lifecycle).
  const renameViaTree = async (oldAbs: string, newAbs: string) => {
    if (!fileTree) {
      // Defensive only — fileTree is constructed unconditionally in
      // editorBootstrap whenever the nav DOM exists, well before any commit
      // can fire, so this is not a real document-mode/project-mode branch.
      // Still routed through the link-updating command (never the plain
      // rename_entry, which skips project-wide ref rewriting): a heading
      // rename that quietly breaks incoming links is not an acceptable
      // fallback just because there is no tree to paint a toast against.
      const result = await engine.renameEntryWithRefs(oldAbs, newAbs);
      return result.status === 'ok'
        ? ({ status: 'ok', data: null } as const)
        : ({ status: 'error', error: result.error } as const);
    }
    try {
      await fileTree.renameThroughTree(oldAbs, newAbs);
      return { status: 'ok', data: null } as const;
    } catch (e) {
      return { status: 'error', error: e instanceof Error ? e.message : String(e) } as const;
    }
  };
  // Title-mode commit: write the title: field via the chip bar's store. The
  // frontmatterStore.subscribe handler recomputes the heading from the new
  // value, so the displayed text reconciles automatically. Flash the title
  // chip on the next frame (the store re-renders chips synchronously on
  // setField; the chip exists by then). Capture the current chipBar so a file
  // switch within the frame doesn't flash the next file's chip.
  const setTitleField = (value: string) => {
    const cb = chipBar;
    cb?.setFieldValue('title', value);
    requestAnimationFrame(() => { if (cb === chipBar) cb?.flashChip('title'); });
  };

  headingController = wireEditorHeading({
    getCurrentPath: currentFilePath,
    // Identity, per the project-wide rule: the path moves on rename, the
    // EntryId does not — and the heading's orphan check has to survive exactly
    // that, since renaming the open file is what committing a title DOES.
    getCurrentId: currentEntryId,
    // Obsidian inline-title parity: Enter (or ArrowDown at the title's end)
    // moves the caret DOWN into the CM6 body at document start. Focus +
    // selection ONLY — never a document change (CM6 external-update invariant:
    // doc changes must route through applyExternal()).
    focusBody: () => {
      const view = cmEditor?.getView();
      if (!view) return;
      if (editorCommitLocked) bodyFocusOwed = true;
      else view.focus();
      view.dispatch({ selection: { anchor: 0 } });
    },
    onCommit: async (newText) => {
      const path = currentFilePath();
      if (!path) return { ok: false, error: t('heading.no_file_open') };
      // Untitled-first session (create-flow.ts): while the created file is
      // still title-only, the committed H1 names the file through the
      // session's generated-name rename (date-prefix inference included),
      // bypassing source-based routing. Ends on the first body edit.
      if (createFlow?.isActiveForCurrent()) {
        return createFlow.commitTitle(newText);
      }
      const result = await routeHeadingCommit({
        newText,
        currentFilePath: path,
        projectRoot: projectRoot ?? '',
        source: headingController?.saved()?.source ?? 'Filename',
        headingStateLoaded: headingController?.saved() != null,
        // Through the tree's own rename (renameThroughTree → commitRename), so a
        // heading rename rewrites links project-wide, keeps the entry's
        // identity and fires the rename lifecycle like any other rename.
        renameEntry: renameViaTree,
        registerPendingRename: (from, to) => pendingRenames.register(from, to),
        setTitleField,
      });
      // The tree's rename updates the editor in place; a file-selected here
      // would reload it. Only the tree-less path, which fires no lifecycle, needs it.
      if (result.ok && result.renamedTo && !fileTree) {
        document.getElementById('moss-editor-root')!.dispatchEvent(
          new CustomEvent('editor:file-selected', {
            bubbles: true,
            detail: { filePath: result.renamedTo },
          }),
        );
      }
      // Narrow to HeadingCommitResult for the controller (drop renamedTo).
      return result.ok ? { ok: true } : { ok: false, error: result.error };
    },
  });

  // Untitled-first create flow: ＋/▾ buttons in .nav-right, ⌘N/⇧⌘N, the
  // session state machine and the status line (create-flow.ts). Shares
  // renameViaTree + setTitleField with the heading commit path above.
  createFlow = new CreateFlow({
    projectRoot: () => projectRoot ?? '',
    fileTree: () => fileTree,
    openNewEntry: (rel) => openSelectedEntry(rel, true),
    currentFilePath: () => currentFilePath(),
    upsert: (rel) => registry?.upsert(rel, false) ?? null,
    pathOf: (id) => registry?.pathOf(id) ?? null,
    registerPendingRename: (from, to) => pendingRenames.register(from, to),
    renameEntry: renameViaTree,
    setTitleField,
    setHeadingSaved: (text) => headingController?.noteSavedText(text),
    listDirectory: (absDir) => engine.listDirectory(absDir, true),
    createFile: (absDir, name) => createOneFile(engine, absDir, name),
    instantiateTemplate: (id, absDir, name) => engine.instantiateTemplate(id, absDir, name),
    deleteEntry: (absPath) => engine.deleteEntry(absPath),
    // The template list / rename / delete seam — the same object the tree
    // narrows for Save as Template…
    templates: boot.data.project_path ? engine : null,
    languages: engine,
    createTranslation: (sourceAbs, lang) => engine.createTranslation(sourceAbs, lang),
  });
  createFlow.wire();
  // Shell-forwarded ⌘N/⇧⌘N (keydown never crosses webviews — see
  // app.ts initializeKeyboardShortcuts; the same seam keeps the tauri event
  // API out of create-flow.ts).
  unlistenFns.push(uiHost.bus.on('editor:request-new-page', () => { void createFlow?.newPage(); }));
  unlistenFns.push(uiHost.bus.on('editor:request-new-folder', () => { createFlow?.newFolder(); }));
  // Same requests as DOM events, dispatched from within THIS webview (the
  // file tree's empty-area context menu). Creation stays owned by the one
  // CreateFlow instance — surfaces only ever ask for it.
  document.addEventListener('editor:request-new-page', () => { void createFlow?.newPage(); });
  document.addEventListener('editor:request-new-folder', () => { createFlow?.newFolder(); });

  // Clean up listeners when this webview is destroyed
  window.addEventListener('beforeunload', () => {
    scaleAbort.abort();
    createFlow?.destroy();
    createFlow = null;
    unlistenFns.forEach((fn) => fn());
    if (fileTree) {
      fileTree.unmount();
      fileTree = null;
    }
    previewFollower?.stop();
    previewFollower = null;
    osDragIn = null;
    // Stop the coordinator (tears down the ONE FileChanged + AssetReady cross-webview
    // subscriptions and clears all registered handlers). Must run after fileTree.unmount()
    // since unmount calls the file-tree's unregister fn synchronously.
    void fileEvents?.stop();
    fileEvents = null;
    composerHandle?.destroy(); // flush the draft + destroy the composer's CM6
    composerHandle = null;
    versionsMode.unmount();
  });

  // Silent-save data-safety: flush
  // pending edits when the editor loses focus or the app backgrounds, so the 2s
  // autosave debounce window can't be stranded by an OS kill / suspend. On macOS
  // a window close blurs the webview before teardown, so this also flushes on
  // the common close path. flushPendingSave is idempotent and a no-op when
  // clean; now that saves are silent, an early autosave is invisible.
  const flushOnHidden = () => {
    if (document.visibilityState === 'hidden') void flushPendingSave();
  };
  const flushOnBlur = () => { void flushPendingSave(); };
  document.addEventListener('visibilitychange', flushOnHidden);
  window.addEventListener('blur', flushOnBlur);
  unlistenFns.push(() => {
    document.removeEventListener('visibilitychange', flushOnHidden);
    window.removeEventListener('blur', flushOnBlur);
  });

  // Load the initial source file from the URL query parameter (read at the
  // top of init — the source path is baked into the editor URL by
  // open_editor() so it's available immediately, no event race with the
  // main window).
  if (composeArticleId) {
    await mountEmailComposer(composeArticleId, params.get('deployed_url') ?? '');
    return;
  }
  if (initialSource) {
    // boot.data.initial_file is the bootstrap's best-effort pre-parse of this
    // exact source; loadFile falls back to parseFrontmatter when it's null.
    await loadFile(initialSource, boot.data.initial_file ?? undefined);
  } else if (fileTree && projectRoot) {
    const { node, homeFromDisk } = readEditorNode(editorNodeFromQuery(params));
    if (homeFromDisk) {
      // The root's home resolves from disk even mid-build, so it needs no shell answer.
      if (currentEntryId() === null) await resolveAndApplyRootSurface();
    } else {
      // A prewarmed webview has no page identity; it stays neutral until the
      // shell answers the handshake below.
      const surface = decideEditorSurface(node);
      if (surface.kind !== 'content' && currentEntryId() === null) applyEditorSurface(surface);
    }
  }
  if (!initialSource) uiHost.bus.emit('editor:request-page', {});

  // Consume a warm-open that raced init (the eval-or-stash contract above).
  const w2 = window as unknown as { __moss_pending_open?: EditorNodeWire };
  if (w2.__moss_pending_open) {
    const pending = w2.__moss_pending_open;
    delete w2.__moss_pending_open;
    await openFromWarmReopen(pending);
  }
}

/**
 * THE single owner of the `#chip-bar` visibility. Chip-bar visibility is a pure
 * function of the mounted surface: visible IFF the editor is editing a markdown
 * file (a folder's home page included); hidden for every non-content surface
 * (takeover / root-empty / generated / pending / email) and for
 * non-markdown / non-text files. Hiding also empties it so its `border-bottom`
 * can't paint as a stray second hairline under the title bar in no-file states
 * (issue #4). All former ad-hoc `chipBarEl.style.display` writes route here.
 * Null-safe for the test harness DOMs that don't declare `#chip-bar`.
 */
/** The composer and Versions mode both own the panel: the tree, the create
 * button and the window caption are editor-only chrome — the breadcrumb
 * strip is NOT: it is replaced wholesale via `FileTree.setTitleBar`, a navigation stack.
 * Called pre-paint at boot on the ?compose= path, again by
 * mountEmailComposer, and by versions-mode.ts on entry (paired with
 * `showEditorChromeForSurface` below — leaving Versions mode never reloads
 * the page). */
function hideEditorChromeForSurface(): void {
  const heading = document.getElementById('editor-heading');
  if (heading) heading.style.display = 'none';
  const navBody = document.querySelector<HTMLElement>('.nav-body');
  if (navBody) navBody.style.display = 'none';
  // FADE, not display:none (round five, chrome-reuse doc item 3) — keeps
  // their boxes so the breadcrumb's width allocation stays untouched.
  for (const id of ['nav-right', 'nav-caption']) {
    document.getElementById(id)?.classList.add('is-chrome-faded');
  }
}

/** The composer's own titlebar replaces the heading and the whole breadcrumb surface;
 * unlike Versions mode it is a fresh document load, so nothing restores them. */
function hideChromeForComposer(): void {
  for (const id of ['editor-heading', 'nav-surface']) document.getElementById(id)?.style.setProperty('display', 'none');
}

/** The inverse of `hideEditorChromeForSurface` — see its own comment. */
function showEditorChromeForSurface(): void {
  const heading = document.getElementById('editor-heading');
  if (heading) heading.style.display = '';
  const navBody = document.querySelector<HTMLElement>('.nav-body');
  if (navBody) navBody.style.display = '';
  for (const id of ['nav-right', 'nav-caption']) {
    document.getElementById(id)?.classList.remove('is-chrome-faded');
  }
}

export function setChipBarVisible(visible: boolean): void { // exported for testing
  const chipBarEl = document.getElementById('chip-bar');
  if (!chipBarEl) return;
  chipBarEl.style.display = visible ? '' : 'none';
  if (!visible) chipBarEl.innerHTML = '';
}

/**
 * Tear down file-mode editor state (CM6, frontmatter form, chip bar, file
 * viewer, toolbar, heading) so the CM slot can be taken over by folder-mode or
 * the folder-page empty-state. Called by `applyEditorSurface` (pre-mount) and
 * `loadFile`.
 */
function teardownFileMode(): void {
  // Reset BEFORE the destroy sequence, never after. ChipBar.destroy closes any
  // open popover, and a close fires the history boundary — which after the
  // generation moved on would stamp the OUTGOING document's edit with the
  // INCOMING one's generation, forging exactly the stale entry the stamp
  // exists to make unforgeable. ChipBar.destroy() suppresses that emit via its
  // own `tearingDown`; this ordering is the other half of the same belt.
  resetDocumentHistory();
  if (cmEditor) { cmEditor.destroy(); cmEditor = null; }
  if (frontmatterStore) { frontmatterStore.destroy(); frontmatterStore = null; }
  if (chipBar) { chipBar.destroy(); chipBar = null; }
  destroyFileViewer();
  // Hide the chip bar via the single owner (empties it too so its border-bottom
  // can't paint as a stray hairline in no-file states — issue #4). Re-shown by
  // loadFile's markdown branch when real chips render.
  setChipBarVisible(false);
  setPreviewNote(null);
  // Heading element belongs to file mode; clear it.
  const headingEl = document.getElementById('editor-heading');
  if (headingEl) headingEl.dataset.empty = 'true';
}

/**
 * Mount the email composer (`?compose=<article-id>&deployed_url=...`), the
 * host's `composer` slot: tear down file mode and hand it the container.
 * navigate-in-place reloads the document, so this runs on a fresh init() and
 * the chrome hidden for it never leaks to the next editor load.
 */
async function mountEmailComposer(articleId: string, deployedUrl: string): Promise<void> {
  teardownFileMode();
  const cm = document.getElementById('moss-editor-cm');
  const mount = host.composer;
  if (!cm || !projectRoot || !mount) {
    console.error('[mountEmailComposer] missing #moss-editor-cm, projectRoot or a composer');
    return;
  }
  // Chip bar is hidden by teardownFileMode above; the rest of the editor chrome
  // was hidden pre-paint at boot (hideChromeForComposer).
  cm.setAttribute('data-editor-surface', 'email-composer');
  composerHandle?.destroy();
  composerHandle = await mount(cm, { projectRoot, articleId, deployedUrl });
}

/**
 * Mount the surface chosen by `decideEditorSurface`. THE single mount path for
 * every non-content surface — boot, file-tree, and preview-nav all route here.
 * Owns teardown + the idempotency guard (keyed on surface kind via the
 * `data-editor-surface` marker on the CM container) so no trigger churns the
 * DOM for a state already shown. `content` is NOT handled here — its caller
 * runs the loadFile dedup (shouldLoadContentPage).
 */
/**
 * Flash the editor breadcrumb (#nav-surface) green to signal new content
 * arriving (e.g. an import landing files) — the editor-side analogue of the
 * preview nav-pill flash. Restart-safe (remove → reflow → add) so
 * repeated arrivals re-trigger the animation. Uses the existing borderless
 * `.moss-just-committed` ambient pulse — `--moss-color-accent` mixed into the
 * surface, matching the nav-pill "build applied" flash (editor.css).
 */
function flashNavSurface(): void {
  const el = document.getElementById('nav-surface');
  if (!el) return;
  el.classList.remove('moss-just-committed');
  void el.offsetWidth; // force reflow to restart the animation
  el.classList.add('moss-just-committed');
}

/**
 * Restart-safe green `.moss-just-committed` flash on an arbitrary element.
 * Mirrors flashNavSurface / flashBreadcrumbSegment so every "confirmation"
 * pulse shares the same vocabulary as the preview nav-pill.
 */
function flashElement(el: HTMLElement): void {
  el.classList.remove('moss-just-committed');
  void el.offsetWidth; // force reflow so the next class add restarts the animation
  el.classList.add('moss-just-committed');
}

/**
 * Load a content page's source into the editor iff it isn't already mounted —
 * the shared dedup for the two root-resolve legs (resolveAndApplyRootSurface +
 * applyResolvedNode). Identity is compared by stable EntryId when both sides are
 * known, else by relative path, so a file that landed during a mid-resolve await
 * is never stomped (see shouldLoadContentPage).
 *
 * Two different questions get two different answers, because a caller that
 * navigates the preview has to distinguish them:
 *  - `null` — nothing was attempted: the file is ALREADY the open document, so
 *             the dedup skipped it.
 *  - `{ path, outcome }` — a load ran; `outcome` is loadFile's verdict. Only
 *             `'opened'` means the editor is now showing `path`. `'failed'` and
 *             `'superseded'` mean it is not, and a caller must not navigate the
 *             preview there (see LoadOutcome's docstring for why flattening
 *             the three into a boolean breaks this).
 *
 * Always force-anchors the file-tree breadcrumb (`{ forceAnchor: true }`).
 * Every caller reaching this function is an EXTERNAL trigger relative to the
 * tree — applyResolvedNode's callers are preview-nav (editor:load-source) and
 * warm reopen (openFromWarmReopen), and resolveAndApplyRootSurface is the
 * cold-boot root resolve — never the in-tree row-click path (editor:file-
 * selected calls loadFile directly, bypassing this function). Forcing anchor
 * on warm reopen / cold boot is a harmless no-op: the tree has nothing open
 * yet at that point, so there is no frozen anchor to diverge from.
 */
async function loadContentPageDeduped(
  sourcePathAbs: string,
): Promise<{ path: string; outcome: LoadOutcome } | null> {
  const rel = toRelative(sourcePathAbs, projectRoot ?? '');
  const entryId = registry?.getByPath(rel) ?? null;
  // In Versions mode the registry still names the document the list replaced,
  // so "same file" would skip the load and leave the list standing.
  const sameFile = !versionsMode.isActive() && ((entryId !== null && currentEntryId() !== null)
    ? entryId === currentEntryId()
    : rel === currentFilePath());
  if (shouldLoadContentPage(sameFile, cmEditor !== null || fileViewerIsOpen())) {
    return { path: rel, outcome: await loadFile(rel, undefined, { forceAnchor: true }) };
  }
  return null;
}

/**
 * Source-truth root resolve. Ask Rust (`resolvePageSource('/')`) what the
 * site home is rather than hardcoding a surface. Builds a ResolvedNode from the
 * real home `source_path` and routes through the SAME decideEditorSurface every
 * other trigger uses: home file present → OPEN it (loadFile); empty folder →
 * root-empty (4 cards); folder-with-content-but-no-home → the root-home
 * takeover the resolver computed.
 *
 * Caller guarantees no file is currently open (so we never stomp an active
 * edit). A failed resolve retains the current surface, never throws.
 */
async function resolveAndApplyRootSurface(folderPath = projectRoot): Promise<void> {
  const request = loadGeneration.bump();
  const url = folderPath && folderPath !== projectRoot ? await engine.resolveUrlForFolder(folderPath) : null;
  const urlPath = folderPath === projectRoot ? '/' : url?.status === 'ok' ? url.data : null;
  if (!urlPath || loadGeneration.isStale(request)) return;
  // Explicit tree/root requests resolve from disk. On failure the unresolved
  // node retains the current surface; it cannot establish a generated page.
  const node = await resolveNodeForEdit(urlPath, (u) => engine.resolvePageSource(u));
  if (loadGeneration.isStale(request)) return;
  const surface = decideEditorSurface(node);
  if (surface.kind === 'content') {
    // Dedup shared with applyResolvedNode. Point the preview follower at the
    // home ONLY when the editor really opened it: a cold boot on a home file
    // that is iCloud-evicted or otherwise unreadable fails the open, leaves a
    // non-content surface in the panel, and must leave the preview where it is
    // — the preview follows the editor, or the two halves of the screen show
    // different documents with nothing saying which is real.
    const loaded = await loadContentPageDeduped(surface.sourcePath);
    if (loaded?.outcome === 'opened') void previewFollower?.navigate(loaded.path);
    return;
  }
  // Non-content surface: only assert it when no file is open, so a file that
  // landed during the await keeps the pane (parity with the boot/load-source
  // currentEntryId guards).
  if (currentEntryId() === null) applyEditorSurface(surface);
}

/**
 * FIX 4 — flash the DEEPEST currently-visible container for each new file.
 *
 * For every distinct resolved target, apply the green `.moss-just-committed`
 * pulse ONCE per event tick (coalesced — never strobe). Resolution order for a
 * new file's deepest visible container:
 *   1. a file-tree folder ROW (`.nav-body .seg[data-path]`) for the file's
 *      parent or its deepest rendered ancestor, when the tree is expanded and
 *      that row is on screen;
 *   2. ELSE the breadcrumb SEGMENT (`.nav-bc-strip .seg.bc[data-path]`) for the
 *      deepest shown ancestor;
 *   3. ELSE the whole nav-surface.
 *
 * Viewport-only: never mutates tree expansion overrides. `paths` are
 * project-root-relative source paths (the FileChanged wire domain).
 */
let pendingDownloadFlashPaths: string[] = [];
let downloadFlashTimer: ReturnType<typeof setTimeout> | null = null;

function flashDownloadTargets(paths: string[]): void {
  if (paths.length === 0) return;
  // DEBOUNCE to the rebuild boundary. An import writes many files across several
  // rebuild ticks, EACH of which re-renders the breadcrumb/tree. Flashing
  // synchronously paints a pulse onto DOM the very next render replaces — the
  // user sees nothing (verified live: stickyFlash:0 after a 42-file import).
  // Accumulate every arrival and flash ONCE, ~400ms after the last one, against
  // the SETTLED DOM: one calm green pulse per distinct container that gained
  // files, never a strobe.
  // [diag] green-flash: when a batch of created paths ARRIVES. The gap between
  // the first of these and the flush log below = 400ms debounce + burst length;
  // the absolute time of the FIRST one (relative to the import) shows why the
  // pulse "only shows after a while" (no RawFileCreated/source_creates fires
  // during the initial import build — the watcher starts after it).
  console.info(`[diag] green-flash: +${paths.length} created path(s) arrived (pending=${pendingDownloadFlashPaths.length + paths.length})`);
  pendingDownloadFlashPaths.push(...paths);
  if (downloadFlashTimer) clearTimeout(downloadFlashTimer);
  downloadFlashTimer = setTimeout(flushDownloadFlash, 400);
}

/** Resolve + flash the accumulated download targets against the settled DOM.
 *  Exported for tests to flush the debounce synchronously. */
export function flushDownloadFlash(): void {
  downloadFlashTimer = null;
  const paths = pendingDownloadFlashPaths;
  pendingDownloadFlashPaths = [];
  if (!fileTree || paths.length === 0) return;
  // Coalesce by resolved target element so a 40-file import flashes each
  // distinct container once, not 40 times. Driven by both source_creates
  // (FileChanged process-hook path) and RawFileCreated (watcher raw path).
  const targets = new Set<HTMLElement>();
  let fellBackToSurface = false;
  for (const p of paths) {
    const parent = parentDirOf(p);
    const target = fileTree.deepestVisibleContainerFor(parent);
    if (target) {
      targets.add(target);
    } else {
      fellBackToSurface = true;
    }
  }
  for (const el of targets) flashElement(el);
  // Any arrival with no visible tree-row/breadcrumb ancestor falls back to the
  // whole nav-surface (e.g. nothing expanded, or the file lives outside every
  // shown branch). One flash regardless of how many such arrivals there were.
  if (fellBackToSurface || targets.size === 0) flashNavSurface();
  // [diag] green-flash: the pulse actually paints NOW (400ms after the last
  // arrival). container flashes + optional surface fallback.
  console.info(`[diag] green-flash: FLUSH → flashed ${targets.size} container(s)${fellBackToSurface || targets.size === 0 ? " + nav-surface" : ""} (from ${paths.length} path(s))`);
}

/** Show the folder a deleted document lived in (the editor closes). */
async function showFolderOfDeleted(deletedSource: string): Promise<void> {
  const request = loadGeneration.current();
  await fileTree?.refreshAndRender();
  if (loadGeneration.isStale(request) || !projectRoot) return;
  await resolveAndApplyRootSurface(toAbsolute(parentDirOf(deletedSource), projectRoot));
}

/**
 * Parent directory of a project-root-relative path, in the registry domain.
 * Returns the empty string (project root) for a top-level file. Separator-
 * agnostic to match the registry's native-separator keys.
 */
function parentDirOf(relPath: string): string {
  const lastSep = Math.max(relPath.lastIndexOf('/'), relPath.lastIndexOf('\\'));
  return lastSep >= 0 ? relPath.slice(0, lastSep) : '';
}

/**
 * Tell the shell whether a still-downloading file is on screen, and which one.
 *
 * The editor cannot watch the cloud — the shell owns the one `icloud-sync`
 * listener — so the arrival retry lives there and this is its only input. Both
 * directions ride the one event (see SOURCE_PENDING_EVENT): a separate
 * "stop watching" channel could overtake an arm emitted just before it, leaving
 * the shell re-offering a file the user has already navigated away from.
 *
 * `sourcePath` is absolute because the shell hands it straight back through
 * `editor:load-source`, whose domain is the resolver's.
 */
function announceSurface(surface: Exclude<EditorSurface, { kind: 'content' }>): void {
  // A page with no answer is the shell's to settle: no bytes are awaited, so a
  // retry armed for the page it replaces is disarmed.
  const payload: SourcePendingPayload = surface.kind === 'pending' && surface.reason === 'download'
    ? { pending: true, sourcePath: surface.sourcePath }
    : { pending: false, sourcePath: null };
  uiHost.bus.emit(SOURCE_PENDING_EVENT, payload);
}

/**
 * A page with no answer yet keeps a document, the boot skeleton or a waiting
 * pane, with its unsaved text, autosave and undo; it replaces only a claim
 * about some other page.
 */
function keepsCurrentSurface(surface: EditorSurface): boolean {
  if (surface.kind !== 'pending' || surface.reason !== 'lookup') return false;
  const shown = document.getElementById('moss-editor-cm')?.getAttribute('data-editor-surface') ?? null;
  if (shown === null) return true;
  // A pane waiting on a named file is replaced: its retry would load that file
  // over whatever page the preview has moved to. One waiting on the page
  // re-asks the live page, so it stays.
  return shown === 'pending' && lastSurface?.kind === 'pending' && (lastSurface.reason === 'lookup' || lastSurface.sourcePath === null);
}

function applyEditorSurface(surface: Exclude<EditorSurface, { kind: 'content' }>): void {
  const cm = document.getElementById('moss-editor-cm');
  if (!cm || !projectRoot) return;

  if (keepsCurrentSurface(surface)) return;
  // BEFORE the idempotency gate, deliberately. The shell needs to know what is
  // ON SCREEN, and the gate below answers a different question — whether
  // anything had to be re-mounted to put it there. A retry that lands back on
  // the downloading pane changes nothing visually and must still re-arm the
  // shell, or the one arrival that finally delivers the file finds nobody watching.
  announceSurface(surface);

  // Idempotency: same surface already shown → skip teardown/remount. A
  // takeover is keyed by its recipe, so a different folder still re-renders.
  const surfaceKey = surface.kind === 'takeover' ? JSON.stringify(surface.takeover)
    : surface.kind === 'root-empty' ? JSON.stringify(surface.folder)
    : surface.kind === 'pending' ? surface.reason : '';
  if (
    cm.getAttribute('data-editor-surface') === surface.kind
    && cm.getAttribute('data-editor-surface-key') === surfaceKey
  ) return;

  // Tear down whatever is mounted; each renderer below also self-clears, but
  // do it once here so the slot is empty before the chosen renderer mounts.
  teardownFileMode();
  unmountFolderMode();
  unmountPaneState(cm);
  versionsMode.unmount();
  cm.innerHTML = '';
  lastSurface = surface;

  if (surface.kind === 'root-empty') {
    showFolderMode(surface.folder);
  } else {
    // takeover / pending / generated are all pane-states; telling them apart
    // is the whole job of this pane, so their copy lives together.
    mountNoContentPane(cm, surface, { takeOver: (t) => void takeOver(t) });
  }
  cm.setAttribute('data-editor-surface', surface.kind);
  cm.setAttribute('data-editor-surface-key', surfaceKey);
  // Observability (issue #5b): these surfaces are NOT a CodeMirror document, so
  // "can't type here" is by design. Logging which one is shown distinguishes a
  // genuine non-editable-editor bug from the user simply landing on a folder /
  // empty pane.
  void logInfo(`[editor] showing non-file surface="${surface.kind}" (no editable CM6 document)`);
  // The editor now represents this surface, not the last-opened file — clear
  // the stale tree highlight so it doesn't contradict the pane.
  fileTree?.clearSelection?.();
}

function showFolderMode(folder: Takeover): void {
  // File-mode already torn down by applyEditorSurface (the only caller).
  const cmContainer = document.getElementById('moss-editor-cm');
  if (!cmContainer || !projectRoot) return;

  // The backend recipe names the selected folder and its creation target.
  const folderName = folder.display;
  const folderPath = folder.files[0].dir;

  const deps = {
    folderPath,
    folderName,
    createHome: folder.kind === 'folder-home' ? () => takeOver(folder) : undefined,
    loadFile: (path: string) => revealCreatedEntry(path.startsWith('/') ? path : `${folderPath}/${path}`),
    host: cmContainer,
  };
  mountFolderMode(cmContainer, {
    folderPath,
    folderName,
    isEmpty: true,
    // All content cards reveal and open their home through the create flow and
    // transition to file mode; Try an example shows the starter
    // picker; URL drives the scraper; Plugin emits a custom event the
    // app shell uses to surface the importable-plugin catalog. See
    // `folder-mode-handlers.ts` for the per-card flows.
    onCardClick: makeCardClickHandler(deps),
  });
  // The launcher's New from starter… made this folder to choose a starter in.
  // A failed ask leaves the four cards, which is the ordinary empty folder.
  void Promise.resolve(uiHost.starterPicker?.takeRequest(folderPath))
    .then((open) => { if (open) void dispatchCard('try-example', deps); })
    .catch(() => {});
}


// The SINGLE takeover action: write the recipe's files in one call, then
// reveal the last one across EVERY surface — the file tree (row), the
// breadcrumb (active selection), the editor (CM6, opened AND focused), and the
// preview (navigate). Shared by the pane's button (any takeover kind), the
// file-tree folder context-menu "Create home file" item, and folder
// navigation. The recipe itself is the backend's (takeover.rs) except for a
// folder the tree names by real path, where `folderHomeTakeover` builds the
// same self-named home note — which `moss_core::home::is_home_file` elects as
// the folder's home at the root and in subfolders alike.
async function takeOver(takeover: Takeover): Promise<void> {
  if (!projectRoot) return;
  if (takeover.kind === 'root-home') {
    const host = document.getElementById('moss-editor-cm');
    if (!host) return;
    await dispatchCard('start-writing', { folderPath: projectRoot, folderName: rootName ?? '', host, loadFile: revealCreatedEntry });
    return;
  }
  const result = await engine.createFiles(takeover.files);
  if (result.status !== 'ok') {
    showToast({ what: t('tree.create_failed').replace('{error}', result.error), severity: 'Blocking' });
    return;
  }
  // The last file is the one to open: tree first, then re-anchor, then open,
  // the file-tree inline create's sequence, via the shared `revealAndOpenNewEntry`.
  const created = result.data[result.data.length - 1];
  if (created) await revealCreatedEntry(toRelative(created, projectRoot));
}

/** Created and installed pages share tree anchoring, focus and preview following. */
async function revealCreatedEntry(path: string): Promise<void> {
  await revealAndOpenNewEntry(fileTree, toRelative(path, projectRoot ?? ''), (rel) => openSelectedEntry(rel, true));
}

/** The file-tree event and awaited create flows enter the same open action. */
async function openSelectedEntry(filePath: string, isNewFile = false): Promise<void> {
  // The one gate for a user-chosen open (tree click and keys, wikilinks, chip
  // links). A restored or preview-followed file never dispatches
  // `editor:file-selected`, so it is not asked about again.
  if (!isNewFile && !(await mayOpenSpecialFile(registry?.recordAt(toRelative(filePath, projectRoot ?? ''))?.special, askAboutSpecialFile))) {
    reassertOpenDocument();
    return;
  }
  if (await loadFile(filePath) !== 'opened') return;
  const loadedPath = toRelative(filePath, projectRoot ?? '');
  const id = registry?.getByPath(loadedPath) ?? null;
  // A newer selection may supersede this open before its caller resumes.
  if (id === null || id !== currentEntryId()) return;
  const category = detectFileCategory(filePath);
  if (category === 'text') {
    // Untitled-first pages focus the heading; homes/templates focus the body.
    // Ordinary navigation never steals focus from the preview or file tree.
    if (isNewFile && !createFlow?.consumeHeadingFocus(loadedPath)) cmEditor?.getView()?.focus();
    await previewFollower?.navigate(filePath, isNewFile ? engine.resolveExpectedUrlForFile : undefined);
  } else {
    const result = await engine.resolveAssetUrlForFile(toAbsolute(filePath, projectRoot ?? ''));
    if (id === currentEntryId() && result.status === 'ok' && result.data) {
      uiHost.bus.emit('editor:navigate-preview', { urlPath: result.data });
    }
  }
}

// ── File Loading ───────────────────────────────────────────────────────

/**
 * Flush any pending auto-save synchronously, preserving unsaved edits.
 *
 * Called from two paths:
 *  1. File switch (loadFile) — prevents losing edits for the file we're
 *     navigating away from.
 *  2. The host's save request (panel close, quit) — prevents losing edits
 *     when the settings modal or caller closes the editor webview during
 *     the 2s auto-save debounce.
 *
 * Idempotent and safe to call when nothing is pending.
 *
 * Returns `true` when the write this flush forced (or the in-flight write it
 * awaited) succeeded, `false` when that write erred. A pure no-op flush
 * (nothing dirty, no store-pending, no in-flight save) returns `true`
 * regardless of any stale prior `'error'` — the prior failure already shows
 * its badge; reporting it here would trap the shutdown coordinator (and the
 * user) behind an un-closable window. Only `false` means "this flush forced a
 * write and that write failed" (MF-2, the load-bearing correctness fix).
 */
async function flushPendingSave(): Promise<boolean> {
  // Cancel both pending debounces and, if anything was dirty/queued, route a
  // single save through the one writer (it reads path + body + frontmatter
  // fresh at write time). The store's debounced frontmatter edits and the
  // CM6 body debounce both collapse into this one saveActiveDocument call.
  // Read before cancelling the store debounce, which clears the pending write.
  const needsWrite = unsavedAwareSaveState() === 'dirty';
  if (saveTimeout) { clearTimeout(saveTimeout); saveTimeout = null; }
  frontmatterStore?.cancelPendingWrite();

  const inFlight = saveInFlight;

  if (!needsWrite && !inFlight) {
    // Pure no-op — nothing to settle. Do NOT report a stale prior 'error'.
    return true;
  }

  if (needsWrite) {
    // Force a write through the one writer; it coalesces with any in-flight
    // save and re-runs if a change landed mid-write. Sets lastWriteOk.
    await saveActiveDocument('forced');
  } else {
    // Nothing newly dirty, but a save is still in flight — await it so the
    // write settles (and lastWriteOk is current) before we report. Today's
    // bug: this path returned early without awaiting.
    await inFlight;
  }

  // lastWriteOk reflects the write we just forced/awaited. If somehow still
  // null (no write actually ran), treat as success — see the no-op contract.
  return lastWriteOk ?? true;
}

/**
 * Route a node through the single surface decision and mount it — the shared
 * body of the 'editor:load-source' listener and the eval-driven open entry
 * point (window.__moss_editor_open). Re-bakes an answer into the document
 * query (history.replaceState) so an HMR full reload or the navigate fallback
 * stays consistent with the live surface.
 */
async function applyResolvedNode(wire: EditorNodeWire): Promise<void> {
  let { node, homeFromDisk } = readEditorNode(wire);
  // Kept means untouched, a load in flight included: it may be the user's own open.
  if (!homeFromDisk && keepsCurrentSurface(decideEditorSurface(node))) return;
  const request = node.sourcePath && !node.pending ? loadGeneration.current() : loadGeneration.bump();
  if (homeFromDisk) node = await resolveNodeForEdit('/', (u) => engine.resolvePageSource(u));
  if (loadGeneration.isStale(request)) return;
  const surface = decideEditorSurface(node);
  if (!node.pending) history.replaceState(null, '', `${location.pathname}${editorQueryFromNode(node)}`);
  // NOTE: tree-collapse on warm reopen is NOT done here. applyResolvedNode is
  // the SHARED body of BOTH the 'editor:load-source' listener (fired on every
  // in-session preview navigation) AND the warm-reopen entry points. Collapsing
  // here would wrongly reset the tree mid-session on every preview navigate.
  // The collapse lives in openFromWarmReopen() (below), which wraps the two
  // warm-reopen call sites only.
  debug(`[editor] applyResolvedNode surface=${surface.kind}${surface.kind === 'content' ? ` sourcePath=${surface.sourcePath}` : ''}`); // empty-state surfaces look identical whether correct or a stale/failed resolve

  if (surface.kind === 'content') {
    await loadContentPageDeduped(surface.sourcePath);
    return;
  }
  applyEditorSurface(surface);
}

/**
 * A home-less folder named by real path: its recipe is refreshed from disk
 * before choosing cards or a home-page offer, since the build map can lag
 * creation and deletion. A refresh that gets no answer keeps the recipe given.
 */
async function applyFolderFromDisk(takeover: Takeover): Promise<void> {
  const request = loadGeneration.bump();
  const isRoot = takeover.kind === 'root-home';
  const dir = takeover.kind === 'folder-home' ? takeover.files[0]?.dir : undefined;
  const url = dir ? await engine.resolveUrlForFolder(dir) : null;
  const urlPath = isRoot ? '/' : url?.status === 'ok' ? url.data : null;
  const fresh = urlPath ? await resolveNodeForEdit(urlPath, (u) => engine.resolvePageSource(u)) : null;
  if (loadGeneration.isStale(request)) return;
  await applyResolvedNode(fresh && fresh.pending !== 'lookup' ? fresh : { sourcePath: null, isRoot, takeover, pending: null });
}

/**
 * Warm-reopen entry point. The editor webview shell is persistent, so
 * mount() — which resets the view to `initialView()` — does NOT run when the user
 * reopens the editor on a new folder/file. Without this, the previous session's
 * open-folder overrides would persist and the tree would render pre-expanded.
 *
 * collapseForReopen() wipes overrides + park by reassignment (collapseAll)
 * without rendering; the subsequent applyResolvedNode
 * → highlight() → navigateTo re-renders against the now-collapsed state.
 *
 * Only the two warm-reopen call sites (the __moss_editor_open eval and the
 * __moss_pending_open race-consume at end of init) route through here; the
 * in-session 'editor:load-source' listener calls applyResolvedNode directly so
 * it never collapses the tree.
 */
async function openFromWarmReopen(wire: EditorNodeWire): Promise<void> {
  const { node, homeFromDisk } = readEditorNode(wire);
  // A bare page keeps what is open until the shell answers, tree included.
  if ((homeFromDisk || !keepsCurrentSurface(decideEditorSurface(node))) && fileTree) fileTree.collapseForReopen();
  // Reopening on the file already loaded keeps its CM6 instance — and the
  // lead the user left it with. The panel's grow animation is about to reflow
  // it; that must not move the preview the user opened the editor from.
  followPreview();
  await applyResolvedNode(wire);
  // The native open carries no verdict; the shell's answer arrives through the handshake.
  if (!wire.sourcePath && !wire.takeover) uiHost.bus.emit('editor:request-page', {});
  followPreviewPosition();
}

/**
 * Tear down whatever empty-state surface owns the CM slot (folder-mode, the
 * folder-page CTA, a pane-state) right before the slot is repopulated.
 * Folder-mode owns the CM slot until a file is selected; once a file is
 * loading, the slot belongs to the file viewer / CM6 again. The folder-page
 * CTA leaks if the user navigates away by selecting a file (not the CTA).
 *
 * MUST be called in the same synchronous run as the replacement mount —
 * never before an `await`. loadFile used to run this teardown up front and
 * then await the file read, leaving a ~5-20ms window where the cm slot
 * existed with neither skeleton nor content (a white pane — the cold-open
 * probe trips on it). The outgoing surface stays painted until
 * the incoming content is ready; on a failed read we stay on it entirely.
 */
function teardownEmptyStateSurfaces(): void {
  // The other exit from a pending pane, and the one applyEditorSurface never
  // sees: a real document mounting over it. An arm left standing here would
  // re-offer the old file on the next arrival and yank the user out of what
  // they are now editing.
  uiHost.bus.emit(SOURCE_PENDING_EVENT, { pending: false, sourcePath: null } satisfies SourcePendingPayload);
  unmountFolderMode();
  versionsMode.unmount();
  const emptyHost = document.getElementById('moss-editor-cm');
  if (emptyHost) {
    unmountPaneState(emptyHost);
    emptyHost.removeAttribute('data-editor-surface');
    emptyHost.removeAttribute('data-editor-surface-key');
  }
}

/**
 * Show or clear the "invalid YAML frontmatter" banner above the frontmatter
 * chips. This is the editor's live consumer of
 * `ParsedFrontmatter.frontmatter_error` (BUG 3): a malformed `---...---` block
 * yields zero parsed fields, so the chip form renders blank; the banner makes
 * the failure visible instead. Created lazily so it works across the editor
 * HTML and every test harness DOM (none of which declare it). `null`/empty
 * removes it, so switching to a clean file clears a stale banner. Called only
 * AFTER the chip bar has settled into its home for the incoming file (the CM
 * scroller, Amendment III), so `insertBefore` anchors it there and it scrolls
 * with the chips.
 */
function renderFrontmatterErrorBanner(error: string | null | undefined): void {
  let banner = document.getElementById('fm-error-banner');
  if (!error) {
    banner?.remove();
    return;
  }
  if (!banner) {
    banner = document.createElement('div');
    banner.id = 'fm-error-banner';
    banner.className = 'fm-error-banner';
    banner.setAttribute('role', 'alert');
    const chipBarEl = document.getElementById('chip-bar');
    // Insert directly above the chip bar; fall back to the editor root when
    // the chip bar is absent (non-standard harness) so the banner still shows.
    if (chipBarEl?.parentElement) {
      chipBarEl.parentElement.insertBefore(banner, chipBarEl);
    } else {
      (document.getElementById('moss-editor-root') ?? document.body).prepend(banner);
    }
  }
  banner.textContent = `Invalid YAML frontmatter: ${error}`;
}

/**
 * Put the file tree back onto the document the editor is ACTUALLY showing.
 *
 * A row click paints its selection in the click's own frame; when the open then
 * FAILS the editor keeps the previous document, so without this the tree asserts
 * one file while another is mounted and the next keystroke edits a file the user
 * believes they left. Selection and editor content must never disagree.
 *
 * Identity is the registry's active EntryId (CLAUDE.md: compare files by
 * EntryId, never by path) — `highlight()` upserts that path back to the SAME
 * EntryId. Nothing mounted → nothing to assert, so the selection is cleared.
 */
function reassertOpenDocument(): void {
  if (!fileTree) return;
  const openPath = currentFilePath();
  if (openPath) fileTree.highlight(openPath);
  else fileTree.clearSelection();
}

/**
 * What a `loadFile()` call actually did — so a caller can tell "the editor is
 * now showing this file" from "it isn't" before doing follow-up work —
 * notably the preview navigation in the `editor:file-selected` handler: a file
 * that failed to open must not move the preview, or the preview shows one
 * document and the editor another with nothing saying which is real.
 *
 * Three states rather than a boolean, because the two non-`'opened'` cases are
 * different facts and a caller that conflates them will get one of them wrong:
 *
 *  - `'opened'`   — the document is mounted. Follow-up work is valid.
 *  - `'failed'`   — the read failed. THIS call still owns the UI: it has
 *                   already toasted and put the tree back on the document that
 *                   is really open (`reassertOpenDocument`).
 *  - `'superseded'` — a newer `loadFile()` started while this one was awaiting.
 *                   This call owns NOTHING; the newer one owns the editor, the
 *                   tree and the preview. It must not toast-recover, re-assert
 *                   selection, or navigate — every one of those would fight the
 *                   load that replaced it.
 *
 * Flattened to `false`, `'superseded'` reads as failure and invites exactly
 * that fight; flattened to `true` it would re-authorize the stale navigation
 * this type exists to prevent.
 */
type LoadOutcome = 'opened' | 'failed' | 'superseded';

/**
 * `opts.forceAnchor` forwards straight to `fileTree.highlight()` (see its
 * docstring): pass `{ forceAnchor: true }` from a caller reached via an
 * EXTERNAL navigation (preview-nav syncing back into the editor, or warm
 * reopen — both route through applyResolvedNode/loadContentPageDeduped) so
 * the tree breadcrumb re-anchors even while expanded. Leave it omitted for
 * in-tree triggers (the `editor:file-selected` row-click/wikilink-nav
 * listener) so the frozen-anchor design is unaffected.
 */
/** Identifies the in-flight loadFile() call. Read by the describe_source_role
 *  resume guard below and by every `'superseded'` return. */
const loadGeneration = createGenerationToken();

/**
 * Load `filePath`, then say so (`editor:file-ready`) on every exit unless a
 * newer load took over: the shell delivers a pending caret position on it.
 */
async function loadFile(
  filePath: string,
  preParsed?: ParsedFrontmatter,
  opts?: { forceAnchor?: boolean },
): Promise<LoadOutcome> {
  const thisLoad = loadGeneration.bump();
  try {
    return await loadFileBody(filePath, thisLoad, preParsed, opts);
  } finally {
    if (!loadGeneration.isStale(thisLoad)) {
      uiHost.bus.emit('editor:file-ready', { path: toRelative(filePath, projectRoot ?? '') });
    }
  }
}

async function loadFileBody(
  filePath: string,
  thisLoad: number,
  preParsed?: ParsedFrontmatter,
  opts?: { forceAnchor?: boolean },
): Promise<LoadOutcome> {
  // A document whose file left outside moss keeps its text when the author moves on.
  outsideFile.leaving();
  await flushPendingSave();
  if (loadGeneration.isStale(thisLoad)) return 'superseded';
  parkActiveDocument();
  otherWriters.leaving();

  // Forget what the heading knows — the new file's state hasn't resolved yet,
  // and any heading commit dispatched before applyHeadingState resolves must be
  // refused rather than routed against the previous file. Text, source and
  // "loaded" go together because they are one record; keeping them as separate
  // variables in lockstep was a paired invariant nothing enforced.
  headingController?.clear();

  // Normalize to the registry domain (project-root-relative) at the entry
  // point. Tauri wire events may carry absolute paths; all registry/event-bus
  // usages of `filePath` inside this function are relative; Tauri command
  // call sites wrap with `toAbsolute()`.
  filePath = toRelative(filePath, projectRoot ?? '');

  const category = detectFileCategory(filePath);

  // Non-text files → native viewer (image, video, audio, PDF, font, binary)
  if (category !== 'text') {
    // Synchronous from here to showFileViewer — no white-pane gap.
    teardownEmptyStateSurfaces();
    // Tear down editor state
    // Reset BEFORE the destroy sequence, never after. ChipBar.destroy closes any
    // open popover, and a close fires the history boundary — which after the
    // generation moved on would stamp the OUTGOING document's edit with the
    // INCOMING one's generation, forging exactly the stale entry the stamp
    // exists to make unforgeable. ChipBar.destroy() suppresses that emit via
    // its own `tearingDown`; this ordering is the other half of the same belt.
    resetDocumentHistory();
    if (cmEditor) { cmEditor.destroy(); cmEditor = null; }
    if (frontmatterStore) { frontmatterStore.destroy(); frontmatterStore = null; }
    if (chipBar) { chipBar.destroy(); chipBar = null; }
    destroyFileViewer();

    // Hide editor-only UI. Chip bar off (empties it too) via the single owner so
    // its border-bottom doesn't paint as a stray hairline over the viewer (#4).
    setChipBarVisible(false);

    // Render viewer. showFileViewer receives the absolute path for getFileInfo
    // (Rust fs::metadata boundary) and the projectRoot to compute the
    // root-relative form for the moss-source:// media URL.
    const cmContainer = document.getElementById('moss-editor-cm')!;
    cmContainer.innerHTML = '';
    showFileViewer(cmContainer, toAbsolute(filePath, projectRoot ?? ''), projectRoot ?? '', category);

    {
      const id = registry?.upsert(filePath, false) ?? null;
      registry?.setActive(id);
    }
    setSaveState('clean');
    documentBase = null;

    if (fileTree) fileTree.highlight(filePath, opts);
    // Non-text files never get an editor heading — pass empty frontmatter so
    // compute_heading_state returns visible=false (extension check fails).
    await applyHeadingState(filePath, {});
    // Create-session lifecycle: opening ANY file ends a session for another.
    createFlow?.onFileLoaded(filePath);
    return 'opened';
  }

  // Text files → existing CodeMirror flow
  destroyFileViewer();

  // Parse frontmatter and body — don't update currentFilePath until success
  // so we stay on the last file if this fails. Tauri command boundary:
  // registry stores relative, Rust opens via std::fs.
  //
  // `preParsed` is ONLY valid from the single init boot call site (the
  // editorBootstrap server-side parse of this exact source): at boot nothing
  // can be dirty, so the flushPendingSave() at the top of loadFile cannot
  // have rewritten the file after that parse. Any later call site would race
  // flushPendingSave — do not pass preParsed from anywhere else.
  let parsedData = preParsed;
  if (!parsedData) { let parseError: string | undefined;
    // A failed open must be VISIBLE, and must not leave the tree claiming a
    // file the editor never opened. runAction guarantees
    // the toast on both failure shapes; reassertOpenDocument re-syncs the tree.
    //
    // Deliberately guardless (no `trigger`, no `key`): opening a file
    // SUPERSEDES an in-flight open — clicking B while A loads must land on B —
    // where runAction's guard would refuse the second click. `loadGeneration`
    // is that supersede check and doubles as `stale()`: a superseded load still
    // toasts (the read did fail) but owns no UI, so it touches no selection.
    parsedData = await runAction({
      run: () => engine.parseFrontmatter(toAbsolute(filePath, projectRoot ?? '')),
      failMessage: (error) => { parseError = error; return t('editor.open_failed', { file: filePath.split('/').pop() || filePath, error }); },
    });
    if (!parsedData) {
      if (loadGeneration.isStale(thisLoad)) return 'superseded';
      // Stay on the current file; do not swap in a broken document — and put
      // the tree back onto whatever that current file is. The heading goes back
      // too: `clear()` above withdrew its answer in anticipation of a load that
      // has now not happened, and no `applyHeadingState` runs on this path, so
      // leaving it withdrawn makes the still-open document's title silently
      // uncommittable until some unrelated edit happens to re-resolve it.
      reassertOpenDocument();
      headingController?.restore();
      // A failure here leaves whatever surface was already painted untouched (see above), silent beyond a toast.
      void warn(`[editor] loadFile: parseFrontmatter failed for ${filePath}: ${parseError}`);
      return 'failed';
    }
  }

  // The file is real and its bytes are not here yet. Replace the
  // outgoing surface rather than staying on it: a pane left standing over a
  // failed open is read as a verdict about the file just asked for, which is
  // how "This page is generated by moss" ended up over a home note the user
  // could see in Finder. The outgoing document was saved and parked above;
  // applyEditorSurface clears the tree selection too, so the two halves cannot
  // disagree about what is open.
  if (parsedData.still_arriving) {
    if (loadGeneration.isStale(thisLoad)) return 'superseded';
    // Named, not merely announced: the shell's only other handle on "which
    // document" is the live preview URL, and after a file-tree click that names
    // a different page, and healing it would aim at the wrong file.
    // Absolute, to match the domain the shell pushes back in.
    applyEditorSurface({
      kind: 'pending',
      reason: 'download',
      sourcePath: toAbsolute(filePath, projectRoot ?? ''),
    });
    return 'failed';
  }

  const { frontmatter, body } = parsedData;
  const isMarkdown = isMarkdownFile(filePath);
  let slotRole: SlotRole | null = null;
  if (isMarkdown) {
    // Finish optional enrichment before switching document ownership. During
    // this await the outgoing editor, store and registry remain one document.
    try {
      const roleResult = await engine.describeSourceRole(toAbsolute(filePath, projectRoot ?? ''));
      if (loadGeneration.isStale(thisLoad)) return 'superseded';
      if (roleResult.status === 'ok' && roleResult.data.slot) slotRole = { slot: roleResult.data.slot, language: roleResult.data.language };
    } catch (e) {
      if (loadGeneration.isStale(thisLoad)) return 'superseded';
      console.warn('[editor] describeSourceRole failed; treating as a normal page:', e);
    }
  }

  // Content is in hand — but only this load's content matters. Without the
  // check a superseded load tears down the surface the WINNING load mounted
  // and, since teardown announces the pane settled, disarms an arrival retry
  // the winner had just armed: two rapid tree clicks, the slower one landing
  // last, leave a pending pane's shell no longer watching.
  if (loadGeneration.isStale(thisLoad)) return 'superseded';
  // The previous surface stays painted through the read above, and a failed
  // read returns without touching it.
  teardownEmptyStateSurfaces();

  const id = registry?.upsert(filePath, false) ?? null;
  registry?.setActive(id);

  // Clear the outgoing file's banner now; this file's renders after the CM mount,
  // beside its final anchor rather than inside a scroller about to be destroyed.
  renderFrontmatterErrorBanner(null);

  // Tear down the previous store, chip bar and history: flushPendingSave (top of
  // loadFile) already flushed the store's write, and ChipBar.destroy closes any open
  // popover, a close that must not record an entry under the incoming document.
  resetDocumentHistory();
  if (frontmatterStore) {
    frontmatterStore.destroy();
    frontmatterStore = null;
  }
  if (chipBar) chipBar.destroy();

  const initial = (frontmatter && typeof frontmatter === 'object' && !Array.isArray(frontmatter))
    ? frontmatter as Record<string, unknown>
    : {};

  // CodeMirror stores line breaks as LF, so a parked CRLF file is compared normalized.
  const parkedDoc = id ? parked.get(id) : undefined;
  const parkedText = parkedDoc ? (parkedDoc.cm as { doc: string }).doc : undefined;
  const resumed = parkedDoc && ((parkedDoc.unsaved && parkedDoc.base !== null) || (
    parkedText === body.replace(/\r\n?/g, '\n')
    && deepEqual(parkedDoc.fm, isMarkdown ? initial : {})))
    ? parkedDoc
    : undefined;
  const unsavedResume = resumed?.unsaved === true && resumed.base !== null;

  // Render form (markdown only — non-markdown files have no frontmatter)
  if (isMarkdown) {
    // The store holds no path: saveActiveDocument, the single writer, reads the live
    // path at write time, so a rename before the write never targets the old path.
    frontmatterStore = new FrontmatterStore({ initial: unsavedResume ? resumed.fm : initial, requestSave: saveActiveDocument });

    // Every frontmatter edit (title:, draft:, …) re-evaluates the heading element.
    frontmatterStore.subscribe((values) => {
      invalidatePublishPreflightPresentation();
      const p = currentFilePath();
      if (p) {
        void applyHeadingState(p, values);
      }
    });

    // Slots keep a store so hidden frontmatter survives saves; only their
    // chips are read-only.
    chipBar = new ChipBar(schema!, frontmatterStore, {
      getActivePath: () => activeDocumentPath() ?? '',
      getProjectRoot: () => projectRoot ?? undefined,
      afterVaultImport: () => afterVaultImport(fileTree),
      slotRole,
      // One entry per completed chip edit, interleaved with the body's own in
      // the order they happened — which is the whole point: ⌘Z after "typed a
      // sentence, then set a tag" undoes the tag first.
      onFieldEdit: (key, before, after) => {
        documentHistory.recordField(key, before, after);
        noteDocumentEdited();
      },
    });
    // Re-show the bar a non-markdown file may have hidden; showing never clears it,
    // so the render fills a shown-but-empty bar.
    setChipBarVisible(true);
    chipBar.render(document.getElementById('chip-bar')!);
    void refreshPageFacts();
    void new ChipDiagnostics((p) => engine.validateContent(p)).refresh(filePath, 'chip-bar');
  } else {
    chipBar = null;
    // No frontmatter chips: keep the bar hidden so its border doesn't paint.
    setChipBarVisible(false);
  }

  // Mount CM6.
  //
  // Order matters: the heading element has been re-parented INTO the
  // previous CM6 scroller (see scrollDOM.prepend below). We must rescue
  // it BEFORE EditorView.destroy() destroys that scroller, or the heading
  // goes with it. The rescue runs via the `onBeforeDestroy` callback,
  // installed when we construct the editor below.
  if (cmEditor) cmEditor.destroy();
  const cmContainer = document.getElementById('moss-editor-cm')!;
  cmContainer.innerHTML = '';
  cmEditor = new CmEditor({
    content: body,
    restore: resumed?.cm,
    container: cmContainer,
    // The create-flow notification is what ends an Untitled-first session on
    // the first body edit (the filename stops following the title).
    // An instrument's release is a whole gesture: write it now, not after the typing debounce.
    onChange: (gestureEnd) => { scheduleSave(); if (gestureEnd) void flushPendingSave(); createFlow?.onBodyEdited(); },
    // The combined history reconciles itself against CM6's own depth rather
    // than mirroring the text, so it needs every update, not only the ones that
    // change the document.
    // Linux/Windows have no Edit menu at all, so this keymap IS the keyboard
    // path there; on macOS the menu accelerator wins first and it never fires.
    onHistoryKey: (direction) => routeHistory(direction),
    // "Expand" reads the title facts from the store and writes what the
    // derived text supplied through the chip bar's own update path, so each
    // field is a history entry like any chip edit (expand-derived/extension.ts).
    getFrontmatter: () => frontmatterStore?.getValues() ?? {},
    applyFrontmatter: (patch) => {
      if (patch.title !== undefined) chipBar?.setFieldValue('title', patch.title);
      if (patch.description !== undefined) chipBar?.setFieldValue('description', patch.description);
      if (patch.date !== undefined) chipBar?.setFieldValue('date', patch.date);
      if (patch.inferred !== undefined) chipBar?.setFieldValue('inferred', patch.inferred ?? undefined);
    },
    onDerivedFacts: (facts) => chipBar?.setSuggestions(
      Object.fromEntries(Object.entries(facts).filter(([, v]) => !!v)) as Record<string, string>,
    ),
    // The second editor→preview sync trigger. See onEditorCaretMove.
    onCaretMove: () => onEditorCaretMove(),
    onHistoryUpdate: (update) => {
      documentHistory.observeCm6(update);
      if (update.docChanged) noteDocumentEdited();
      else docStateCluster.sync();
    },
    // Link validation + Cmd+Click nav now pull from the shared reference cache
    // (cm-reference-resolver, driven by editorResolveReferences). No separate
    // link-resolution RPC is wired from here.
    getFromFile: () => activeDocumentPath() ?? filePath,
    // The reference card's link/image providers: a page-link
    // card's backend call and an image card's Replace… picker both need the
    // project root, the same accessor shape the insert bar's media picker
    // already uses below.
    getProjectRoot: () => projectRoot,
    getCurrentArticle: () => ({
      title: headingController?.saved()?.text ?? '',
      path: activeDocumentPath() ?? filePath,
      locations: toNameList(frontmatterStore?.getValues().location),
      revealLocations: () => { chipBar?.revealField('location'); },
    }),
    // Bottom toolbar (insert + format). Picker-backed inserts import like an
    // OS drop, then flush so ONE watcher rebuild sees asset + embed together.
    insertBar: isMarkdown
      ? {
          pickMedia: (k) => pickMediaForBar(k, { currentFilePath, projectRoot: () => projectRoot }),
          afterMediaInsert: async () => { await flushPendingSave(); await afterVaultImport(fileTree); },
          // "Settings…" catalog row → relay to the shell, which owns the
          // SettingsModal (same cross-webview relay shape as reveal-field).
        }
      : undefined,
    // Wikilink / ![[asset]] / [[Page#Heading]] autocomplete (markdown only).
    // One Rust command computes all candidates (page/asset/heading); the
    // frontend computes only the cursor context and renders. `page` is the
    // heading target (project-relative .md) for [[Page#…/[[#…, else null for
    // page+asset completion.
    wikilinkComplete: isMarkdown ? {
      complete: async (prefix, fromFile, syntax, page, allowedKinds) => {
        // Heading target may arrive as a path-without-ext (page name) or a
        // relative .md path; the Rust command joins it onto the project root,
        // so re-add the .md extension when missing.
        const heading = page === null ? null : (page.endsWith('.md') ? page : `${page}.md`);
        const result = await engine.wikilinkCompletions(
          prefix,
          toAbsolute(fromFile, projectRoot ?? ''),
          syntax,
          heading,
          allowedKinds ?? null,
        );
        return result.status === 'ok' ? result.data : [];
      },
      // Project-root-relative path of the active file (or null), for [[#
      // same-page heading lookup.
      getCurrentPath: () => activeDocumentPath(),
    } : undefined,
    // "Browse files…" on the asset-completion tooltip. Same import
    // follow-through an OS drop gets: a copy that never reaches the tree is
    // invisible until some unrelated refresh.
    assetBrowse: isMarkdown ? {
      pagePath: () => activeDocumentPath(),
      projectRoot: () => projectRoot ?? null,
      afterImport: () => afterVaultImport(fileTree),
    } : undefined,
    // Screenshot paste: persist clipboard image bytes where the
    // `[editor].attachment_folder` setting says dropped images go (default:
    // next to the open file) and hand cm-paste the stored path, which it
    // writes as `![](path)` from the page.
    pasteImage: async (suggestedName, dataBase64) => {
      const path = activeDocumentPath();
      if (!path) return null;
      const dir = await attachmentDirFor(path);
      const rel = childPath(dir, suggestedName);
      const result = await engine.writeEditorFileBinary(rel, dataBase64);
      return result.status === 'ok' ? rel : null;
    },
    onBeforeDestroy: () => {
      // Both scroller residents go back to the root before the scroller dies
      // (the chip bar lives there too — design doc Amendment III).
      const root = document.getElementById('moss-editor-root');
      for (const id of ['editor-heading', 'chip-bar']) {
        const el = document.getElementById(id);
        if (el && root) root.appendChild(el);
      }
    },
    // Thread the coordinator so the reference resolver registers its
    // FileChanged/AssetReady handlers via the single shared coordinator rather
    // than self-subscribing to the cross-webview bus.
    fileEvents: fileEvents ?? undefined,
    // Internal tree→editor drop resolver: maps entry IDs
    // from the `application/x-moss-entries` drag payload to project-relative
    // paths. Directory entries end with `/` so the embed names the folder with its trailing slash.
    // Delegated to FileTree.resolveDropEntries which reads `EntryRecord.isDir`
    // from the registry — correct for both file and folder drag sources.
    resolveEntries: (ids: number[]) => fileTree?.resolveDropEntries(ids) ?? [],
    destinationFor,
    // ArrowUp on an empty cursor at line 1 moves focus UP into the title with
    // the caret at its end (Obsidian inline-title parity — the mirror of the
    // heading's ArrowDown-descend into `focusBody` above). Returns false when
    // the title is hidden (empty-title state) so CM6's default ArrowUp runs.
    onEscapeTop: () => headingController?.focusEnd() ?? false,
  });
  documentBase = unsavedResume ? resumed.base : { ...baseFrom(parsedData), frontmatter: isMarkdown ? initial : {} };
  if (resumed) documentHistory.restore(resumed.log);
  if (id) parked.delete(id);
  void renderPublishPreflightHints();
  void revealPendingPreflight();
  setSaveState(unsavedResume ? 'dirty' : 'clean');
  // The text came back on its old base. A save settles it through the one
  // owner: refused, it merges the disk or stays unsaved; accepted, it lands.
  if (unsavedResume) scheduleSave();

  // Observability (issue #5b): record that a real, editable CM6 document mounted
  // (vs. a folder/empty surface). Pairs with the editor:seal log so a stuck-seal
  // vs. wrong-surface vs. mount-failure can be told apart from moss.log alone.
  void logInfo(`[loadFile] mounted CM6 editor — isMarkdown=${isMarkdown} bodyLen=${body.length}`);

  // Re-parent the heading element INTO CM6's scroller so it scrolls with
  // the markdown body. CM6 only manages `.cm-content`'s children; the
  // scroller's child set is ours to control. We prepend the heading so it
  // appears above the markdown content inside the scroll surface.
  // (See editor.css comment on `.editor-heading` for the architecture.)
  const headingEl = document.getElementById('editor-heading');
  if (headingEl && cmEditor) {
    cmEditor.getView().scrollDOM.prepend(headingEl);
  }
  // The chip bar is document content (Amendment III): it joins the scroller
  // ABOVE the heading — source order, frontmatter before H1 — and scrolls
  // away with the text. Prepending after the heading puts it first. Because
  // it scrolls, a body-portaled chip popover would detach from its anchor;
  // the scroll closes it (ctx-menu convention).
  const chipBarEl = document.getElementById('chip-bar');
  if (chipBarEl && cmEditor && isMarkdown) {
    const view = cmEditor.getView();
    view.scrollDOM.prepend(chipBarEl);
    view.scrollDOM.addEventListener('scroll', () => chipBar?.closeAnyOverlay(), { passive: true });
  }
  // BUG 3: a malformed `---...---` block parses to ZERO fields, so the chip
  // form renders blank with no hint the block is broken. Surface the YAML
  // error as a banner above the chips (the raw block stays in CM6 `body` for
  // the author to repair). Rendered HERE, after the chip bar has joined the
  // new scroller, so the banner anchors beside it and scrolls with it; it is
  // deliberately NOT rescued by onBeforeDestroy — it dies with the scroller
  // and every load re-derives it from the file's own parse.
  if (isMarkdown) renderFrontmatterErrorBanner(parsedData.frontmatter_error ?? null);

  // Test-harness hook — exposes the live CM6 EditorView and a `setContent`
  // that loads a markdown doc through the FULL markdown extension stack
  // (the real `CmEditor` built above, so the shortcode grammar + block
  // extension + live-preview are all wired). Used by the Playwright glitch
  // regression test for shortcode rendering to drive real
  // edits and assert shortcode blocks never flicker. Gated to the test
  // harness so it never ships in production builds.
  if (cmEditor && import.meta.env.DEV) {
    const view = cmEditor.getView();
    (window as unknown as {
      __harness?: { view: typeof view; setContent: (t: string) => void };
    }).__harness = {
      view,
      // Raw whole-doc replace. Deliberately skips the `externalUpdate`
      // annotation the save pipeline requires for external replacement — this
      // is test-only fixture setup, not a real document load, so it must not be
      // copied as a pattern for production document replacement.
      setContent: (t: string) =>
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: t } }),
    };
  }

  // Attach scroll listener for scroll sync (must be after CM6 mount)
  attachEditorScrollListener();

  // Highlight in file tree and update breadcrumb
  if (fileTree) {
    fileTree.highlight(filePath, opts);
  }

  // Apply heading state — fetches the canonical {visible, text} from Rust
  // (`compute_heading_state`), shared source of truth with the build pipeline.
  const fm = (frontmatter && typeof frontmatter === 'object' && !Array.isArray(frontmatter))
    ? frontmatter as Record<string, unknown>
    : {};

  await applyHeadingState(filePath, unsavedResume ? resumed.fm : fm);
  // The LAST await before 'opened' needs the same guard as every earlier one —
  // a caller (editor:file-selected's own previewFollower.navigate) gates on
  // this outcome and would drag the preview back to a superseded file.
  if (loadGeneration.isStale(thisLoad)) return 'superseded';
  // Create-session lifecycle (create-flow.ts): a load of the just-created
  // untitled file activates its session (status line + heading placeholder);
  // a load of anything else ends it. Runs on EVERY loadFile path — tree
  // click, preview-nav, warm reopen — so a session can never outlive its file.
  createFlow?.onFileLoaded(filePath);
  return 'opened';
}

// ── Auto-Save ──────────────────────────────────────────────────────────

/**
 * Auto-save debounce (ms). Lowered from 2000 (the old Obsidian-match) to make
 * the live preview feel responsive: the user pauses, the file is written, the
 * watcher rebuilds, the morph applies. moss is a preview tool, so fast feedback
 * beats matching a pure editor's cadence; 800ms still coalesces a typing burst
 * into one write (the file watcher + rebuild lock coalesce further downstream).
 */
const AUTOSAVE_DEBOUNCE_MS = 800;

const externalReloadGeneration = createGenerationToken();

function scheduleSave(): void {
  if (saveTimeout) clearTimeout(saveTimeout);
  invalidatePublishPreflightPresentation();
  setSaveState('dirty');

  saveTimeout = setTimeout(() => {
    // Route through the single writer — it reads the live path plus
    // body+frontmatter fresh at write time.
    void saveActiveDocument();
  }, AUTOSAVE_DEBOUNCE_MS);
}

/** One-way, truncated hash of a path for logging — the path itself never reaches the log. */
async function hashPathForLog(path: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(path));
  return Array.from(new Uint8Array(digest).slice(0, 4))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

// The ONE place that writes a document to disk. Path + frontmatter + body are
// all read fresh per iteration, so body and frontmatter can never target two
// paths, and a change that arrives mid-write is re-saved (trailing coalesce).
//
// `reason` names which trigger STARTED this write cycle — best-effort, not
// exact: a write that arrives while one is already in-flight only sets
// `saveQueued` and rides the ORIGINAL caller's reason (the coalesce this
// function's own doc describes), so a forced flush that lands mid-autosave
// is still logged as `autosave`.
async function saveActiveDocument(reason: 'autosave' | 'forced' = 'autosave'): Promise<void> {
  // While a rename, move or undo commit holds the editor read-only, the
  // commit's own explicit flush (reason: 'forced', called once up front) is
  // the only write this function permits. An autosave-reason call arriving
  // during or after the commit — from a debounce that was already ticking
  // down before the lock engaged, or one that otherwise slips through — is
  // refused rather than allowed to write pre-commit content over the
  // commit's own rewrite of that same file. The lock therefore guards
  // autosave calls as well as direct input events.
  if (editorCommitLocked && reason !== 'forced') return;
  if (saveInFlight) { saveQueued = true; return saveInFlight; }
  // Defensive: clear the prior result so a run that short-circuits before any
  // write (no path / no editor) leaves lastWriteOk null — flushPendingSave then
  // reports `null ?? true` (no-op success), never a stale prior false.
  lastWriteOk = null;
  saveInFlight = (async () => {
    let conflicts = 0;
    do {
      saveQueued = false;
      const path = activeDocumentPath();
      const editor = cmEditor;
      const base = documentBase;
      if (!path || !editor || !base) break;
      // The file left this path; nothing is written until the file event says
      // where it went, and a caller flushing before it moves on must know.
      if (outsideFile.isGoneAt(path)) { lastWriteOk = false; break; }
      const frontmatter = frontmatterStore ? frontmatterStore.getValues() : {};
      const body = editor.getContent();
      const bodyBytes = new TextEncoder().encode(body).length;
      void hashPathForLog(path).then((pathHash) => {
        void logInfo(`[editor] save path_hash=${pathHash} bytes=${bodyBytes} autosave=${reason === 'autosave'}`);
      });
      setSaveState('saving');
      try {
        const result = await engine.saveEditorContent(
          toAbsolute(path, projectRoot ?? ''),
          frontmatter as unknown as JsonValue,
          body,
          base.hash,
          base.keep === true,
        );
        if (result.status === 'ok' && result.data?.status === 'gone') {
          // Nothing was written: the file left this path a moment ago. Keep the
          // text unsaved and wait for the file event.
          outsideFile.noteGone(path);
          setSaveState('dirty');
          lastWriteOk = false;
          break;
        }
        if (result.status === 'ok' && result.data.status === 'conflict') {
          // Nothing was written: another writer changed the file. Fold its text
          // into the buffer and save again on its base. A bounded retry, since
          // a writer that keeps rewriting the file must not spin this loop.
          const outcome = cmEditor === editor ? reconcileWithDisk(result.data.disk) : 'detached';
          if (outcome !== 'detached' && ++conflicts <= MAX_CONFLICT_RETRIES) { saveQueued = true; continue; }
          setSaveState('dirty');
          lastWriteOk = false;
          // A writer rewriting the file faster than the retries: try again after the debounce.
          if (outcome !== 'detached') scheduleSave();
          break;
        }
        if (result.status === 'ok') {
          if (result.data.status === 'not-kept') {
            // The other writer's text could not be kept, so nothing was written.
            if (cmEditor === editor) otherWriters.noteNotKept(path);
            setSaveState('dirty');
            lastWriteOk = false;
            break;
          }
          if (result.data.status === 'saved' && cmEditor === editor) {
            documentBase = { hash: result.data.hash, body, frontmatter };
            otherWriters.withdrawNotKept();
            if (result.data.displaced) otherWriters.announceKept(path);
          }
          setSaveState('clean');
          lastWriteOk = true;
        } else {
          setSaveState('error');
          lastWriteOk = false;
          console.error('Save failed:', result.error);
        }
      } catch (err) {
        setSaveState('error');
        lastWriteOk = false;
        console.error('Save threw:', err);
      }
    } while (saveQueued);
  })();
  try { await saveInFlight; } finally { saveInFlight = null; }
}

function setSaveState(state: SaveState): void {
  // Silent-save: an internal flag that gates flushPendingSave and renders nothing. Failure surfaces
  // flag that gates flushPendingSave and renders nothing. Failure surfaces
  // through the Rust save PanelTask's inline badge.
  saveState = state;
}

const MAX_CONFLICT_RETRIES = 3;

/**
 * Bring a disk text the buffer has not seen into the open document. The one
 * owner of that decision, for the watcher's reload and for a save that came
 * back `conflict`. Synchronous, so the buffer it merges is the buffer it
 * changes: a keystroke cannot land between the two.
 *
 * `unchanged`: the disk is the base. `applied`: the disk's edits that touch
 * none of the person's are merged into the buffer and the disk is the new
 * base; where they overlap, the person's text stays and the base is marked to
 * keep the disk's bytes when the next save writes over them. `detached`: no
 * document to merge into.
 */
function reconcileWithDisk(disk: ParsedFrontmatter): 'unchanged' | 'applied' | 'detached' {
  const base = documentBase;
  const editor = cmEditor;
  const store = frontmatterStore;
  const path = currentFilePath();
  if (!base || !editor || !path) return 'detached';
  if (disk.hash === base.hash) return 'unchanged';
  const next = baseFrom(disk);
  if (!store) next.frontmatter = {};
  const buffer = { body: editor.getContent(), frontmatter: store?.getValues() ?? {} };
  const merge = mergeExternal(base, next, buffer);
  // The build's evidence names spans in the bytes it read; mapped through this
  // change it would outlive its revision. Clear it, then let the revision check
  // decide whether the last build saw these bytes.
  replacePublishPreflightHints([]);
  editor.applyExternal(merge.changes);
  store?.onFileChanged(merge.frontmatter);
  // Lines set aside earlier may still be in these bytes.
  if (base.keep) next.keep = true;
  if (!isClean(merge)) {
    void warn(`[editor] the open file changed outside moss on lines with unsaved edits (${merge.overlaps.length} overlapping hunks, keys: ${merge.keys.join(', ') || 'none'}); the person's text wins and the other is kept on save`);
    next.keep = true;
    otherWriters.remember({ body: editor.getContent(), frontmatter: merge.frontmatter }, outsideWins(base, next, buffer));
  }
  documentBase = next;
  // Unsaved edits now sit on the new base; make sure a save carries them there.
  const unsaved = buffer.body !== base.body || !deepEqual(buffer.frontmatter, base.frontmatter);
  if (unsaved && !saveTimeout && !saveInFlight && !store?.hasPendingWrite()) scheduleSave();
  void renderPublishPreflightHints();
  void applyHeadingState(path, merge.frontmatter);
  return 'applied';
}

/** The mounted document owns every external read: the watcher and the post-rename refresh. */
async function reloadExternalDocument(): Promise<void> {
  const path = currentFilePath();
  const editor = cmEditor;
  const store = frontmatterStore;
  if (!path || !editor) return;
  const documentGeneration = docGeneration.current();
  const request = externalReloadGeneration.bump();
  const current = () => !docGeneration.isStale(documentGeneration)
    && !externalReloadGeneration.isStale(request)
    && currentFilePath() === path && cmEditor === editor && frontmatterStore === store;
  // A save in flight moves the base when it lands; read after it, so these
  // bytes are never older than the base they are compared with.
  if (saveInFlight) await saveInFlight;
  if (!current()) return;
  const base = documentBase;
  const parsed = await engine.parseFrontmatter(toAbsolute(path, projectRoot ?? ''));
  if (!current()) return;
  if (parsed.status !== 'ok') { console.warn('External reload failed:', parsed.error); return; }
  // An evicted file's bytes are not here: an empty text is not what is on disk.
  if (parsed.data.still_arriving) return;
  if (documentBase !== base || saveInFlight) { await reloadExternalDocument(); return; }
  reconcileWithDisk(parsed.data);
}

/**
 * Freeze (or unfreeze) the open CM6 buffer for the duration of a rename,
 * move or undo commit. Engaged BEFORE the file tree's own
 * `flushPendingSave()` call so read-only refuses typing for the whole commit,
 * including while that flush's own IPC is in flight, so a keystroke can
 * never arm an autosave debounce that outlives the commit and overwrites
 * its rewrite of that same file. `saveActiveDocument`'s own guard on
 * `editorCommitLocked` is the actual belt-and-suspenders enforcement — this
 * flag is what it reads. Reuses the exact Compartment reconfigure the
 * publish seal already drives (`editor:seal`'s `cmEditor.setReadOnly`) —
 * same mechanism, a different trigger.
 */
function setEditorCommitLock(locked: boolean): void {
  // The freeze makes the body non-editable, and a browser then moves focus to
  // <body>. Focus held when the freeze starts, or asked for during it, is
  // given back when it ends, unless something else took focus meanwhile.
  if (locked) bodyFocusOwed ||= cmEditor?.getView()?.hasFocus ?? false;
  editorCommitLocked = locked;
  cmEditor?.setReadOnly(locked, "commit");
  if (!locked && bodyFocusOwed) {
    bodyFocusOwed = false;
    const active = document.activeElement;
    if (active === null || active === document.body) cmEditor?.getView()?.focus();
  }
}

// ── Scroll Sync ─────────────────────────────────────────────────────────
// Scroll sync is always active — there is no toggle.

/**
 * Which pane sets the position both panes show. The preview leads by default,
 * and the editor follows it. The editor takes the lead only when the user works
 * in it — a wheel or trackpad scroll, a pointer press (a scrollbar drag is
 * one), or a key — and hands it back the moment anything arrives from the
 * preview, the editor is opened on a document, or its window loses focus.
 * Following means going where the preview is, not only moving when it moves:
 * see `followPreviewPosition`.
 *
 * Only a leading editor tells the preview where to go. CM6 fires `scroll` for
 * layout as readily as for the user, so a scroll event on its own says nothing
 * about who moved: reopening the editor on the file it last showed grows the
 * panel from collapsed, the reflow moves `scrollTop`, and the editor used to
 * push the line it had been left on into a preview the user was reading
 * somewhere else. The 400ms lock this replaces could not tell those apart
 * either — it only hid the echo of a scroll the editor had just been told to
 * make.
 */
let editorLeads = false;

let scrollSyncTimer: ReturnType<typeof setTimeout> | null = null;
let caretSyncTimer: ReturnType<typeof setTimeout> | null = null;
/** The last line the caret sync sent, so a caret walking within one line does
 *  not re-send. Reset per file by `attachEditorScrollListener`. */
let lastCaretSyncLine = 0;

/** The preview leads again: drop the editor's lead and any sync it had queued. */
function followPreview(): void {
  editorLeads = false;
  if (scrollSyncTimer) clearTimeout(scrollSyncTimer);
  if (caretSyncTimer) clearTimeout(caretSyncTimer);
}

/** The user is working in the editor, so its position is the one to share. */
function leadFromEditor(): void {
  editorLeads = true;
}

let followPositionTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Ask the shell where the preview is, and go there — for an editor that has
 * just opened, loaded a document, or been resized while following. The
 * preview reports its position only when it scrolls, so an editor opened over
 * a page the user had scrolled and left would otherwise sit wherever it was.
 *
 * Debounced: a warm open lands while the panel is still collapsed, and its grow
 * animation resizes the editor on every frame. Only the settled layout is worth
 * aligning to; a line placed at the centre of a sliver-wide editor is nowhere
 * once the text re-wraps. The answer arrives as an ordinary preview scroll
 * (`onPreviewScroll`), and the shell drops the question when the preview is on
 * a different page than this document.
 */
function followPreviewPosition(): void {
  if (followPositionTimer) clearTimeout(followPositionTimer);
  followPositionTimer = setTimeout(() => {
    followPositionTimer = null;
    const path = currentFilePath();
    if (editorLeads || !cmEditor || !path) return;
    uiHost.bus.emit('scroll-sync:request-preview-position', { sourceFile: toAbsolute(path, projectRoot ?? '') });
  }, 150);
}

/** Threshold for "editor at top." Mirror of TOP_THRESHOLD in
 *  crates/moss-build/src/js-src/bridge/iframe-bridge.ts; keep in lockstep unless an
 *  intentional divergence is documented. */
const TOP_THRESHOLD = 4;

/** Threshold (px) for "editor at bottom" — small slack so a scroller that
 *  stops a sub-pixel short of its max still counts as bottom. */
const BOTTOM_THRESHOLD = 4;

/**
 * Handle CM6 scroll: send the top visible line number so the preview can
 * scroll to the nearest element with a matching `data-source-line` attribute.
 * Falls back to heading-anchor sync for content without line annotations.
 */
function onEditorScroll(): void {
  if (!editorLeads || !cmEditor) return;

  if (scrollSyncTimer) clearTimeout(scrollSyncTimer);
  scrollSyncTimer = setTimeout(() => {
    if (!cmEditor) return;
    const view = cmEditor.getView();
    // The "focus line" is the source line at the editor's viewport center (see
    // getFocusLine / FOCUS_FRACTION). The preview aligns the same line at its
    // own center, so the line you're reading stays synced across the panes.
    const topLine = getFocusLine(view);

    // atTop uses the offset-adjusted threshold to mirror getFocusLine's
    // coordinate-system compensation. Without the offset, "editor at top"
    // would be strictly narrower than "editor reports line 1," and after the
    // backstop removal the preview would scrollIntoView the H1 (hiding
    // chrome) whenever the user scrolled past the heading but stayed in
    // line-1 range.
    const offset = computeContentOffset(view);
    const atTop = view.scrollDOM.scrollTop < offset + TOP_THRESHOLD;

    // Symmetric to atTop: the editor's scroller is at (or within a hair of)
    // its maximum. The top-visible line is then some line BEFORE the last, so
    // a line-based sync parks that line's element at block:start and leaves the
    // preview short of its own bottom (the reported "preview stops mid-way"
    // bug). The preview short-circuits to its max scroll on this flag, exactly
    // as atTop short-circuits to 0. atTop is checked first on the bridge, so a
    // doc too short to scroll (both true) resolves to top, which is correct.
    const sd = view.scrollDOM;
    const atBottom = sd.scrollTop + sd.clientHeight >= sd.scrollHeight - BOTTOM_THRESHOLD;

    // Primary: line-based sync (requires data-source-line in built HTML)
    uiHost.bus.emit('scroll-sync:editor-to-preview', {
      type: 'source-line',
      line: topLine,
      // Fallback: include anchor ID for previews without data-source-line
      anchorId: (() => {
        const doc = view.state.doc.toString();
        const heading = findNearestHeading(doc, topLine);
        return heading ? headingToAnchor(heading.text) : null;
      })(),
      // Identity: stamp with the currently-loaded file so the parent-side
      // handler can drop messages that don't match the preview's active page.
      // active.sourceFile is populated from `resolve_page_source` (Rust),
      // which returns an absolute path — stamp absolute here so both sides
      // compare in the same domain.
      sourceFile: currentFilePath() ? toAbsolute(currentFilePath()!, projectRoot ?? '') : null,
      // Named signal: editor's scroller is in the chrome-visible region
      // (heading-visible + line-1 range). The preview short-circuits to
      // scrollTo({top:0}) on this flag regardless of the line value.
      atTop,
      // Named signal: editor's scroller is at its bottom. The preview
      // short-circuits to its max scroll (scrollHeight) regardless of line.
      atBottom,
    });
  }, 100);
}

/**
 * The caret moved (or an edit moved it): tell the preview to show that line,
 * BUT ONLY IF IT IS NOT ALREADY SHOWING IT.
 *
 * That last clause is the whole difference from the scroll trigger, and it is
 * the reason this can exist at all. The caret moves on every keystroke; a sync
 * that centred the line each time would make the preview twitch continuously
 * while you typed, which is worse than a preview that does not follow you. So
 * the request is marked `onlyIfOffscreen`, and the bridge — the only party that
 * knows what is actually visible over there — drops it when the target is
 * already comfortably in view. Typing on a line you can see moves nothing;
 * clicking a media token forty lines away brings it in.
 *
 * Debounced at 120ms and skipped when the line has not changed, so holding a
 * key down or arrowing through a paragraph costs one message, not fifty.
 */
function onEditorCaretMove(): void {
  if (!editorLeads || !cmEditor) return;
  if (caretSyncTimer) clearTimeout(caretSyncTimer);
  caretSyncTimer = setTimeout(() => {
    if (!cmEditor) return;
    const view = cmEditor.getView();
    const line = view.state.doc.lineAt(view.state.selection.main.head).number;
    if (line === lastCaretSyncLine) return;
    lastCaretSyncLine = line;
    uiHost.bus.emit('scroll-sync:editor-to-preview', {
      type: 'source-line',
      line,
      anchorId: (() => {
        const heading = findNearestHeading(view.state.doc.toString(), line);
        return heading ? headingToAnchor(heading.text) : null;
      })(),
      sourceFile: currentFilePath() ? toAbsolute(currentFilePath()!, projectRoot ?? '') : null,
      // Never an edge short-circuit: those mean "the SCROLLER is at its end",
      // which says nothing about where the caret is.
      atTop: false,
      atBottom: false,
      onlyIfOffscreen: true,
    });
  }, 120);
}

/**
 * The preview scrolled: move the editor to the same place, and let the preview
 * keep the lead so the scroll this causes is not sent back.
 *
 * `atTop` is resolved first, symmetric with the bridge, so a page too short to
 * scroll — both flags true — parks the editor at its top, where the file-title
 * heading stays visible; aligning line 1 would scroll past it. `atBottom` parks
 * the editor at its end, because the preview's last screenful is often
 * unannotated (a tall footer, an embed) and a line would leave it short.
 */
function onPreviewScroll({ line, atTop, atBottom, answering }: { line?: number; atTop?: boolean; atBottom?: boolean; answering?: string }): void {
  if (!cmEditor) return;
  // A reply to followPreviewPosition's question rather than a scroll the
  // preview made. Nothing moved over there, so it loses to anything that
  // happened here while it was in flight: the user starting to work in the
  // editor, or the editor moving on to another document.
  const path = currentFilePath();
  if (answering !== undefined && (editorLeads || !path || answering !== toAbsolute(path, projectRoot ?? ''))) return;
  followPreview();
  const view = cmEditor.getView();
  if (atTop) scrollEditorToTop(view);
  else if (atBottom) scrollEditorToBottom(view);
  else if (line != null) scrollEditorToLine(view, line);
}

/**
 * Attach the SCROLL half of editor→preview sync. Must be called after each
 * loadFile since CM6 is destroyed/recreated. The caret half is an editor
 * option (`onCaretMove`), registered where CM6's extensions are composed.
 *
 * TWO triggers, because they answer different questions:
 *  - scrolling asks "what am I READING" → the viewport centre, tracked
 *    continuously, and the preview centres it to match;
 *  - the caret asks "what am I WORKING ON" → the caret's line, sent only when
 *    it lands somewhere the preview is not already showing.
 *
 * Before this, scrolling was the only trigger, so the preview followed you
 * only when you moved the scrollbar. Clicking something, jumping with a
 * keystroke, or typing on a line already on screen moved nothing — the preview
 * would sit on a different part of the page than the one being edited until a
 * scroll happened to shake it loose.
 *
 * Both triggers fire only while the editor leads (see `editorLeads`), so the
 * user's own input in the editor's pane is what hands it the lead: the pane,
 * not just this scroller, because the insert bar sits beside the scroller in
 * it. (A file drop never reaches the DOM — Tauri takes it — so the native drop
 * handler takes the lead itself.) A freshly loaded document starts following
 * the preview.
 */
function attachEditorScrollListener(): void {
  if (!cmEditor) return;
  const view = cmEditor.getView();
  followPreview();
  lastCaretSyncLine = 0;
  // The pane outlives every document; re-adding the same listener is a no-op.
  const pane = document.getElementById('moss-editor-cm') ?? view.scrollDOM;
  for (const type of ['wheel', 'pointerdown', 'keydown']) {
    pane.addEventListener(type, leadFromEditor, { capture: true, passive: true });
  }
  view.scrollDOM.addEventListener('scroll', onEditorScroll, { passive: true });
  followPreviewPosition();
}

// ── External Drag-and-Drop Helpers ─────────────────────────────────────

/** Ensure the hint pill is present in document.body, positioned over cmEl.
 * Uses position:fixed so the pill is visible regardless of cmEl's scroll
 * position (cmEl has overflow-y:auto — an absolutely-positioned child would
 * be clipped to the content bottom on long documents, the same trap the slash
 * menu avoids by appending to document.body; see cm-slash-menu.ts:147). */
function ensureDropHint(cmEl: HTMLElement): void {
  if (document.querySelector('.moss-drop-hint')) return; // already present
  const pill = document.createElement('div');
  pill.className = 'moss-drop-hint';
  pill.textContent = t('dnd.drop_to_embed');
  // Position pill at the bottom-centre of the editor using fixed coords derived
  // from the editor's viewport rect (mirrors the slash-menu positioning pattern).
  const rect = cmEl.getBoundingClientRect();
  pill.style.left = `${rect.left + rect.width / 2}px`;
  pill.style.bottom = `${window.innerHeight - rect.bottom + 16}px`;
  document.body.appendChild(pill);
}

/** Remove the hint pill from document.body if present. */
function removeDropHint(): void {
  document.querySelector('.moss-drop-hint')?.remove();
}

export function showDropZone(show: boolean): void { // exported for testing
  const cmEl = document.getElementById('moss-editor-cm');
  if (show) {
    cmEl?.classList.add('moss-drop-active');
  } else {
    cmEl?.classList.remove('moss-drop-active');
    removeDropHint(); // remove hint on drop/leave
  }
}

export function routeDropIndicator(logicalY: number, logicalX?: number): void { // exported for testing
  const cmRect = document.getElementById('moss-editor-cm')?.getBoundingClientRect();
  const cmEl = document.getElementById('moss-editor-cm');
  // X-bounds check: if logicalX is provided, also confirm cursor is within the editor's
  // horizontal extent so a drag over the file-tree at the editor's Y-range routes to
  // the tree, not the editor. When logicalX is omitted (legacy callers) only Y is checked.
  const isOverEditor = cmRect &&
    logicalY >= cmRect.top && logicalY <= cmRect.bottom &&
    (logicalX === undefined || (logicalX >= cmRect.left && logicalX <= cmRect.right));

  // Only the CM editor zone indicator is managed here.
  if (isOverEditor) {
    cmEl?.classList.add('moss-drop-active');
    if (cmEl) ensureDropHint(cmEl); // show pill when over editor
  } else {
    cmEl?.classList.remove('moss-drop-active');
    removeDropHint(); // hide pill when routing to nav/tree
  }
}

export async function handleEditorFileDrop(paths: string[], point?: { x: number; y: number }): Promise<void> { // exported for testing
  const path = currentFilePath();
  // Only a host with files on disk delivers an OS drop, and copies it in.
  const copyIn = uiHost.copyIn;
  if (!cmEditor || !path || !copyIn) return;

  // Classify each path as directory, embeddable file, or import-only file.
  // A folder is embedded as ![[name/]] after it is copied in.
  const isDirectory = uiHost.isDirectory ?? (async () => false);
  const statResults = await Promise.all(paths.map((p) => isDirectory(p).catch(() => false)));

  const dirPaths = paths.filter((_, i) => statResults[i]);
  const filePaths = paths.filter((_, i) => !statResults[i]);
  const embeddable = filePaths.filter(acceptOnDrop);
  const importOnly = filePaths.filter(p => !acceptOnDrop(p));

  const currentFileDir = path.includes('/')
    ? path.substring(0, path.lastIndexOf('/'))
    : ''; // file at project root → dir is the root itself
  const targetAbs = toAbsolute(currentFileDir, projectRoot ?? '');

  // Every branch below that lands a copy in the vault must be followed by a
  // file-tree refresh — the same afterVaultImport follow-through the tree-drop
  // path (os-drag-in.ts handleOsDrop) runs. Tracked with a flag + finally so
  // the early returns (embeddable-copy failure after a successful import-only
  // copy; non-embeddable-only drops that skip the embed entirely) can't skip
  // it — that skip was exactly the "editor drop never shows up in the tree"
  // bug this structure fixes.
  let copiedSomething = false;
  try {
    // Import-only file types still land co-located (no silent swallow), then toast.
    if (importOnly.length > 0) {
      const r = await copyIn(importOnly, targetAbs);
      if (r.status === 'ok') {
        reportImportSkips(r.data.skipped);
        copiedSomething = r.data.copied.length > 0;
        // Count what LANDED, not what was asked for — "Imported 3 files" over a
        // copy that landed one is the same lie in a friendlier voice.
        if (r.data.copied.length > 0) {
          const what = r.data.copied.length === 1
            ? t('dnd.imported_not_embeddable_one')
            : t('dnd.imported_not_embeddable_many', { n: r.data.copied.length });
          showToast({ what, severity: 'ShippedDegraded' });
        }
      } else {
        console.error('copy failed for import-only files', r.error);
        showToast({ what: t('dnd.import_files_failed'), severity: 'Blocking' });
      }
    }

    // Collect project-relative paths of what landed: embeddable files +
    // folders, identified by their trailing slash.
    const allEmbeddable: string[] = [];

    if (embeddable.length > 0) {
      // The one import policy (importIntoVault): images where the "Asset
      // path" setting says, everything else next to the page. It reports
      // failures and skips itself; the embed is written from the page to
      // wherever each file landed.
      const landed = await importIntoVault(embeddable, path, projectRoot ?? '');
      if (!landed) return;
      copiedSomething = copiedSomething || landed.length > 0;
      for (const f of landed) allEmbeddable.push(childPath(f.dir, f.finalName));
    }

    if (dirPaths.length > 0) {
      // Recursively copy dropped folders. copyFilesToProject
      // handles directories via copy_dir_recursive. After copying, embed as
      // ![](name/) (trailing slash marks a folder transclusion).
      const result = await copyIn(dirPaths, targetAbs);
      if (result.status !== 'ok') {
        console.error('Failed to copy dropped folders:', result.error);
        showToast({ what: t('dnd.import_folders_failed'), severity: 'Blocking' });
        // Do NOT return here — embeddable files already collected into allEmbeddable
        // must still be inserted (they are on disk). Only the failed folders are skipped.
      } else {
        reportImportSkips(result.data.skipped);
        copiedSomething = copiedSomething || result.data.copied.length > 0;
        for (const f of result.data.copied) allEmbeddable.push(`${childPath(currentFileDir, f.final_name)}/`);
      }
    }

    if (allEmbeddable.length === 0) return;

    // Insert the embeds at the DROP POINT when coordinates are available
    // (posAtCoords), else use document end as the deterministic fallback.
    // DELIBERATE change from the previous handleImageDrop (which fell back to
    // selection.main.head): a drop with no resolvable position lands at document END.
    // Block-snap the insertion position so the embed always lands on its own line
    // between paragraphs rather than mid-word (matches the indicator position).
    const view = cmEditor.getView();
    const rawAt = (point ? view.posAtCoords(point) : null) ?? null;
    const at = rawAt != null
      ? snapToBlockBoundary(view, rawAt, point?.y)
      : view.state.doc.length;
    const md = (await Promise.all(allEmbeddable.map(target => embedFor(target, destinationFor)))).join('\n');
    view.dispatch({ changes: { from: at, insert: md + '\n' }, userEvent: 'input.drop' });

    // Persist the embed IMMEDIATELY rather than waiting out the 800ms autosave
    // debounce. The just-copied asset already triggered a watcher rebuild; without
    // this flush that rebuild sees the markdown WITHOUT the embed (the image is
    // unreferenced → nothing to encode → a wasted `changed=0 → SUPPRESS` pass),
    // and the real encode only fires ~800ms later on a SECOND rebuild. The two
    // rapid rebuilds are exactly what races the staged `.webp` into the orphaned
    // state the asset-pipeline self-heal now recovers from — flushing now
    // collapses them so a single rebuild sees both the new asset AND the embed.
    //
    // The dispatch above already ran the CM6 updateListener synchronously, so
    // saveState is 'dirty' here; flushPendingSave cancels the pending debounce and
    // routes one write through the single writer. This is an ordinary user edit,
    // so normal save semantics (undo history intact, NOT an externalUpdate) apply.
    await flushPendingSave();
  } finally {
    // Shared post-import follow-through (see afterVaultImport in os-drag-in.ts):
    // the copied files must appear in the file tree immediately.
    if (copiedSomething) await afterVaultImport(fileTree);
  }
}

// ── Boot ───────────────────────────────────────────────────────────────

// Skeleton failsafe: by init resolution every boot path has either mounted
// content (the skeleton is already structurally gone) or terminally mounted
// nothing (no project + no source; bootstrap error; initial parse failure) —
// an eternal shimmer would be dishonest, so remove it and show the plain
// background. On the no-project path this is the SOLE nav-skeleton remover.
/** The editor's boot, settled once it has mounted something or nothing. */
export const editorBoot = init()
  .catch((e) => console.error('[editor] init failed:', e))
  .finally(dismissEditorSkeleton);
