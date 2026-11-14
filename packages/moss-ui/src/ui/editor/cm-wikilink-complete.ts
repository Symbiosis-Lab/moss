// The editor's completion extension: the one `autocompletion()` call, the
// code-fence guard it wraps the source in, and the `[[` bracket auto-close.
//
// The source itself — where the caret is, what it asks the backend for, how a
// row is rendered and applied — is `cm-completion.ts`. Everything
// about WHICH targets exist and what text each inserts is Rust
// (`wikilink_completions`).

import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from '@codemirror/autocomplete';
import { completionSource } from './cm-completion.js';
import { syntaxTree } from '@codemirror/language';
import type { Extension, EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';

export type { WikilinkCompleteOptions, WikilinkCompletionItem } from '@symbiosis-lab/moss-syntax';
import type { WikilinkCompleteOptions } from '@symbiosis-lab/moss-syntax';

/**
 * Decide whether typing `[` should auto-close the wikilink brackets.
 *
 * Obsidian/VS Code convention: typing the opening of a bracket pair inserts
 * the closer with the cursor between. We scope this to wikilinks only (rather
 * than enabling CM6's global `closeBrackets()` for every `(` `{` `"` etc.) to
 * keep moss's typing surface minimal — the "one obvious way" principle.
 *
 * Fires when the just-pressed `[` completes a `[[` (or `![[`): i.e. the char
 * immediately before the cursor is already `[`. Returns the text to insert
 * (`[]]` — the typed `[` plus the auto `]]`) and the caret offset within it
 * (1, between `[[` and `]]`), or null to let CM6 insert the `[` normally.
 *
 * Guards against doubling: if `]]` already immediately follows the cursor
 * (re-typing inside an existing pair), insert just the `[`.
 */
export function wikilinkBracketClose(
  beforeCursor: string,
  afterCursor: string,
): { insert: string; caretOffset: number } | null {
  // The char right before the cursor must be `[` (so this `[` makes `[[`).
  if (!beforeCursor.endsWith('[')) return null;
  // ...but not `[[[` — only the SECOND `[` of a fresh pair triggers close.
  if (beforeCursor.endsWith('[[')) return null;
  // Don't double an existing closer.
  if (afterCursor.startsWith(']]')) return { insert: '[', caretOffset: 1 };
  return { insert: '[]]', caretOffset: 1 };
}

// ── CM6 completion source ─────────────────────────────────────────────

/**
 * Lezer (markdown) node names that mean "the cursor is inside code". Wikilink
 * completion must stay silent here so typing `[[` in a fenced/inline code block
 * does not pop the menu.
 *   - FencedCode / CodeText — ``` fenced blocks (CodeText is the fence body)
 *   - CodeBlock             — indented code blocks
 *   - InlineCode            — `inline code`
 */
const CODE_NODE_NAMES = new Set([
  'FencedCode',
  'CodeText',
  'CodeBlock',
  'InlineCode',
]);

/**
 * True when `pos` is inside a fenced/indented/inline code region, per the
 * markdown syntax tree. Resolves the node just before the cursor (side -1) and
 * walks ancestors. Returns false when no markdown tree is present (e.g. a
 * non-markdown language) — the source() caller only runs for markdown anyway.
 *
 * Exported for unit testing against a real markdown syntax tree.
 */
export function isInCode(state: EditorState, pos: number): boolean {
  let node: ReturnType<ReturnType<typeof syntaxTree>['resolveInner']> | null =
    syntaxTree(state).resolveInner(pos, -1);
  while (node) {
    if (CODE_NODE_NAMES.has(node.name)) return true;
    node = node.parent;
  }
  return false;
}

/**
 * CM6 input handler that auto-closes `]]` when the user types the second `[`
 * of a `[[` / `![[`. Exported for tests. Scoped to wikilinks (not global
 * `closeBrackets()`) per moss's minimal-surface design. `from`/`to` is the
 * typed-char range; for a `[` keypress with no selection they're equal.
 */
export function wikilinkBracketCloseHandler(): Extension {
  return EditorView.inputHandler.of((view, from, to, text) => {
    if (text !== '[') return false;
    const line = view.state.doc.lineAt(from);
    const beforeCursor = line.text.slice(0, from - line.from);
    const afterCursor = view.state.doc.sliceString(to, line.to);
    const close = wikilinkBracketClose(beforeCursor, afterCursor);
    if (!close) return false;
    view.dispatch({
      changes: { from, to, insert: close.insert },
      selection: { anchor: from + close.caretOffset },
      userEvent: 'input.type',
    });
    return true;
  });
}

/**
 * Wrap the completion source so it stays silent inside code: typing `[[` or
 * `![](` in a fenced block must not pop the menu, and a moss doc that
 * DOCUMENTS moss markdown once had its fenced examples silently rewritten on
 * accept. The guard lives at the composition point, with one owner.
 */
function silentInCode(
  source: (ctx: CompletionContext) => CompletionResult | Promise<CompletionResult | null> | null,
): (ctx: CompletionContext) => CompletionResult | Promise<CompletionResult | null> | null {
  return (ctx) => (isInCode(ctx.state, ctx.pos) ? null : source(ctx));
}

/**
 * Build the full CM6 completion extension: autocomplete dropdown (Rust-backed
 * targets) + bracket auto-close on typing.
 */
export function wikilinkCompleteExtension(
  opts: WikilinkCompleteOptions,
): Extension {
  return [
    autocompletion({ override: [silentInCode(completionSource(opts))], tooltipClass: () => 'moss-menu', optionClass: () => 'moss-menu-row' }),
    wikilinkBracketCloseHandler(),
  ];
}
