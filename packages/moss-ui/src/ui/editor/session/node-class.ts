// packages/moss-ui/src/ui/editor/session/node-class.ts
//
// Pure node→surface decision for the editor. Every trigger (boot, file-tree,
// preview-nav) routes a ResolvedNode through decideEditorSurface; a single
// applyEditorSurface in editor-main.ts mounts the chosen surface.

import { isRootUrlPath } from "../../utils/url-path";

/**
 * Which generated page a takeover replaces. Mirrors `TakeoverKind` in
 * crates/moss-build/src/editor/resolve/takeover.rs (kebab-case on the wire)
 * as a list, which the generated type cannot give at runtime. node-class.test.ts
 * fails to typecheck the moment this list and the generated type drift.
 */
export const TAKEOVER_KINDS = ['root-home', 'folder-home', 'term-page', 'lang-home'] as const;
export type TakeoverKind = (typeof TAKEOVER_KINDS)[number];

/**
 * What an author would create to take over a page moss generates — computed
 * by the backend from the build's record, written by `create_files` in one
 * call, the LAST file opened afterwards. Mirrors `Takeover` in takeover.rs.
 * The editor never builds one of these from a URL; the one place it builds
 * one at all is `folderHomeTakeover`, for a folder the file tree already
 * knows by real path.
 */
export interface Takeover {
  kind: TakeoverKind;
  /** Rust directory truth: null is unknown; absent is a legacy/placeholder recipe, never evidence for cards. */
  folder_empty?: boolean | null;
  /** The name the pane shows: the folder, the root, the term, the language. */
  display: string;
  /**
   * For a `term-page`, the name-list field the claim is written through —
   * `author`, `editor`, `jury`, `tags`. It is what the pane's copy is worded
   * from, so one kind covers every namespace a site declares. `null` for
   * every other kind.
   */
  field: string | null;
  files: { dir: string; name: string; frontmatter: { [key in string]: JsonValue } }[];
}

/** Same shape as bindings' `JsonValue`, so the two `Takeover`s are interchangeable. */
type JsonValue = null | boolean | number | string | JsonValue[] | { [key in string]: JsonValue };

/**
 * The takeover for a home-less folder the file tree names by real path (a
 * folder click, a just-created folder, the context menu's "Create home
 * file"): a self-named home note, the same recipe the backend computes for
 * that folder's URL. `index` for the nameless root, so a bare `.md` is never
 * created.
 */
export function folderHomeTakeover(dir: string, name: string, isRoot = false): Takeover {
  return {
    kind: isRoot ? 'root-home' : 'folder-home',
    display: name,
    field: null,
    files: [{ dir, name: `${name || 'index'}.md`, frontmatter: { home: true } }],
  };
}

/** A node previewed/selected in the editor, resolved to its source/folder identity. */
export interface ResolvedNode {
  /** Absolute or project-relative source path of an editable file, or null. */
  sourcePath: string | null;
  /** True when the node is the site root home. */
  isRoot: boolean;
  /** What would replace the page when nothing backs it; null when nothing would. */
  takeover: Takeover | null;
  /**
   * The source exists but moss cannot read it yet — the bytes are still coming
   * down from the cloud (`PageSource.source_pending`).
   *
   * Optional so every existing caller keeps its exact meaning: absent is
   * "settled", which is what every node was before this existed.
   */
  sourcePending?: boolean;
}

/** The one surface the editor pane shows for a resolved node. */
export type EditorSurface =
  | { kind: 'content'; sourcePath: string }
  | { kind: 'root-empty'; folder: Takeover }
  /** A generated page the user can take over by creating the recipe's files. */
  | { kind: 'takeover'; takeover: Takeover }
  /**
   * The file is real and its bytes have not arrived. Every other no-content
   * surface here is a claim about the SITE ("this page is generated", "this
   * folder has no home page yet"); this one is a claim about the NETWORK, and
   * it is the only one that resolves itself — see the arrival retry in
   * `stale-source-refresh.ts`.
   *
   * `sourcePath` is the file the pane is waiting for when the editor knows it
   * (it opened that path and the read came back still-arriving), and null when
   * the pane came from resolving a preview URL that has no source yet. The
   * shell cannot re-derive it: its only other handle is the live preview URL,
   * which after a file-tree click names a different document entirely.
   */
  | { kind: 'pending'; sourcePath: string | null }
  | { kind: 'generated' };

/**
 * The single source of truth for what the editor pane shows. Every trigger
 * (boot, file-tree, preview-nav) routes a ResolvedNode through this.
 *
 * Directory emptiness comes from the Rust recipe, never from editable file count.
 */
