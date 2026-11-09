/**
 * Regression: a classified media embed (`![[demo.mp4]]`) must render exactly
 * ONE block widget through the real CmEditor wiring.
 *
 * The legacy extension-sniffing `EmbedCardWidget` (cm-embed-cards.ts) was
 * superseded by the kind-routed `MediaEmbedBlockWidget` (the shared
 * embed renderer in cm-media-embed.ts) but stayed registered alongside it in
 * cm-editor.ts — so once the reference batch classified the target, the same
 * embed got TWO cards (legacy above the line, kind-routed below it).
 *
 * jsdom note: widget DOM never materializes without a real measure pass, so
 * the assertion counts block-widget DECORATIONS across every StateField that
 * provides into `EditorView.decorations` — the layer where the duplication
 * exists — rather than querying rendered cards.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { CmEditor } from '../cm-editor';

vi.mock('../../engine', () => ({
  engine: {
    setImageWidth: vi.fn(),
    // Classify every embed target as a video — the kind-routed media-card path.
    editorResolveReferences: vi.fn(
      async (targets: Array<{ text: string; is_embed: boolean }>) => ({
        status: 'ok',
        data: targets.map((t) => ({
          target: t.text,
          is_embed: t.is_embed,
          kind: { kind: 'video' },
          resolved: null,
          url: null,
          anchor: null,
          resolved_path: null,
          message: null,
        })),
      }),
    ),
  },
}));

/** Count block widget decorations across all StateField-provided sets. */
function countBlockWidgets(view: EditorView): number {
  let count = 0;
  for (const provided of view.state.facet(EditorView.decorations)) {
    if (typeof provided === 'function') continue; // ViewPlugin-provided
    const it = (provided as DecorationSet).iter();
    while (it.value) {
      const spec = it.value.spec as { block?: boolean; widget?: unknown };
      if (spec.block === true && spec.widget != null) count++;
      it.next();
    }
  }
  return count;
}

describe('media embed renders exactly one card', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('classified ![[demo.mp4]] yields ONE block widget, not two', async () => {
    const editor = new CmEditor({
      content: '![[demo.mp4]]',
      container,
      onChange: () => {},
      getFromFile: () => 'note.md',
    });
    const view = editor.getView();

    // Let the reference resolver's 100ms debounce fire, the (mocked) batch
    // land, and the refsResolvedEffect rebuild the decoration fields.
    await new Promise((r) => setTimeout(r, 200));

    expect(countBlockWidgets(view)).toBe(1);
    editor.destroy();
  });
});
