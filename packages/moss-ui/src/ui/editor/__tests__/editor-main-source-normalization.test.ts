// Direct test of the wiring in editor-main's file-changed handler: a
// relative-path source_renames event routes through the registry's
// relative byPath keys. Registry keys are project-root-relative throughout.
//
// We test the contract (helper + registry composition), not the full
// editor-main listener — mounting the panel for a unit test is
// disproportionate when the wiring is a four-line closure.
// Integration tests in file-tree-rename.test.ts cover the full flow.

import { describe, test, expect } from 'vitest';
import { EntryRegistry } from '../entry-registry';

describe('editor-main source-domain normalization (wire contract)', () => {
  test('relative source_renames pair updates the relative-keyed registry entry', () => {
    const registry = new EntryRegistry();
    // Simulates what file-tree.flattenTree puts in the registry at mount
    // (project-root-relative)
    const id = registry.upsert('index.md', false);

    // What the watcher emits over the wire (relative, per path_to_relative_key)
    const wirePair: [string, string] = ['index.md', 'Mira Vale.md'];

    const returned = registry.rename(wirePair[0], wirePair[1]);

    // The registry preserves the EntryId across the rename
    expect(returned).toBe(id);
    // byPath now keys the new relative path
    expect(registry.getByPath('Mira Vale.md')).toBe(id);
    // The old relative path is gone
    expect(registry.getByPath('index.md')).toBeNull();
  });

  test('relative source_creates path lands in registry under relative key', () => {
    const registry = new EntryRegistry();

    const wireCreate = 'new-post.md';
    const id = registry.upsert(wireCreate, false);

    expect(registry.getByPath('new-post.md')).toBe(id);
  });

  test('relative source_deletes path removes the relative registry entry', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('old-post.md', false);

    const wireDelete = 'old-post.md';
    registry.delete(wireDelete);

    expect(registry.getByPath('old-post.md')).toBeNull();
    expect(registry.getById(id)).toBeNull();
  });

  test('nested-path rename routes through correctly', () => {
    // Important for the moveFile case where the rename crosses directories.
    const registry = new EntryRegistry();
    const id = registry.upsert('index.md', false);

    const wirePair: [string, string] = ['index.md', 'archive/index.md'];
    registry.rename(wirePair[0], wirePair[1]);

    expect(registry.getById(id)?.path).toBe('archive/index.md');
  });
});
