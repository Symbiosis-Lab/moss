// cm-json.test.ts — unit tests for the JSON item deletion range computation.
//
// Tests cover: single-line arrays, multiline arrays, objects, nested structures,
// first / middle / last / single-element cases, and comma handling.
//
// The pure `jsonItemDeleteRange(state, pos)` function is tested directly — no
// EditorView required — keeping tests fast and deterministic.
//
// Scope note: the delete command is context-aware at the NEAREST enclosing
// Array/Object level. For nested structures, the cursor position determines
// which container is targeted:
//   - Cursor inside a property value → edits that property's container (Object).
//   - Cursor inside a nested array element → edits that nested array.
// This is the correct lezer-tree-based CM6 structural editing convention.

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { json } from '@codemirror/lang-json';
import { jsonItemDeleteRange } from '../cm-json';

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Create an EditorState with the JSON language extension, apply
 * `jsonItemDeleteRange` at `pos`, and return the resulting document string
 * or null if deletion is not applicable at that position.
 */
function applyDelete(doc: string, pos: number): string | null {
  const state = EditorState.create({ doc, extensions: [json()] });
  const range = jsonItemDeleteRange(state, pos);
  if (!range) return null;
  return doc.slice(0, range.from) + doc.slice(range.to);
}

/** Return the raw deletion range (for asserting exact from/to positions). */
function deleteRange(doc: string, pos: number): { from: number; to: number } | null {
  const state = EditorState.create({ doc, extensions: [json()] });
  return jsonItemDeleteRange(state, pos);
}

// ── Single-line array ──────────────────────────────────────────────────────────

describe('single-line array', () => {
  // ["a", "b", "c"]
  // String "a" at [1..4], , [4..5], String "b" at [6..9], , [9..10], String "c" at [11..14]
  const doc = '["a", "b", "c"]';

  test('first element: removes item and trailing comma+space', () => {
    // pos=2 inside "a"
    const result = applyDelete(doc, 2);
    expect(result).toBe('["b", "c"]');
  });

  test('middle element: removes item and trailing comma+space', () => {
    // pos=7 inside "b"
    const result = applyDelete(doc, 7);
    expect(result).toBe('["a", "c"]');
  });

  test('last element: removes preceding comma+space and item', () => {
    // pos=12 inside "c"
    const result = applyDelete(doc, 12);
    expect(result).toBe('["a", "b"]');
  });
});

// ── Single-element arrays ─────────────────────────────────────────────────────

describe('single element arrays', () => {
  test('single string element removed, leaving []', () => {
    // ["only"]: String "only" at [1..7] — only child → delete [1..7] → []
    const doc = '["only"]';
    const result = applyDelete(doc, 2);
    expect(result).toBe('[]');
  });

  test('single number element removed, leaving []', () => {
    const doc = '[42]';
    const result = applyDelete(doc, 1);
    expect(result).toBe('[]');
  });

  test('single array element is itself a nested array: removes the inner value at cursor', () => {
    // [{"x":1}]: cursor at pos=2 (inside PropertyName "x").
    // Nearest Array/Object parent = the inner Object.
    // Property "x":1 is the only property → delete it → [{}].
    const doc = '[{"x":1}]';
    const result = applyDelete(doc, 2);
    expect(result).toBe('[{}]');
  });
});

// ── Multiline array ───────────────────────────────────────────────────────────

describe('multiline array', () => {
  // [\n  "a",\n  "b",\n  "c"\n]
  // [ [0..1], \n [1], "  " [2..4], String "a" [4..7], , [7..8],
  //   \n [8], "  " [9..11], String "b" [11..14], , [14..15],
  //   \n [15], "  " [16..18], String "c" [18..21], \n [21], ] [22]
  const doc = '[\n  "a",\n  "b",\n  "c"\n]';

  test('first element: removes item + comma + leading whitespace of next', () => {
    // "a" at [4..7]; comma [7..8]; next sibling "b" starts at 11.
    // delete [4..11] → [\n  "b",\n  "c"\n]
    const result = applyDelete(doc, 5);
    expect(result).toBe('[\n  "b",\n  "c"\n]');
  });

  test('middle element: removes item + comma + leading whitespace of next', () => {
    // "b" at [11..14]; comma [14..15]; next sibling "c" starts at 18.
    // delete [11..18] → [\n  "a",\n  "c"\n]
    const result = applyDelete(doc, 12);
    expect(result).toBe('[\n  "a",\n  "c"\n]');
  });

  test('last element: removes preceding comma+whitespace and item; closing bracket stays on its own line', () => {
    // "c" at [18..21]; preceding comma at [14..15].
    // delete [14..21] = ,\n  "c" → [\n  "a",\n  "b"\n]
    const result = applyDelete(doc, 19);
    expect(result).toBe('[\n  "a",\n  "b"\n]');
  });
});

