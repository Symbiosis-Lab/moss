import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';
import {
  coverRect, embedPosition, embedWithPosition, imageAttrOf, imageAttrPosition, imageAttrWithPosition,
  pointOfPosition, positionOfSegments, positionWord, segmentsWithPosition,
} from '../image-position';

type Expected = string | { focus: [number, number] } | null;
interface Vectors {
  position_words: { input: string; position: Expected; note: string }[];
  frame_segments: { input: string; fit: string | null; position: Expected; align: string | null; note: string }[];
}

// moss's own answers (crates/moss-core/tests/position_vectors.rs checks the parser against this file).
const vectors = JSON.parse(readFileSync(
  (import.meta.dirname + "/../../../../../../moss-syntax/fixtures/position.vectors.json"), 'utf8',
)) as Vectors;

/** A name stands for a point on a 3x3 grid: its words say which edge each axis sits on. */
function pointOfName(name: string): { x: number; y: number } {
  const words = name.split('-');
  const x = words.includes('left') ? 0 : words.includes('right') ? 100 : 50;
  const y = words.includes('top') ? 0 : words.includes('bottom') ? 100 : 50;
  return { x, y };
}

function expectPoint(actual: ReturnType<typeof pointOfPosition>, expected: Expected) {
  if (expected === null) return expect(actual).toBeNull();
  const want = typeof expected === 'string' ? pointOfName(expected) : { x: expected.focus[0], y: expected.focus[1] };
  expect(actual).toEqual(want);
}

describe('the grammar agrees with moss position vectors', () => {
  test('the vectors loaded', () => {
    expect(vectors.position_words.length).toBeGreaterThan(30);
    expect(vectors.frame_segments.length).toBeGreaterThan(20);
  });

  test.each(vectors.position_words.map((v) => [v.input, v] as const))('position word %j', (_, v) => {
    expectPoint(pointOfPosition(v.input), v.position);
  });

  test.each(vectors.frame_segments.map((v) => [v.input, v] as const))('frame segment %j', (_, v) => {
    expectPoint(positionOfSegments([v.input]), v.position);
    // A segment moss reads anything from is the frame's options, so a new position joins it
    // rather than becoming a segment of its own (an unknown word is a caption, left alone).
    const isFrame = v.fit !== null || v.position !== null || v.align !== null || v.input.startsWith('color=');
    expect(segmentsWithPosition([v.input], 'center')).toHaveLength(isFrame ? 1 : 2);
  });
});

describe('positionWord', () => {
  test('within 4 points of a name on both axes takes the name', () => {
    expect(positionWord(3, 4)).toBe('top-left');
  });
  test('5 points off on one axis is a free point', () => {
    expect(positionWord(5, 0)).toBe('focus:5%,0%');
    expect(positionWord(50, 55)).toBe('focus:50%,55%');
  });
  test('rounds to whole percentages and stays in 0..100', () => {
    expect(positionWord(19.6, 62.4)).toBe('focus:20%,62%');
    expect(positionWord(-3, 130)).toBe('bottom-left');
  });
});

describe('coverRect', () => {
  test('a frame wider than the picture keeps the full width and slides vertically', () => {
    expect(coverRect(1, 2, 0.5, 0)).toEqual({ left: 0, top: 0, width: 1, height: 0.5 });
    expect(coverRect(1, 2, 0.5, 1)).toEqual({ left: 0, top: 0.5, width: 1, height: 0.5 });
  });
  test('a frame narrower than the picture keeps the full height and slides sideways', () => {
    expect(coverRect(2, 1, 0.5, 0.5)).toEqual({ left: 0.25, top: 0, width: 0.5, height: 1 });
    expect(coverRect(2, 1, 1, 0.5).left).toBe(0.5);
  });
  test('the same shape keeps everything', () => {
    expect(coverRect(1.5, 1.5, 0.3, 0.9)).toEqual({ left: 0, top: 0, width: 1, height: 1 });
  });
});

