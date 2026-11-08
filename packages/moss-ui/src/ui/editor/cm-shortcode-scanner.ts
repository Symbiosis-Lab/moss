/**
 * DEV-ONLY shortcode parse divergence check. Rendering comes from the Lezer
 * grammar (@symbiosis-lab/moss-syntax, shortcode.ts); this plugin runs only in dev builds and warns
 * when the authoritative Rust parse disagrees with the Lezer parse, catching
 * drift during development. No decorations, no rendering role.
 */
import { EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { topLevelLegacyDividerCount } from '@symbiosis-lab/moss-syntax/cm6';
import { engine } from '../engine';

function lezerBlockNames(view: EditorView): string[] {
  const names: string[] = [];
  syntaxTree(view.state).iterate({
    enter(n) {
      if (n.name === 'ShortcodeName') names.push(view.state.doc.sliceString(n.from, n.to));
    },
  });
  return names;
}

export function shortcodeDivergenceCheck(): Extension {
  if (!import.meta.env.DEV) return [];
  return ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;
      constructor(private view: EditorView) {}
      update(u: ViewUpdate) {
        if (!u.docChanged) return;
        if (this.timer) clearTimeout(this.timer);
        this.timer = setTimeout(() => void this.check(), 500);
      }
      destroy() { if (this.timer) clearTimeout(this.timer); }
      private async check() {
        const text = this.view.state.doc.toString();
        let res;
        try { res = await engine.scanShortcodes(text); } catch { return; }
        if (!res || res.status !== 'ok') return;
        // Belt-and-suspenders: the typed Result guarantees `data` when status
        // is 'ok', but this is a dev-only diagnostic — never let a malformed or
        // future-changed IPC payload crash the editor plugin. No-op if blocks
        // isn't an array.
        if (!res.data || !Array.isArray(res.data.blocks)) return;
        const rust = res.data.blocks.map((b) => b.name);
        const lezer = lezerBlockNames(this.view);
        const eq = rust.length === lezer.length && rust.every((n, i) => n === lezer[i]);
        if (!eq) {
          // eslint-disable-next-line no-console
          console.warn('[shortcode] Lezer/Rust parse divergence', { lezer, rust });
        }
        // Same check for the deprecated `---` divider. "A `---` is a cell
        // divider only inside `:::grid`" is written in both parsers now — the
        // Rust one drives the build warning, the TS one drives the editor
        // hint — so compare them rather than trust them to stay in step.
        const rustLegacy = res.data.blocks.reduce(
          (n, b) => n + (b.dividers ?? []).filter((d) => d.legacy).length, 0);
        const lezerLegacy = topLevelLegacyDividerCount(this.view.state);
        if (rustLegacy !== lezerLegacy) {
          // eslint-disable-next-line no-console
          console.warn('[shortcode] legacy `---` divider divergence',
            { lezer: lezerLegacy, rust: rustLegacy });
        }
      }
    }
  );
}