// ── Two-element multiline array ───────────────────────────────────────────────

describe('two-element multiline array', () => {
  const doc = '[\n  "first",\n  "second"\n]';

  test('delete first element', () => {
    // "first" at [4..11]; comma [11..12]; "second" starts at 15.
    // delete [4..15] → [\n  "second"\n]
    const result = applyDelete(doc, 6);
    expect(result).toBe('[\n  "second"\n]');
  });

  test('delete last element; closing bracket stays on its own line', () => {
    // "second" at [15..23]; preceding comma at [11..12].
    // delete [11..23] = ,\n  "second" → [\n  "first"\n]
    const result = applyDelete(doc, 16);
    expect(result).toBe('[\n  "first"\n]');
  });
});

// ── Single-line object ────────────────────────────────────────────────────────

describe('single-line object', () => {
  // {"a": 1, "b": 2, "c": 3}
  // Property "a":1 [1..7], , [7..8], Property "b":2 [9..15], , [15..16], Property "c":3 [17..23]
  const doc = '{"a": 1, "b": 2, "c": 3}';

  test('first property: removes property and trailing comma+space', () => {
    // Property "a":1 [1..7]; next sibling "b":2 starts at 9. delete [1..9].
    const result = applyDelete(doc, 2);
    expect(result).toBe('{"b": 2, "c": 3}');
  });

  test('middle property: removes property and trailing comma+space', () => {
    // Property "b":2 [9..15]; next sibling "c":3 starts at 17. delete [9..17].
    const result = applyDelete(doc, 10);
    expect(result).toBe('{"a": 1, "c": 3}');
  });

  test('last property: removes preceding comma+space and property', () => {
    // Property "c":3 [17..23]; preceding comma at [15..16]. delete [15..23].
    const result = applyDelete(doc, 18);
    expect(result).toBe('{"a": 1, "b": 2}');
  });

  test('single property: removes property only, leaving {}', () => {
    // {"key": "val"}: Property [1..13] — only child → delete [1..13] → {}
    const single = '{"key": "val"}';
    const result = applyDelete(single, 2);
    expect(result).toBe('{}');
  });
});

// ── Multiline object ──────────────────────────────────────────────────────────

describe('multiline object', () => {
  // {\n  "a": 1,\n  "b": 2,\n  "c": 3\n}
  const doc = '{\n  "a": 1,\n  "b": 2,\n  "c": 3\n}';

  test('first property deleted', () => {
    const result = applyDelete(doc, 5);
    expect(result).toBe('{\n  "b": 2,\n  "c": 3\n}');
  });

  test('middle property deleted', () => {
    const result = applyDelete(doc, 15);
    expect(result).toBe('{\n  "a": 1,\n  "c": 3\n}');
  });

  test('last property deleted; closing brace stays on its own line', () => {
    // "c": 3 is the last property. preceding comma exists.
    // delete [commaFrom..itemTo] → {\n  "a": 1,\n  "b": 2\n}
    const result = applyDelete(doc, 25);
    expect(result).toBe('{\n  "a": 1,\n  "b": 2\n}');
  });
});

// ── Cursor on value vs key of property ───────────────────────────────────────

describe('cursor position variants within a property', () => {
  const doc = '{"name": "Alice", "age": 30}';

  test('cursor on key removes the whole property', () => {
    // "name" is the PropertyName; Property "name":"Alice" [1..16]
    const result = applyDelete(doc, 2);
    expect(result).toBe('{"age": 30}');
  });

  test('cursor on value removes the whole property', () => {
    // "Alice" value; pos inside it e.g. pos=10
    const result = applyDelete(doc, 10);
    expect(result).toBe('{"age": 30}');
  });

  test('cursor on last property key removes it', () => {
    // "age" PropertyName; Property "age":30 [18..26]
    const result = applyDelete(doc, 19);
    expect(result).toBe('{"name": "Alice"}');
  });
});

