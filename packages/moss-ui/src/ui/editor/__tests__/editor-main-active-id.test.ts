// Asserts the registry contract that editor-main's currentEntryId()
// helper depends on. We do not import editor-main directly (it has
// module-init side effects); the test exercises the registry's
// resolution path that editor-main's refactor relies on.
//
// Paths are project-root-relative.

import { describe, test, expect } from 'vitest';
import { EntryRegistry } from '../entry-registry';

describe('editor-main currentEntryId resolves via registry', () => {
  test('starts null, follows setActive, preserves across rename, clears on delete', () => {
    const registry = new EntryRegistry();
    expect(registry.activeEntryId).toBeNull();

    const id = registry.upsert('index.md', false);
    registry.setActive(id);
    expect(registry.activeEntryId).toBe(id);
    expect(registry.getById(registry.activeEntryId!)?.path).toBe('index.md');

    // Rename preserves active id across path change
    registry.rename('index.md', 'home.md');
    expect(registry.activeEntryId).toBe(id);
    expect(registry.getById(registry.activeEntryId!)?.path).toBe('home.md');

    // Delete clears active
    registry.delete('home.md');
    expect(registry.activeEntryId).toBeNull();
  });
});
