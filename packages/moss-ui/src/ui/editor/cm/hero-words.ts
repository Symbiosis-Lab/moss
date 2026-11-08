/**
 * The hero-words instrument, editor half: with the caret on a `:::hero` opening
 * line the shell draws a box over the preview where the hero's words are, and
 * when the box is dropped at start, centre or end the shell asks for the
 * `align=` rewrite here. The editor makes the write; the shell never does.
 */

import type { Bus } from "../../host";
import { isolateHistory } from '@codemirror/commands';
import type { Extension } from '@codemirror/state';
import { ViewPlugin, type EditorView, type ViewUpdate } from '@codemirror/view';
import {
  HERO_ALIGN, HERO_ALIGNS, HERO_WORDS, isLine,
  type HeroAlignRequest, type HeroWordsAt,
} from '../session/instrument-events';
import { heroLineWithAlign } from './hero-align';
import { pictureAtCaretField } from './picture-context';

/** `path` is the editor file's project-relative path. */
export function heroWordsExtension(path: () => string, host: Bus): Extension {
  const plugin = ViewPlugin.fromClass(class {
    private line: number | null = null;
    private readonly off: () => void;
    private readonly onVisible = () => this.say(document.hidden ? null : this.openerLine(), true);

    constructor(private readonly view: EditorView) {
      this.off = host.on<HeroAlignRequest>(HERO_ALIGN, (r) => this.align(r));
      document.addEventListener('visibilitychange', this.onVisible);
      this.say(this.openerLine());
    }

    update(u: ViewUpdate): void {
      if (u.selectionSet || u.docChanged) this.say(this.openerLine());
    }

    destroy(): void {
      this.off();
      document.removeEventListener('visibilitychange', this.onVisible);
      this.say(null);
    }

    private openerLine(): number | null {
      const hero = this.view.state.field(pictureAtCaretField);
      return hero?.opener ? hero.frameLine : null;
    }

    /** Tell the shell where the caret is, when that moved (or `force`, for a shell that lost track). */
    private say(line: number | null, force = false): void {
      if (line === this.line && !force) return;
      this.line = line;
      host.emit(HERO_WORDS, { path: path(), line } satisfies HeroWordsAt);
    }

    private align({ line, align }: HeroAlignRequest): void {
      if (!isLine(line) || line !== this.line || !HERO_ALIGNS.includes(align)) return;
      const row = this.view.state.doc.line(line);
      const insert = heroLineWithAlign(row.text, align);
      if (insert === row.text) return;
      this.view.dispatch({ changes: { from: row.from, to: row.to, insert }, userEvent: 'input.instrument', annotations: isolateHistory.of('full') });
    }
  });
  return plugin;
}
