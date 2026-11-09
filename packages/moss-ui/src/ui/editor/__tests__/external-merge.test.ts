import { describe, expect, test } from 'vitest';
import { Text } from '@codemirror/state';
import { changesBetween, isClean, lineDiff, mergeExternal, outsideWins, type DocumentText } from '../external-merge';

const doc = (body: string, frontmatter: Record<string, unknown> = {}): DocumentText => ({ body, frontmatter });
const apply = (merge: ReturnType<typeof mergeExternal>, buffer: string) => merge.changes.apply(Text.of(buffer.split('\n'))).toString();

const BASE = 'Title\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n';

describe('mergeExternal', () => {
  test('an untouched disk changes nothing', () => {
    const buffer = BASE.replace('First paragraph.', 'First paragraph, typed.');
    const merge = mergeExternal(doc(BASE), doc(BASE), doc(buffer));
    expect(isClean(merge) && merge.changes.empty).toBe(true);
  });

  test('an untouched buffer takes exactly the disk text', () => {
    const disk = 'Title\n\nRewritten.\n\nSecond paragraph.\nAdded line.\n';
    expect(apply(mergeExternal(doc(BASE), doc(disk), doc(BASE)), BASE)).toBe(disk);
  });

  test('edits to different paragraphs both survive, and only the disk edit is applied', () => {
    const buffer = BASE.replace('First paragraph.', 'First paragraph, typed by the person.');
    const disk = BASE.replace('Third paragraph.', 'Third paragraph, revised by an agent.');
    const merge = mergeExternal(doc(BASE), doc(disk), doc(buffer));
    expect(apply(merge, buffer)).toBe(BASE
      .replace('First paragraph.', 'First paragraph, typed by the person.')
      .replace('Third paragraph.', 'Third paragraph, revised by an agent.'));
    // The one change lies in the third paragraph of the BUFFER: past the person's longer first line.
    let touched: [number, number] | null = null;
    merge.changes.iterChangedRanges((fromA, toA) => { touched = [fromA, toA]; });
    const at = buffer.indexOf('Third paragraph.');
    expect(touched![0]).toBeGreaterThanOrEqual(at);
  });

  test('an agent edit at the top and another at the bottom merge around the person in the middle', () => {
    const buffer = BASE.replace('Second paragraph.', 'Second paragraph, typed.');
    const disk = BASE.replace('Title', 'New title').replace('Third paragraph.', 'Third, by an agent.');
    expect(apply(mergeExternal(doc(BASE), doc(disk), doc(buffer)), buffer))
      .toBe('New title\n\nFirst paragraph.\n\nSecond paragraph, typed.\n\nThird, by an agent.\n');
  });

  test("two edits to the same line keep the person's line and set the disk's aside", () => {
    const buffer = BASE.replace('Second paragraph.', 'Second paragraph, typed.');
    const disk = BASE.replace('Second paragraph.', 'Second paragraph, by an agent.');
    const merge = mergeExternal(doc(BASE), doc(disk), doc(buffer));
    expect(merge.overlaps).toHaveLength(1);
    expect(apply(merge, buffer)).toBe(buffer);
  });

  test("an overlap sets aside only the overlapping disk change; the rest of the disk's still merges", () => {
    const buffer = BASE.replace('Second paragraph.', 'Second paragraph, typed.');
    const disk = BASE.replace('Second paragraph.', 'Second, by an agent.').replace('Third paragraph.', 'Third, by an agent.');
    const merge = mergeExternal(doc(BASE), doc(disk), doc(buffer));
    expect(apply(merge, buffer)).toBe(buffer.replace('Third paragraph.', 'Third, by an agent.'));
  });

  test('edits to adjacent lines overlap, as in a three-way text merge', () => {
    const base = 'one\ntwo\nthree\n';
    expect(isClean(mergeExternal(doc(base), doc('one\nTWO\nthree\n'), doc('one\ntwo\nTHREE\n')))).toBe(false);
  });

  test('an insertion where the person also inserted overlaps: their order is unknown', () => {
    const base = 'one\nthree\n';
    expect(isClean(mergeExternal(doc(base), doc('one\ntheirs\nthree\n'), doc('one\nmine\nthree\n')))).toBe(false);
  });

  test('only the characters that differ inside a changed line are replaced', () => {
    const base = 'alpha\nbravo\ncharlie';
    const merge = mergeExternal(doc(base), doc('alpha\nBRAVO!\ncharlie'), doc(base));
    const ranges: [number, number][] = [];
    merge.changes.iterChangedRanges((fromA, toA) => ranges.push([fromA, toA]));
    expect(ranges).toEqual([[6, 11]]);
  });

  test('the same edit made by both is not a conflict and is not applied twice', () => {
    const edited = BASE.replace('Second paragraph.', 'Second paragraph, agreed.');
    expect(apply(mergeExternal(doc(BASE), doc(edited), doc(edited)), edited)).toBe(edited);
  });

  describe('frontmatter, by key', () => {
    const merge = (base: Record<string, unknown>, disk: Record<string, unknown>, buffer: Record<string, unknown>) =>
      mergeExternal(doc('x\n', base), doc('x\n', disk), doc('x\n', buffer));

    test('a key changed on one side takes that side, and a key the build stamped is added', () => {
      const m = merge({ title: 'A', tags: ['x'] }, { title: 'A', tags: ['x', 'y'], uid: '1a2b' }, { title: 'B', tags: ['x'] });
      expect(m).toMatchObject({ keys: [], frontmatter: { title: 'B', tags: ['x', 'y'], uid: '1a2b' } });
    });

    test('a key removed on disk is removed, unless the person changed it', () => {
      expect(merge({ a: 1, b: 2 }, { a: 1 }, { a: 1, b: 2 })).toMatchObject({ keys: [], frontmatter: { a: 1 } });
      expect(merge({ a: 1, b: 2 }, { a: 1 }, { a: 1, b: 3 })).toMatchObject({ keys: ['b'], frontmatter: { a: 1, b: 3 } });
    });

    test("one key changed on both sides to different values keeps the person's value", () => {
      expect(merge({ title: 'A' }, { title: 'Disk' }, { title: 'Mine' })).toMatchObject({ keys: ['title'], frontmatter: { title: 'Mine' } });
    });
  });
});