// ── Nested structures ─────────────────────────────────────────────────────────

describe('nested structures', () => {
  test('delete element from nested array: cursor targets nearest container', () => {
    // [[1, 2, 3], [4, 5]] — cursor inside the '2' of the first inner array.
    // Nearest Array parent = inner [1,2,3]; deletes '2' (middle element).
    const doc = '[[1, 2, 3], [4, 5]]';
    const result = applyDelete(doc, 5);
    expect(result).toBe('[[1, 3], [4, 5]]');
  });

  test('delete first element of inner array (nearest container)', () => {
    // [[1, 2], [3, 4]] — cursor at pos=2 (inside Number 1, parent = inner Array [1..7]).
    // Deletes '1' from [1,2], leaving [[2], [3, 4]].
    const doc = '[[1, 2], [3, 4]]';
    const result = applyDelete(doc, 2);
    expect(result).toBe('[[2], [3, 4]]');
  });

  test('delete property whose value is an object', () => {
    // {"meta": {"key": "val"}, "name": "x"} — cursor on "meta" PropertyName.
    const doc = '{"meta": {"key": "val"}, "name": "x"}';
    const result = applyDelete(doc, 2);
    expect(result).toBe('{"name": "x"}');
  });

});

// ── Null / bool / number values in arrays ────────────────────────────────────

describe('non-string JSON value types', () => {
  test('delete null from array (first element)', () => {
    const doc = '[null, "b"]';
    const result = applyDelete(doc, 1);
    expect(result).toBe('["b"]');
  });

  test('delete true from array (first element)', () => {
    const doc = '[true, false]';
    const result = applyDelete(doc, 1);
    expect(result).toBe('[false]');
  });

  test('delete false from array (last element)', () => {
    const doc = '[true, false]';
    const result = applyDelete(doc, 8);
    expect(result).toBe('[true]');
  });

  test('delete number property from object', () => {
    const doc = '{"x": 1, "y": 2}';
    const result = applyDelete(doc, 2);
    expect(result).toBe('{"y": 2}');
  });
});

// ── Non-applicable cursor positions ──────────────────────────────────────────

describe('non-applicable cursor positions', () => {
  test('returns null when cursor is on opening bracket (no content sibling)', () => {
    // pos=0 is on the outer '['; climbing from '[' → Array → JsonText (no Array/Object parent).
    const doc = '["a"]';
    expect(deleteRange(doc, 0)).toBeNull();
  });

  test('returns null for empty array', () => {
    const doc = '[]';
    expect(deleteRange(doc, 1)).toBeNull();
  });

  test('returns null for empty object', () => {
    const doc = '{}';
    expect(deleteRange(doc, 1)).toBeNull();
  });
});

// ── Exact deletion range offsets ─────────────────────────────────────────────

describe('exact deletion range offsets', () => {
  test('single-line array first element: from=itemFrom, to=nextSibling.from', () => {
    // ["a", "b"]  String "a" [1..4], , [4..5], String "b" [6..9]
    // delete "a": from=1, to=6 (nextSibling.from=6)
    const doc = '["a", "b"]';
    const r = deleteRange(doc, 2);
    expect(r).toEqual({ from: 1, to: 6 });
  });

  test('single-line array last element: from=commaFrom, to=itemTo', () => {
    // ["a", "b"]  , [4..5], String "b" [6..9]
    // delete "b": from=4 (comma), to=9 (itemTo — no newline eaten)
    const doc = '["a", "b"]';
    const r = deleteRange(doc, 7);
    expect(r).toEqual({ from: 4, to: 9 });
  });

  test('multiline array last element: to=itemTo (trailing newline stays)', () => {
    // ["a",\n"b"\n]  String "a" [1..4], , [4..5], String "b" [6..9], \n [9], ] [10..11]
    // delete "b": from=4 (comma), to=9 (itemTo, NOT 10 — \n before ] stays)
    const doc = '["a",\n"b"\n]';
    const r = deleteRange(doc, 7);
    expect(r).toEqual({ from: 4, to: 9 });
  });

  test('single-element array: from=itemFrom, to=itemTo (no comma)', () => {
    // [42]  Number [1..3]
    const doc = '[42]';
    const r = deleteRange(doc, 1);
    expect(r).toEqual({ from: 1, to: 3 });
  });
});
