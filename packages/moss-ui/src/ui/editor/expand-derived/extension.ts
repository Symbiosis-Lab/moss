// "Expand": the link-styled action at the end of a `![[file.pdf#text]]` line.
//
// The line stands for text a plugin derived from the file. Once that text is
// ready the author may take it over: Expand replaces the line with the text as
// ordinary markdown, as ONE edit in the editor's own history, saved by the
// normal auto-save like typing. Nothing is drawn while the text is still being
// read or when the plugin declined the file, and nothing here writes to disk.
//
// Readiness is learnt, not guessed: one probe per document open (and again when
// the tree's derivation state refreshes) asks the backend to expand the whole
// page and diffs its answer against the document. The lines it would change are
// the ready ones (plan.ts). The probe is never per keystroke; an edit keeps the
// readiness of the lines it left alone and drops the rest. The backend stays
// behind `expand`, so this module imports no bindings.

import { type EditorState, type Extension, StateEffect, StateField, Transaction } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, WidgetType } from '@codemirror/view';
import { isolateHistory } from '@codemirror/commands';
import { blockEmbedsIn } from '../cm-block-embeds';
import {
  DERIVATIONS_REFRESHED, changesFor, derivedFragmentOf, diffLines, hunksForLine, maskLines, readyLines,
} from './plan';
import { headFor, patchBetween, splitPage, type FrontmatterPatch, type FrontmatterValues } from './frontmatter';

export interface ExpandResult { content: string; replaced: number }

export interface ExpandDerivedOptions {
  /** The backend's expansion of `content`, or null when it could not be asked. */
  expand(content: string): Promise<ExpandResult | null>;
  /** The page's front matter, held outside the document (the chip bar's store). */
  getFrontmatter?(): FrontmatterValues;
  /** Write fields back through the store's own update path, as a chip edit does. */
  applyFrontmatter?(patch: FrontmatterPatch): void;
  /** The title, description and date the page's embeds would give, learnt by the probe (empty when the page takes none). */
  onFacts?(facts: { title?: string; description?: string; date?: string }): void;
  /** The action's word, read at draw time so a locale switch stays live. */
  label(): string;
}

/** Every line that holds only a derived-text embed, as 0-based line index plus line start. */
export function derivedEmbedLines(state: EditorState): { index: number; from: number; to: number; text: string }[] {
  const out: { index: number; from: number; to: number; text: string }[] = [];
  for (const b of blockEmbedsIn(state, 0, state.doc.length)) {
    if (b.unit || b.parts.syntax !== 'wikilink-embed' || derivedFragmentOf(b.parts.target) === null) continue;
    const line = state.doc.lineAt(b.line.from);
    out.push({ index: line.number - 1, from: line.from, to: line.to, text: line.text });
  }
  return out;
}

interface Ready { from: number; text: string }

const setReady = StateEffect.define<{ doc: string; ready: Ready[] }>();
const dropReady = StateEffect.define<number>();

const readyField = StateField.define<Ready[]>({
  create: () => [],
  update(value, tr) {
    let next = value;
    for (const e of tr.effects) {
      if (e.is(setReady) && e.value.doc === tr.state.doc.toString()) next = e.value.ready;
      else if (e.is(dropReady)) next = next.filter((r) => r.from !== e.value);
    }
    if (!tr.docChanged || next.length === 0) return next;
    // Keep a line's readiness only while the line is still exactly what was probed.
    return next.flatMap((r) => {
      const from = tr.changes.mapPos(r.from, 1);
      return tr.state.doc.lineAt(from).text === r.text && tr.state.doc.lineAt(from).from === from ? [{ from, text: r.text }] : [];
    });
  },
});

class ExpandWidget extends WidgetType {
  constructor(private readonly lineFrom: number, private readonly label: string, private readonly run: (view: EditorView, lineFrom: number) => void) {
    super();
  }
  eq(other: ExpandWidget): boolean { return other.lineFrom === this.lineFrom && other.label === this.label; }
  toDOM(view: EditorView): HTMLElement {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'moss-link cm-expand-derived';
    b.textContent = this.label;
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => this.run(view, this.lineFrom));
    return b;
  }
  ignoreEvent(): boolean { return true; }
}

/** `expand` asked about the whole page: the body with the head the store implies. Returns the body half and what happened to the head. */
async function ask(opts: ExpandDerivedOptions, body: string, forFacts = false): Promise<(ExpandResult & { patch: FrontmatterPatch }) | null> {
  const values = opts.getFrontmatter?.() ?? {};
  // For the facts alone the author's own title and description are left out, so the answer names what the embeds give even where the page already overrides it.
  const head = headFor(forFacts ? { inferred: values.inferred } : values);
  const r = await opts.expand(head + body);
  if (!r) return null;
  const page = splitPage(head, r.content);
  return { content: page.body, replaced: r.replaced, patch: patchBetween(head, page.head) };
}

