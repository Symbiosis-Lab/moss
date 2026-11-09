// Pure inline-format primitives shared by cm-format-keys (⌘B/⌘I/…) and the
// insert bar's format page (cm-insert-bar.ts).

import type { Text } from '@codemirror/state';
export type FormatAction = 'bold' | 'italic' | 'strike' | 'code' | 'link';

/** Pure function: wrap selected text with markdown formatting markers. */
export function wrapSelection(selected: string, action: FormatAction): string {
  switch (action) {
    case 'bold': return `**${selected}**`;
    case 'italic': return `*${selected}*`;
    case 'strike': return `~~${selected}~~`;
    case 'code': return `\`${selected}\``;
    case 'link': return `[${selected}]()`;
  }
}

export interface ToggleResult {
  isToggle: boolean;
  /** Position of first character to remove (the opening marker). */
  removeFrom: number;
  /** Position after the last character to remove (the closing marker). */
  removeTo: number;
}

/**
 * Pure function: detect whether the selection at [from, to] in doc is already
 * wrapped with the marker for action, i.e. the characters immediately outside
 * the selection match the expected open/close markers.
 *
 * Returns { isToggle: true, removeFrom, removeTo } when toggle-off should fire,
 * where [removeFrom, removeTo) is the full span to delete (markers + inner text).
 * Returns { isToggle: false } otherwise — caller should wrap instead.
 */
export function detectToggle(doc: Text, from: number, to: number, action: FormatAction): ToggleResult {
  const NO_TOGGLE: ToggleResult = { isToggle: false, removeFrom: from, removeTo: to };

  if (action === 'link') {
    // Link has asymmetric markers: [ before, ](url) after.
    if (from === 0) return NO_TOGGLE;
    const before = doc.sliceString(from - 1, from);
    if (before !== '[') return NO_TOGGLE;
    // After `to` we expect ]( ... )
    const afterStart = doc.sliceString(to, to + 2);
    if (afterStart !== '](') return NO_TOGGLE;
    // Scan forward for closing ), tracking paren depth to handle URLs with parentheses.
    const docLen = doc.length;
    let closePos = -1;
    let depth = 0;
    for (let i = to + 2; i < docLen; i++) {
      const ch = doc.sliceString(i, i + 1);
      if (ch === '(') { depth++; }
      else if (ch === ')') {
        if (depth === 0) { closePos = i + 1; break; }
        depth--;
      }
    }
    if (closePos === -1) return NO_TOGGLE;
    return { isToggle: true, removeFrom: from - 1, removeTo: closePos };
  }

  // Symmetric markers: same string before and after.
  const markerMap: Record<Exclude<FormatAction, 'link'>, string> = {
    bold: '**',
    italic: '*',
    strike: '~~',
    code: '`',
  };
  const marker = markerMap[action as Exclude<FormatAction, 'link'>];
  const mLen = marker.length;

  if (from < mLen) return NO_TOGGLE;  // not enough room before from
  const before = doc.sliceString(from - mLen, from);
  const after = doc.sliceString(to, to + mLen);
  if (before !== marker || after !== marker) return NO_TOGGLE;

  // Prevent italic toggle-off from false-firing inside **bold** markers.
  // If the chars immediately outside the `*` are also `*`, this is a bold
  // context, not italic — bail out.
  if (marker === '*') {
    const outerBefore = from - mLen - 1 >= 0 ? doc.sliceString(from - mLen - 1, from - mLen) : '';
    const outerAfter = doc.sliceString(to + mLen, to + mLen + 1);
    if (outerBefore === '*' || outerAfter === '*') return NO_TOGGLE;
  }

  return { isToggle: true, removeFrom: from - mLen, removeTo: to + mLen };
}