describe('segmentsWithPosition', () => {
  test('replaces the position and keeps the other keywords', () => {
    expect(segmentsWithPosition(['cover top'], 'focus:20%,62%')).toEqual(['cover focus:20%,62%']);
    expect(segmentsWithPosition(['cover bottom-left'], 'center')).toEqual(['cover center']);
  });
  test('removes both words of a two-word position', () => {
    expect(segmentsWithPosition(['top left cover'], 'bottom')).toEqual(['bottom cover']);
  });
  test('keeps a width segment and a caption segment untouched', () => {
    expect(segmentsWithPosition(['50%', 'cover top'], 'left')).toEqual(['50%', 'cover left']);
    expect(segmentsWithPosition(['Left hand, 1998', 'cover'], 'top')).toEqual(['Left hand, 1998', 'cover top']);
  });
  test('adds to the frame segment when there is no position, else a segment of its own', () => {
    expect(segmentsWithPosition(['cover'], 'top')).toEqual(['cover top']);
    expect(segmentsWithPosition(['50%'], 'top')).toEqual(['50%', 'top']);
    expect(segmentsWithPosition([], 'top')).toEqual(['top']);
  });
});

describe('embeds', () => {
  test('a wikilink embed with no pipe gains one', () => {
    expect(embedWithPosition('![[photo.jpg]]', 'focus:20%,62%')).toBe('![[photo.jpg|focus:20%,62%]]');
  });
  test('a wikilink embed keeps its other options', () => {
    expect(embedWithPosition('![[photo.jpg|cover top]]', 'bottom-right')).toBe('![[photo.jpg|cover bottom-right]]');
    expect(embedWithPosition('![[photo.jpg|focus:1%,2%|50%]]', 'center')).toBe('![[photo.jpg|center|50%]]');
  });
  test('a markdown image carries its options in the url', () => {
    expect(embedWithPosition('![a view](photo.jpg)', 'top')).toBe('![a view](photo.jpg|top)');
    expect(embedWithPosition('![a view](photo.jpg|cover top)', 'left')).toBe('![a view](photo.jpg|cover left)');
  });
  test('text that is not an embed is returned as it was', () => {
    expect(embedWithPosition('plain', 'top')).toBe('plain');
  });
  test('reads the position back', () => {
    expect(embedPosition('![[photo.jpg|cover top]]')).toEqual({ x: 50, y: 0 });
    expect(embedPosition('![[photo.jpg|focus:20%,62%]]')).toEqual({ x: 20, y: 62 });
    expect(embedPosition('![[photo.jpg|cover]]')).toBeNull();
    expect(embedPosition('![[photo.jpg]]')).toBeNull();
  });
});

describe('a hero opening line', () => {
  test('finds a bare and a quoted image attribute', () => {
    expect(imageAttrOf(':::hero {image=photo.jpg align=end}')?.value).toBe('photo.jpg');
    expect(imageAttrOf(':::hero {image="a b.jpg|cover top"}')?.value).toBe('a b.jpg|cover top');
  });
  test('a line with no image attribute, has none', () => {
    expect(imageAttrOf(':::hero {align=end}')).toBeNull();
    expect(imageAttrOf(':::hero {caption="the image=x"}')).toBeNull();
  });
  test('writing a position quotes the value, because a pipe is not a bareword', () => {
    const line = ':::hero {image=photo.jpg align=end}';
    const attr = imageAttrOf(line)!;
    const out = line.slice(0, attr.from) + imageAttrWithPosition(attr, 'focus:20%,62%') + line.slice(attr.to);
    expect(out).toBe(':::hero {image="photo.jpg|focus:20%,62%" align=end}');
  });
  test('replaces the position inside a quoted value', () => {
    const attr = imageAttrOf(':::hero {image="a b.jpg|cover top"}')!;
    expect(imageAttrPosition(attr)).toEqual({ x: 50, y: 0 });
    expect(imageAttrWithPosition(attr, 'bottom')).toBe('image="a b.jpg|cover bottom"');
  });
});
