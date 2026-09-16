// Moss syntax highlight style — shared by cm-editor.ts and tests.
// Covers markdown-centric tags (link/heading/emphasis) and the standard
// Lezer code tags used by CSS / JS / JSON / TOML grammars. Colors come from
// the shared `--moss-hl-*` tokens already used by shortcode highlights, so
// code rendering matches the rest of the editor palette in light & dark.

import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { tags } from '@lezer/highlight';

export const mossHighlight = HighlightStyle.define([
  // Markdown
  { tag: tags.link, color: 'var(--moss-hl-link)' },
  { tag: tags.url, color: 'var(--moss-hl-link)' },
  { tag: tags.processingInstruction, color: 'var(--moss-hl-punct)' },
  { tag: tags.squareBracket, color: 'var(--moss-hl-punct)' },
  { tag: tags.heading, fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: '600' },
  // Code — CSS / JS / JSON / TOML
  { tag: tags.keyword, color: 'var(--moss-hl-keyword)' },
  { tag: [tags.string, tags.special(tags.string)], color: 'var(--moss-hl-string)' },
  { tag: tags.comment, color: 'var(--moss-hl-comment)', fontStyle: 'italic' },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--moss-hl-string)' },
  { tag: [tags.propertyName, tags.attributeName], color: 'var(--moss-hl-attr)' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: 'var(--moss-hl-keyword)' },
  { tag: tags.operator, color: 'var(--moss-hl-operator)' },
  { tag: tags.tagName, color: 'var(--moss-hl-keyword)' },
  { tag: tags.typeName, color: 'var(--moss-hl-keyword)' },
]);

export function mossHighlightExtension(): Extension {
  return syntaxHighlighting(mossHighlight);
}
