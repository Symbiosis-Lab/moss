/** What the caret is on, read once for every instrument: the hero it is in (if any), whether it is on the hero's opening line, and the picture on the caret's line. */

import { StateField, type EditorState } from '@codemirror/state';
import { collectShortcodeBlocks, flattenBlocks } from '@symbiosis-lab/moss-syntax/cm6';
import {
  embedPosition, embedWithPosition, imageAttrOf, imageAttrPosition, imageAttrWithPosition, type Point,
} from './image-position';

const EMBED = /!\[\[[^\]]*\]\]|!\[[^\]]*\]\([^)]*\)/;

/** A picture an instrument can re-crop. */
export interface HeroPicture {
  line: number;
  /** The asset reference the editor resolves. */
  target: string;
  position: Point | null;
  /** The edit that sets the picture's position to `word`. */
  edit(word: string): { from: number; to: number; insert: string };
}

export interface PictureAtCaret {
  /** The line the enclosing hero's fence opens on, or null outside a hero. */
  heroLine: number | null;
  /** The line the preview names the picture's frame by: the hero's fence line inside a hero (its `data-source-range` start), else the picture's own line. */
  frameLine: number;
  opener: boolean;
  picture: HeroPicture | null;
}

function pictureAtCaret(state: EditorState): PictureAtCaret | null {
  const line = state.doc.lineAt(state.selection.main.head);
  const hero = flattenBlocks(collectShortcodeBlocks(state))
    .filter((b) => b.name === 'hero' && b.from <= line.from && line.to <= b.to)
    .at(-1);
  const heroLine = hero ? state.doc.lineAt(hero.from).number : null;
  const opener = heroLine === line.number;
  let picture: HeroPicture | null = null;
  if (opener) {
    const attr = imageAttrOf(line.text);
    if (attr) {
      picture = {
        line: line.number, target: attr.value.split('|')[0].trim(), position: imageAttrPosition(attr),
        edit: (word) => ({ from: line.from + attr.from, to: line.from + attr.to, insert: imageAttrWithPosition(attr, word) }),
      };
    }
  } else {
    const embed = EMBED.exec(line.text);
    if (embed) {
      const md = embed[0];
      const at = line.from + embed.index;
      picture = {
        line: line.number, position: embedPosition(md),
        target: (md.startsWith('![[') ? md.slice(3, -2) : md.slice(md.lastIndexOf('](') + 2, -1)).split('|')[0].trim(),
        edit: (word) => ({ from: at, to: at + md.length, insert: embedWithPosition(md, word) }),
      };
    }
  }
  if (!opener && !picture) return null;
  return { heroLine, frameLine: heroLine ?? line.number, opener, picture };
}

export const pictureAtCaretField = StateField.define<PictureAtCaret | null>({
  create: pictureAtCaret,
  update: (value, tr) => (tr.docChanged || tr.selection ? pictureAtCaret(tr.state) : value),
});