describe('outsideWins', () => {
  test("puts back the other writer's lines where they overlap and keeps the person's other edits", () => {
    const buffer = BASE.replace('First paragraph.', 'First, mine.').replace('Second paragraph.', 'Second, mine.');
    const disk = BASE.replace('Second paragraph.', 'Second, theirs.');
    expect(outsideWins(doc(BASE), doc(disk, { title: 'Theirs' }), doc(buffer, { title: 'Mine' })))
      .toEqual(doc(BASE.replace('First paragraph.', 'First, mine.').replace('Second paragraph.', 'Second, theirs.'), { title: 'Theirs' }));
  });

  test('applied after the person kept typing, it changes only the lines that were set aside', () => {
    const saved = 'one\ntwo mine\nthree\n';
    const theirs = 'one\ntwo theirs\nthree\n';
    const now = 'one\ntwo mine\nthree\nfour, typed since\n';
    expect(outsideWins(doc(saved), doc(theirs), doc(now)).body).toBe('one\ntwo theirs\nthree\nfour, typed since\n');
  });
});

describe('changesBetween', () => {
  test('turns one text into the other', () => {
    expect(changesBetween('a\nb\nc\n', 'a\nB!\nc\nd\n').apply(Text.of('a\nb\nc\n'.split('\n'))).toString()).toBe('a\nB!\nc\nd\n');
  });
});

describe('lineDiff', () => {
  test('finds the unchanged lines between two edits', () => {
    expect(lineDiff('a\nb\nc\nd\n', 'A\nb\nc\nD\n')).toEqual([
      { from: 0, to: 2, insert: 'A\n' },
      { from: 6, to: 8, insert: 'D\n' },
    ]);
  });
});