export function decideEditorSurface(
  node: ResolvedNode,
): EditorSurface {
  if (node.sourcePath) {
    return { kind: 'content', sourcePath: node.sourcePath };
  }
  // Before every "there is nothing to edit here" answer, because all three of
  // them are lies about a file that is merely downloading — and the user has no
  // way to tell a lie from a verdict.
  if (node.sourcePending) {
    // Reached only with a null `sourcePath` (a non-null one is `content` above),
    // so this leg has no file to name — the URL it was resolved from is the
    // identity, and the shell already holds that.
    return { kind: 'pending', sourcePath: null };
  }
  if (node.takeover?.folder_empty === null && (node.takeover.kind === 'root-home' || node.takeover.kind === 'folder-home')) {
    return { kind: 'pending', sourcePath: null };
  }
  if (node.takeover?.folder_empty === true) {
    return { kind: 'root-empty', folder: node.takeover };
  }
  if (node.takeover) {
    return { kind: 'takeover', takeover: node.takeover };
  }
  return { kind: 'generated' };
}

/** The subset of the generated `PageSource` the source-truth resolve consumes. */
export interface EditSourceResolution {
  source_path: string | null;
  takeover?: Takeover | null;
  /** See `ResolvedNode.sourcePending`. */
  source_pending?: boolean;
}

/**
 * Resolve a preview URL path to the ResolvedNode the editor should mount — the
 * single, shared source-truth resolver for the editor's root-open paths (both
 * cold-boot `resolveAndApplyRootSurface` and warm/in-session `applyResolvedNode`
 * in editor-main.ts call it when a bare `{isRoot:true, sourcePath:null}` node
 * arrives).
 *
 * WHY (build-independent disk truth): the site root's on-disk home (index.md or
 * a self-named folder note like `William Blake.md`) must open in the editor even
 * while a large folder is still rendering its preview. Callers bake only `isRoot`
 * (derived from the live URL, build-independent); the source is resolved HERE
 * from disk truth:
 *   1. `isRoot` is derived from the url path with the shared `isRootUrlPath`
 *      rule, so a root url ('' / '/' / '///') always yields is_root=1.
 *   2. `sourcePath`/`takeover` come from a fresh `resolvePageSource`.
 *      The backend resolver (detect_root_home_source) returns the real home file
 *      for the empty root even mid-build (no article-map needed), so a root with
 *      a backing note resolves to `{kind:'content'}`.
 *
 * On resolve failure it falls back to the cached values (for the editor's root
 * calls, the bare node) so a resolve error never blanks a known source. A
 * genuinely generated page (feed/archive/404) resolves to
 * `{source_path:null, takeover:null}` with isRoot=false → the `generated`
 * surface is preserved; this does not invent a phantom source.
 */
export async function resolveNodeForEdit(
  currentUrlPath: string,
  cached: ResolvedNode,
  resolvePageSource: (
    urlPath: string,
  ) => Promise<{ status: 'ok'; data: EditSourceResolution } | { status: 'error'; error: string }>,
): Promise<ResolvedNode> {
  // Pure, build-independent root detection from the live URL — the same shared
  // rule the PREVIEW_URL_CHANGED handler uses (isRootUrlPath).
  const isRoot = isRootUrlPath(currentUrlPath);
  try {
    const res = await resolvePageSource(currentUrlPath);
    if (res.status === 'ok') {
      return {
        sourcePath: res.data.source_path,
        isRoot,
        takeover: res.data.takeover ?? null,
        // Read fresh every time, so a retry that finds the bytes landed clears
        // the waiting surface with no user action.
        sourcePending: res.data.source_pending ?? false,
      };
    }
  } catch {
    // fall through to cached fallback
  }
  // Resolve failed: keep the freshly-derived isRoot (still reliable) but reuse
  // the cached source/dir rather than blanking them.
  return {
    sourcePath: cached.sourcePath,
    isRoot,
    takeover: cached.takeover,
    sourcePending: cached.sourcePending,
  };
}

/**
 * Decide whether the content-page handler should (re)load the file.
 *
 * `sameFile` alone is NOT sufficient: a no-source pane-state tears down the CM6
 * editor, so after navigating to a no-source page → back to the same content
 * page, `sameFile` is true but the editor is gone. Reloading must also be forced when
 * CM6 is not currently mounted — otherwise the stale pane-state stays visible
 * and the file never re-renders (a silent desync, the very thing this check prevents).
 */
export function shouldLoadContentPage(sameFile: boolean, cmMounted: boolean): boolean {
  return !sameFile || !cmMounted;
}

/**
 * Serialize a ResolvedNode to the editor.html query string — the exact param
 * set + order build_editor_url (the host editor commands) bakes:
 * source, is_root, takeover (JSON); only present params appended; '' when none.
 * Used by the warm-open to history.replaceState the panel URL so a
 * dev-mode HMR full reload (or the navigate fallback) re-opens what the
 * editor is actually showing.
 */
export function editorQueryFromNode(node: ResolvedNode): string {
  const params: string[] = [];
  if (node.sourcePath) params.push(`source=${encodeURIComponent(node.sourcePath)}`);
  if (node.isRoot) params.push('is_root=1');
  if (node.takeover) params.push(`takeover=${encodeURIComponent(JSON.stringify(node.takeover))}`);
  return params.length ? `?${params.join('&')}` : '';
}
