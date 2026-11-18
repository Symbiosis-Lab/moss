/**
 * editor-position.ts — a place in a file the editor should put its caret, and
 * the events that take it there once the editor has loaded that file.
 */

import { host } from "../../host";
import { normalizePath } from '../path-domain';
import type { OpenFileInEditorEvent } from '../../context/event-bus';

/** What a preview menu row does to the picture reference at its line: replace it, put the keyboard on its focus point, select its description, show its file in the tree. */
export type PictureAct = 'replace' | 'focus' | 'description' | 'reveal';
/** An act on the `picture`th picture reference on a line, counted from 0. */
export interface PictureActRequest { act: PictureAct; picture: number }

export type EditorPosition =
  | { kind: 'line'; line: number; act?: PictureActRequest }
  | { kind: 'field'; field: string; seed?: unknown }
  | { kind: 'rule'; selector: string }
  | { kind: 'preflight'; open: OpenFileInEditorEvent };

/** The position a preview source target maps to, or null for a bare page. */
export function sourceTargetPosition(target: { kind: string; line?: number; startLine?: number; field?: string; act?: PictureActRequest }): EditorPosition | null {
  const line = target.kind === 'body-line' ? target.line : target.kind === 'body-range' ? target.startLine : undefined;
  if (typeof line === 'number') return target.act ? { kind: 'line', line, act: target.act } : { kind: 'line', line };
  if (target.kind === 'frontmatter' && typeof target.field === 'string') {
    // No schema default: nav's effective value depends on where the page sits, and a page drawn in the nav is in it.
    if (target.field === 'nav') return { kind: 'field', field: 'nav', seed: true };
    return { kind: 'field', field: target.field };
  }
  return null;
}

export const EDITOR_GOTO_LINE = 'editor:goto-line';
/** After the caret is on the line: `{ line } & PictureActRequest`. */
export const EDITOR_PICTURE_ACT = 'editor:picture-act';
export const EDITOR_OPEN_FIELD = 'editor:open-field';
export const EDITOR_GOTO_RULE = 'editor:goto-rule';
export const EDITOR_OPEN_PUBLISH_PREFLIGHT = 'editor:open-publish-preflight';

export function deliverEditorPosition(at: EditorPosition): void {
  if (at.kind === 'line') {
    host.bus.emit(EDITOR_GOTO_LINE, { line: at.line });
    if (at.act) host.bus.emit(EDITOR_PICTURE_ACT, { line: at.line, ...at.act });
  }
  else if (at.kind === 'field') {
    host.bus.emit(EDITOR_OPEN_FIELD, at.seed === undefined ? { field: at.field } : { field: at.field, seed: at.seed });
  }
  else if (at.kind === 'rule') host.bus.emit(EDITOR_GOTO_RULE, { selector: at.selector });
  else host.bus.emit(EDITOR_OPEN_PUBLISH_PREFLIGHT, at.open);
}

/**
 * One position waiting for the editor to report its file loaded
 * (`editor:file-ready`). Good for the next load only: any ready clears it.
 * `path` null means whichever file loads.
 */
export class EditorHandshake {
  private pending: { path: string | null; at: EditorPosition } | null = null;

  /** `normalize` maps either side to the same project-relative form. */
  constructor(private readonly normalize: (path: string) => string) {}

  expect(path: string | null, at: EditorPosition | null): void {
    this.pending = at ? { path, at } : null;
  }

  ready(path: string): void {
    const pending = this.pending;
    if (!pending) return;
    this.pending = null;
    if (pending.path !== null && normalizePath(this.normalize(pending.path)) !== normalizePath(this.normalize(path))) return;
    deliverEditorPosition(pending.at);
  }
}