/** Replace the embed line at `lineFrom` with its derived text, as one history step. */
async function expandLine(view: EditorView, lineFrom: number, opts: ExpandDerivedOptions): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt++) {
    const state = view.state;
    const content = state.doc.toString();
    const embeds = derivedEmbedLines(state);
    const target = embeds.find((e) => e.from === lineFrom);
    if (!target) return;
    const current = content.split('\n');
    const ready = state.field(readyField).map((r) => state.doc.lineAt(r.from).number - 1).sort((a, b) => a - b);
    // Mask every other derived embed so the answer holds this line's expansion alone.
    const { masked, restore } = maskLines(current, embeds.filter((e) => e !== target).map((e) => e.index));
    const result = await ask(opts, masked);
    if (view.state.doc.toString() !== content) continue; // the page moved while we asked
    if (!result || result.replaced === 0) {
      view.dispatch({ effects: dropReady.of(lineFrom) });
      return;
    }
    const next = restore(result.content).split('\n');
    const isFirst = ready[0] === undefined || ready[0] >= target.index;
    const hunks = hunksForLine(diffLines(current, next), target.index, isFirst);
    if (hunks.length === 0) {
      view.dispatch({ effects: dropReady.of(lineFrom) });
      return;
    }
    // Front matter first, body last: one undo then brings the embed line back
    // while the title stays, never the title gone with the embed already written out.
    if (isFirst) opts.applyFrontmatter?.(result.patch);
    view.dispatch({
      changes: changesFor(current, next, hunks),
      // An ordinary edit to history and auto-save, its own undo step.
      userEvent: 'input.expand',
      annotations: isolateHistory.of('full'),
    });
    return;
  }
}

export function expandDerivedExtension(opts: ExpandDerivedOptions): Extension {
  const run = (view: EditorView, lineFrom: number): void => { void expandLine(view, lineFrom, opts); };

  const decorations = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      const before = tr.startState.field(readyField);
      const after = tr.state.field(readyField);
      if (before === after && !tr.docChanged) return value;
      const label = opts.label();
      return Decoration.set(after.flatMap((r) => {
        const line = tr.state.doc.lineAt(r.from);
        return [Decoration.widget({ widget: new ExpandWidget(line.from, label, run), side: 1 }).range(line.to)];
      }), true);
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  const probe = ViewPlugin.define((view) => {
    let alive = true;
    let running = false;
    const probeNow = async (): Promise<void> => {
      if (running) return;
      const state = view.state;
      const embeds = derivedEmbedLines(state);
      // Nothing to learn, or everything already known.
      const known = new Set(state.field(readyField).map((r) => r.from));
      if (embeds.length === 0 || embeds.every((e) => known.has(e.from))) return;
      running = true;
      try {
        const content = state.doc.toString();
        const result = await ask(opts, content, true);
        if (!alive || view.state.doc.toString() !== content) return;
        opts.onFacts?.({ title: result?.patch.title, description: result?.patch.description, date: result?.patch.date });
        const ready = result && result.replaced > 0
          ? readyLines(embeds.map((e) => e.index), diffLines(content.split('\n'), result.content.split('\n')))
          : [];
        view.dispatch({
          effects: setReady.of({
            doc: content,
            ready: embeds.filter((e) => ready.includes(e.index)).map((e) => ({ from: e.from, text: e.text })),
          }),
        });
      } finally {
        running = false;
      }
    };
    queueMicrotask(() => { void probeNow(); });
    const onRefresh = (): void => { void probeNow(); };
    document.addEventListener(DERIVATIONS_REFRESHED, onRefresh);
    return {
      // A document set from outside (the file loaded after the view, a reload
      // from disk) carries no user event, and an undo or redo can bring an embed
      // line back: those are pages to learn again, whereas typing never is.
      update(u) {
        if (u.transactions.some((tr) => tr.docChanged && (tr.annotation(Transaction.userEvent) === undefined || tr.isUserEvent('undo') || tr.isUserEvent('redo')))) {
          queueMicrotask(() => { void probeNow(); });
        }
      },
      destroy() { alive = false; document.removeEventListener(DERIVATIONS_REFRESHED, onRefresh); },
    };
  });

  return [readyField, decorations, probe];
}


type ExpandCommand = (page: string, content: string) => Promise<{ status: 'ok'; data: ExpandResult } | { status: 'error'; error: string }>;

/** `expand` for the editor: the engine asked about this page, null when no page is open, no site is, or the call fails. */
export function expandViaBackend(command: ExpandCommand, page: () => string | null | undefined): ExpandDerivedOptions['expand'] {
  return async (content) => {
    const path = page();
    if (!path) return null;
    try {
      const r = await command(path, content);
      return r.status === 'ok' ? r.data : null;
    } catch {
      return null;
    }
  };
}
