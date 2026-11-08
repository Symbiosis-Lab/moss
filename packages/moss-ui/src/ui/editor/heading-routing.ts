import { toAbsolute, siblingPath } from './path-domain';
import type { HeadingSource } from '@symbiosis-lab/moss-ui';
import { t } from "../i18n/index";

/**
 * What moss-core last resolved for the open file. One value, so the text, the
 * routing discriminant and "has the first compute landed" cannot disagree —
 * see the header of editor-heading.ts, which owns the record. It is declared
 * HERE because this module is already the one that knows `HeadingSource`.
 */
export interface HeadingSavedState {
  /** The text `compute()` resolved — rendered verbatim. */
  text: string;
  /**
   * Which input it resolved from. Routes the commit: `Title` edits the
   * frontmatter, `Filename` renames the file. moss-core is the single owner of
   * this discriminant; nothing in the editor decides it.
   */
  source: HeadingSource;
}

export type RouteHeadingCommitResult =
  | { ok: true; renamedTo?: string }
  | { ok: false; error: string };

export interface RouteHeadingCommitArgs {
  newText: string;
  /** Project-root-relative path of the file currently open in the editor. */
  currentFilePath: string;
  /**
   * Absolute project root. Used to convert paths to absolute at the
   * `renameEntry` FS boundary — the Rust `rename_entry_with_refs` command
   * requires absolute paths (the planner canonicalizes each path against
   * the project root and rejects one that doesn't resolve inside it).
   * Mirrors `FileTree.commitRename`, which wraps both paths with `toAbsolute`.
   */
  projectRoot: string;
  /**
   * Which input moss-core's `compute()` resolved the heading text from.
   * Determines routing: `Title` edits the frontmatter, `Filename` renames
   * the file. moss-core is the single owner of this discriminant — the
   * editor reads it from `compute_heading_state`, it does not decide here.
   */
  source: HeadingSource;
  /**
   * Whether the first compute_heading_state has resolved. One-bit race
   * guard: commits dispatched before the initial load are rejected (the
   * captured `source` is not yet trustworthy).
   */
  headingStateLoaded: boolean;
  renameEntry(oldPath: string, newPath: string): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: string }>;
  /** Announce a rename moss is about to issue, so the watcher echo for
   *  either side is swallowed rather than reloading a path mid-move. */
  registerPendingRename(from: string, to: string): void;
  /**
   * Title-mode commit: write the `title:` frontmatter field. Empty string is
   * written verbatim (`title: ""`), which makes `compute()` return
   * `visible:false` and the heading hides on the next recompute — see the
   * `empty_title` rule in `crates/moss-core/src/heading/state.rs`. `undefined` is
   * never passed here (that would delete the field).
   */
  setTitleField(value: string): void;
}

export async function routeHeadingCommit(args: RouteHeadingCommitArgs): Promise<RouteHeadingCommitResult> {
  const { newText, currentFilePath, projectRoot, source, headingStateLoaded, renameEntry, registerPendingRename, setTitleField } = args;

  if (!headingStateLoaded) {
    return { ok: false, error: t('heading.state_not_loaded') };
  }

  if (source === 'Title') {
    // Title mode mirrors the preview: editing the heading edits the
    // frontmatter `title:`. Empty is allowed and intentional — it writes
    // `title: ""`, hiding the heading (mirrors compute()'s empty_title rule).
    setTitleField(newText);
    return { ok: true };
  }

  // Filename mode: editing the heading renames the file.
  if (newText === '') {
    return { ok: false, error: t('heading.filename_empty') };
  }

  const lastSlash = currentFilePath.lastIndexOf('/');
  const oldName = currentFilePath.slice(lastSlash + 1);
  const dot = oldName.lastIndexOf('.');
  const ext = dot > 0 ? oldName.slice(dot) : '';
  // Shared rename-path math with FileTree.startRename (root-level safe).
  const newPath = siblingPath(currentFilePath, `${newText}${ext}`);

  // Relative paths — the domain the watcher echo carries. Both sides, so the
  // echo about the NEW name is swallowed too; registering only the old one
  // left the arrival of `newPath` looking like an external edit.
  registerPendingRename(currentFilePath, newPath);
  // rename_entry (Rust) requires absolute paths; convert at the FS boundary.
  const result = await renameEntry(
    toAbsolute(currentFilePath, projectRoot),
    toAbsolute(newPath, projectRoot),
  );
  // renamedTo stays relative — the editor (loadFile, registry, events) is
  // relative-domain; toRelative is idempotent at consumers anyway.
  if (result.status === 'ok') return { ok: true, renamedTo: newPath };
  return { ok: false, error: result.error };
}
