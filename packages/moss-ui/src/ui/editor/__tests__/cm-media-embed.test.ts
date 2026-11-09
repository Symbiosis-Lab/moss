/**
 * Tests for `folderCardDetail` (cm-media-embed.ts) — folding a folder embed's
 * build-time listing facts and source-derived params into what a card renders.
 *
 * `MediaEmbedBlockWidget` lives in `cm-block-embeds.ts`
 * Its tests live in `cm-block-embeds.test.ts`.
 */

import { describe, test, expect } from 'vitest';
import { folderCardDetail } from '../cm-media-embed';

describe('folderCardDetail', () => {
  const info = { title: 'Awards', direct_child_count: 12, descendant_count: 30 };

  test.each([
    ['all', 30],
    ['deep', 12],
    ['1', 12],
    ['All', 12],
    [undefined, 12],
  ] as const)('depth %j selects %i — an EXACT compare, as the build does', (depth, expected) => {
    expect(folderCardDetail(info, { depth })?.childCount).toBe(expected);
  });

  test.each([
    [3, 3],    // the site renders 3 — say 3, with a limit:3 chip beside it
    [20, 12],  // the build truncates only when n < len
    [0, 12],   // …and only when n > 0
  ])('limit %i on 12 shows %i', (limit, expected) => {
    expect(folderCardDetail(info, { limit })?.childCount).toBe(expected);
  });

  test('no folder facts → no count, but the chips still say how', () => {
    const d = folderCardDetail(null, { style: 'grid' });
    expect(d?.childCount).toBeNull();
    expect(d?.chips).toEqual(['style:grid']);
  });

  test('null params → undefined: a markdown-image folder embed lists nothing', () => {
    expect(folderCardDetail(info, null)).toBeUndefined();
  });
});
