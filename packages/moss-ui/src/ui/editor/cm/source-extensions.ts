import { markdown } from '@codemirror/lang-markdown';
import { Strikethrough, Table, TaskList } from '@lezer/markdown';
import type { Extension } from '@codemirror/state';
import {
  footnoteConfig,
  mathConfig,
  shortcodeBlockConfig,
  wikilinkConfig,
} from '@symbiosis-lab/moss-syntax';
import {
  criticmarkupExtension,
  mossHighlightExtension,
} from '@symbiosis-lab/moss-syntax/cm6';
import { calloutExtension } from '../cm-callout';
import { commentExtension, obsidianCommentKeymap, percentCommentExtension } from '../cm-comment';
import { formatKeysExtension, lineIndicatorExtension } from '../cm-format-keys';
import { markdownStyleExtension } from '../cm-markdown-style';
import { slashMenuExtension } from '../cm-slash-menu';
import { footnoteTheme } from '@symbiosis-lab/moss-syntax/cm6';

/**
 * Match the GFM features the build pipeline renders so editors don't diverge
 * from published sites. Parity matrix vs the build's pulldown-cmark options
 * (`parser_options()` in crates/moss-core/src/ast/parser.rs):
 * - `ENABLE_STRIKETHROUGH` -> Strikethrough; `ENABLE_TABLES` -> Table;
 *   `ENABLE_TASKLISTS` -> TaskList.
 * - `ENABLE_FOOTNOTES` -> footnoteConfig. @lezer/markdown has no built-in footnote
 *   grammar; without this extension `[^1]` was parsed as a fake link and a
 *   definition as an opaque link reference. Rendering only recognizes defined
 *   labels, matching the build.
 * - `ENABLE_WIKILINKS` is parsed by wikilinkConfig and resolved by host extensions.
 * - `ENABLE_CJK_FRIENDLY_EMPHASIS` needs no extension: plain CommonMark flanking pairs
 *   `**粗體**字` and `「*斜體*」` like pulldown-cmark with its flag on. The parity
 *   probe catches either parser changing.
 * - `ENABLE_DEFINITION_LIST` stays source text in the editor while the build renders
 *   `<dl>`; the parity probe ensures @lezer/markdown does not claim them.
 * - `ENABLE_MATH` is mathConfig, which marks `$…$` and `$$…$$` source spans for tinting only. The editor
 *   does not render glyphs; the preview shows the typeset output.
 *   `packages/moss-syntax` gates delimiters
 *   against the same vectors as pulldown-cmark.
 *
 * This is deliberately not the full GFM bundle: that also enables Autolink,
 * which the build does not render. `markdown-parity.test.ts` gates this list
 * against the build's parser options.
 */
export const MARKDOWN_EXTENSIONS = [
  Strikethrough, Table, TaskList, shortcodeBlockConfig, wikilinkConfig, mathConfig, footnoteConfig,
];

export interface SourceMarkdownOptions {
  comments?: boolean;
}

/**
 * The source-facing part of moss's markdown editor. Hosts provide their own
 * persistence, navigation, reference resolution, and other platform behavior.
 */
export function sourceMarkdownExtensions(options: SourceMarkdownOptions = {}): Extension[] {
  return [
    markdown({ extensions: MARKDOWN_EXTENSIONS }),
    mossHighlightExtension(),
    formatKeysExtension(),
    ...calloutExtension(),
    slashMenuExtension(),
    footnoteTheme,
    markdownStyleExtension(),
    lineIndicatorExtension(),
    criticmarkupExtension(),
    percentCommentExtension(),
    obsidianCommentKeymap(),
    ...(options.comments === false ? [] : [commentExtension()]),
  ];
}
