import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { linter, forceLinting } from '@codemirror/lint';
import { getHeadingState, readDiagnostics } from '../moss-debug';

// The real editor heading is `#editor-heading` — a contenteditable <div>
// (class `editor-heading`, NOT an <h1>). Text lives in `.textContent`.
// Visibility is encoded by the `data-empty` attribute set in
// `applyHeadingState` (`data-empty="false"` ⇒ visible heading,
// `data-empty="true"` ⇒ hidden/empty). The HeadingSource ('Title' | 'Filename')
// is reflected to a `data-heading-source` attribute on the same element
// (lowercased: 'title' | 'filename'). See editor-heading.ts / editor.html.
describe('getHeadingState', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('reads text/source/visible from the rendered heading element (title source, visible)', () => {
    document.body.innerHTML =
      '<div class="editor-heading" id="editor-heading" contenteditable="plaintext-only" ' +
      'data-empty="false" data-heading-source="title">Hello World</div>';
    expect(getHeadingState()).toEqual({ text: 'Hello World', source: 'title', visible: true });
  });

  it('reads filename source and reports visible:false when data-empty="true"', () => {
    document.body.innerHTML =
      '<div class="editor-heading" id="editor-heading" contenteditable="plaintext-only" ' +
      'data-empty="true" data-heading-source="filename">post</div>';
    expect(getHeadingState()).toEqual({ text: 'post', source: 'filename', visible: false });
  });

  it('reports null/false when the heading element is absent', () => {
    document.body.innerHTML = '<div class="editor-root"></div>';
    expect(getHeadingState()).toEqual({ text: null, source: null, visible: false });
  });

  it('reports source:null when data-heading-source is not yet set', () => {
    document.body.innerHTML =
      '<div class="editor-heading" id="editor-heading" data-empty="false">Untitled</div>';
    expect(getHeadingState()).toEqual({ text: 'Untitled', source: null, visible: true });
  });
});

// readDiagnostics is the production form of the e2e harness's collectDiagnostics:
// it lets a headless agent read CM6 lint diagnostics as structured data (no focus,
// no paint, no rAF) after forcing a lint run via __moss_debug.forceLint().
describe('readDiagnostics', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('returns lint diagnostics as structured data with 1-based line numbers', async () => {
    // A trivial linter flagging chars [6,10) — which sit on line 2 — as a warning.
    const src = linter(() => [
      { from: 6, to: 10, severity: 'warning' as const, message: 'bad ref' },
    ]);
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc: 'line1\nline2word', extensions: [src] }),
      parent: container,
    });
    try {
      forceLinting(view);
      await new Promise((r) => setTimeout(r, 50));
      await Promise.resolve();

      expect(readDiagnostics(view)).toEqual([
        { from: 6, to: 10, line: 2, severity: 'warning', message: 'bad ref', source: null },
      ]);
    } finally {
      view.destroy();
    }
  });

  it('returns an empty array when there are no diagnostics', () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc: 'clean', extensions: [linter(() => [])] }),
      parent: container,
    });
    try {
      expect(readDiagnostics(view)).toEqual([]);
    } finally {
      view.destroy();
    }
  });
});
