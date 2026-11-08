// packages/moss-ui/src/ui/editor/moss-debug.ts
//
// Unambiguous DOM-state hooks for agent-driven verification.
// Pure readers — no side effects, no Tauri IPC. Exposed via `window.__moss_debug`
// in editor-main.ts and consumed by the e2e harness (bridge-drive.cjs + the
// mcp-server-tauri webview, action-panel windowId).
//
// The contract { text, source, visible } is fixed; the DOM it reads is the real
// editor heading. The heading is NOT an `<h1>` — it is `#editor-heading`, a
// contenteditable <div> (see editor.html / editor-heading.ts). Visibility is
// encoded by the `data-empty` attribute that `applyHeadingState` sets
// (`data-empty="false"` ⇒ visible, `"true"` ⇒ hidden). The HeadingSource
// ('Title' | 'Filename', the moss-core-owned discriminant) is reflected to a
// `data-heading-source` attribute (lowercased) on the same element.

import { forEachDiagnostic, type Diagnostic } from '@codemirror/lint';
import type { EditorView } from '@codemirror/view';

export interface HeadingState {
  text: string | null;
  source: string | null;
  visible: boolean;
}

/** A CM6 lint diagnostic flattened to plain data an agent can assert on. */
export interface EditorDiagnostic {
  from: number;
  to: number;
  /** 1-based line number of `from`, for human-readable assertions. */
  line: number;
  /** 'error' | 'warning' | 'info' | 'hint'. */
  severity: Diagnostic['severity'];
  message: string;
  /** The Diagnostic.source tag, if the lint source set one; else null. */
  source: string | null;
}

/**
 * Read all current lint diagnostics from the editor view as structured data.
 *
 * Pure reader over `view.state` (no DOM, no paint, no rAF) — this is what makes
 * lint warnings (a JSON file's) verifiable from a headless agent; a markdown
 * document's problems are read by `readProblems` (cm-problems.ts) instead: pair it with
 * `__moss_debug.forceLint()` (which runs the lint sources immediately, bypassing
 * the throttled debounce that never fires in an unfocused webview), then read the
 * settled diagnostics here. Production form of the e2e harness's
 * `collectDiagnostics` helper.
 */
export function readDiagnostics(view: EditorView): EditorDiagnostic[] {
  const out: EditorDiagnostic[] = [];
  forEachDiagnostic(view.state, (d: Diagnostic, from: number, to: number) => {
    out.push({
      from,
      to,
      line: view.state.doc.lineAt(from).number,
      severity: d.severity,
      message: d.message,
      source: (d.source as string | undefined) ?? null,
    });
  });
  return out;
}

export function getHeadingState(): HeadingState {
  const el = document.getElementById('editor-heading');
  if (!el) return { text: null, source: null, visible: false };
  return {
    text: el.textContent,
    source: el.getAttribute('data-heading-source'),
    // `data-empty="false"` is the editor's "heading is shown" signal.
    visible: el.getAttribute('data-empty') === 'false',
  };
}
