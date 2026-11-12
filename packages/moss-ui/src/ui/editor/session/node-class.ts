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

/**
 * Why the editor cannot show a page's source yet: moss has no answer for the
 * page (`lookup`: the lookup failed, or the build has not mapped the page), or
 * the source is real and its bytes are still coming down from the cloud
 * (`download`).
 */
export type PendingReason = 'lookup' | 'download';

/** A node previewed/selected in the editor, resolved to its source/folder identity. */
export interface ResolvedNode {
  /** Absolute or project-relative source path of an editable file, or null. */
  sourcePath: string | null;
  /** True when the node is the site root home. */
  isRoot: boolean;
  /** What would replace the page when nothing backs it; null when nothing would. */
  takeover: Takeover | null;
  /** Null when the node is an answer; otherwise why it is not one yet. */
  pending: PendingReason | null;
}

/**
 * A node as it crosses into the editor. The shell's page answers carry their
 * verdict; the native open's bake and an explicit file open carry none.
 */
export interface EditorNodeWire {
  sourcePath: string | null;
  isRoot: boolean;
  takeover: Takeover | null;
  pending?: PendingReason | null;
}

/**
 * The one reading of a node off the wire. A node with no verdict was baked by
 * the native open or sent by an explicit open, both from the same resolver as
 * the shell's answers, so a source or a takeover in it is an answer. A bare
 * root is not, and its home is found from disk (`homeFromDisk`). Any other
 * bare node names no page yet, and waits for the shell's answer.
 */
export function readEditorNode(wire: EditorNodeWire): { node: ResolvedNode; homeFromDisk: boolean } {
  const { sourcePath, isRoot } = wire;
  const takeover = wire.takeover ?? null;
  if (wire.pending !== undefined) return { node: { sourcePath, isRoot, takeover, pending: wire.pending }, homeFromDisk: false };
  if (sourcePath || takeover) {
    return { node: { sourcePath, isRoot, takeover, pending: pendingOf({ source_path: sourcePath, takeover }) }, homeFromDisk: false };
  }
  return { node: { sourcePath, isRoot, takeover, pending: 'lookup' }, homeFromDisk: isRoot };
}

/** What a resolver's answer leaves unknown about the page, if anything. */
export function pendingOf(answer: EditSourceResolution): PendingReason | null {
  const takeover = answer.takeover;
  // A home whose folder moss cannot list yet is a folder still coming down.
  const folderArriving = takeover?.folder_empty === null && (takeover.kind === 'root-home' || takeover.kind === 'folder-home');
  if (answer.source_pending || folderArriving) return 'download';
  // Missing on an older backend, which only answers what it knows.
  if (answer.source_known === false) return 'lookup';
  return null;
}

/** The one surface the editor pane shows for a resolved node. */
export type EditorSurface =
  | { kind: 'content'; sourcePath: string }
  | { kind: 'root-empty'; folder: Takeover }
  /** A generated page the user can take over by creating the recipe's files. */
  | { kind: 'takeover'; takeover: Takeover }
  /**
   * The page has no answer yet, for `reason`. Only a `download` resolves
   * itself, through the arrival retry in `stale-source-refresh.ts`.
   *
   * `sourcePath` is the file the pane is waiting for when the editor knows it
   * (it opened that path and the read came back still-arriving), and null when
   * the pane came from resolving a preview URL that has no source yet. The
   * shell cannot re-derive it: its only other handle is the live preview URL,
   * which after a file-tree click names a different document entirely.
   */
  | { kind: 'pending'; reason: PendingReason; sourcePath: string | null }
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
  if (node.pending) return { kind: 'pending', reason: node.pending, sourcePath: node.sourcePath };
  if (node.sourcePath) {
    return { kind: 'content', sourcePath: node.sourcePath };
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
  /** The source exists and its bytes are still coming down from the cloud. */
  source_pending?: boolean;
  /**
   * False when the build's page map is missing or unreadable and the disk
   * found no source; the next build reload settles it. Not yet in the
   * generated `PageSource`, which follows the backend.
   */
  source_known?: boolean;
}

/**
 * Resolve a preview URL path to the ResolvedNode the editor should mount, from
 * disk: the editor's own resolve for the root's home and for a folder it names
 * by real path.
 *
 * WHY (build-independent disk truth): the site root's on-disk home (index.md or
 * a self-named folder note like `William Blake.md`) must open in the editor even
 * while a large folder is still rendering its preview. `isRoot` is derived from
 * the url path with the shared `isRootUrlPath` rule, and the source comes from
 * a fresh `resolvePageSource`, whose root resolver finds the home with no
 * article-map.
 *
 * A failed resolve is a `lookup` verdict, never a claim that the page is
 * generated. A genuinely generated page (feed/archive/404) resolves to
 * `{source_path:null, takeover:null}` and keeps the `generated` surface.
 */
export async function resolveNodeForEdit(
  currentUrlPath: string,
  resolvePageSource: (
    urlPath: string,
  ) => Promise<{ status: 'ok'; data: EditSourceResolution } | { status: 'error'; error: string }>,
): Promise<ResolvedNode> {
  const isRoot = isRootUrlPath(currentUrlPath);
  try {
    const res = await resolvePageSource(currentUrlPath);
    if (res.status === 'ok') {
      return { sourcePath: res.data.source_path, isRoot, takeover: res.data.takeover ?? null, pending: pendingOf(res.data) };
    }
  } catch {
    // A failed resolve is no evidence about the page.
  }
  return { sourcePath: null, isRoot, takeover: null, pending: 'lookup' };
}

/**
 * Decide whether the content-page handler should (re)load the file.
 *
 * `sameFile` alone is NOT sufficient: a no-source pane-state tears down the CM6
 * editor, so after navigating to a no-source page → back to the same content
 * page, `sameFile` is true but the editor is gone. Reloading must also be forced when
 * the file is not currently on screen — otherwise the stale pane-state stays visible
 * and the file never re-renders (a silent desync, the very thing this check prevents).
 * `mounted` is CM6 for a text file and the file viewer for any other.
 */
export function shouldLoadContentPage(sameFile: boolean, mounted: boolean): boolean {
  return !sameFile || !mounted;
}

/**
 * Serialize a node to the editor.html query string — the exact param
 * set + order build_editor_url (the host editor commands) bakes:
 * source, is_root, takeover (JSON); only present params appended; '' when none.
 * Used by the warm-open to history.replaceState the panel URL so a
 * dev-mode HMR full reload (or the navigate fallback) re-opens what the
 * editor is actually showing. Only answers are baked, so no verdict rides it.
 */
export function editorQueryFromNode(node: EditorNodeWire): string {
  const params: string[] = [];
  if (node.sourcePath) params.push(`source=${encodeURIComponent(node.sourcePath)}`);
  if (node.isRoot) params.push('is_root=1');
  if (node.takeover) params.push(`takeover=${encodeURIComponent(JSON.stringify(node.takeover))}`);
  return params.length ? `?${params.join('&')}` : '';
}

/** The node `build_editor_url` baked into the query; a hand-edited takeover reads as none. */
export function editorNodeFromQuery(params: URLSearchParams): EditorNodeWire {
  let takeover: Takeover | null = null;
  try { takeover = JSON.parse(params.get('takeover') ?? 'null'); } catch { /* malformed query */ }
  if (takeover && (!TAKEOVER_KINDS.includes(takeover.kind) || typeof takeover.display !== 'string' || !Array.isArray(takeover.files))) takeover = null;
  return { sourcePath: params.get('source'), isRoot: params.get('is_root') === '1', takeover };
}
