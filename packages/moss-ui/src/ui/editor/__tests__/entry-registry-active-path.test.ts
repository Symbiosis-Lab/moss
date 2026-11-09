import { describe, test, expect, vi } from 'vitest';
import { EntryRegistry } from '../entry-registry';

describe('EntryRegistry.subscribeActivePath', () => {
  test('fires on setActive (ID change) with new and old paths', () => {
    const registry = new EntryRegistry();
    const idA = registry.upsert('a.md', false);
    const idB = registry.upsert('b.md', false);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.setActive(idA);
    expect(handler).toHaveBeenLastCalledWith('a.md', null);

    registry.setActive(idB);
    expect(handler).toHaveBeenLastCalledWith('b.md', 'a.md');
  });

  test('fires on rename of the active entry', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('old.md', false);
    registry.setActive(id);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.rename('old.md', 'new.md');
    expect(handler).toHaveBeenCalledWith('new.md', 'old.md');
  });

  test('does NOT fire on rename of a non-active entry', () => {
    const registry = new EntryRegistry();
    const idA = registry.upsert('a.md', false);
    registry.upsert('b.md', false);
    registry.setActive(idA);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.rename('b.md', 'b-renamed.md');
    expect(handler).not.toHaveBeenCalled();
  });

  test('does NOT fire when setActive is a no-op (same id)', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    registry.setActive(id);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.setActive(id);
    expect(handler).not.toHaveBeenCalled();
  });

  test('does NOT fire on same-path rename (rename to identical path)', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    registry.setActive(id);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.rename('a.md', 'a.md');
    expect(handler).not.toHaveBeenCalled();
  });

  test('fires (null, oldPath) on setActive(null)', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    registry.setActive(id);
    const handler = vi.fn();
    registry.subscribeActivePath(handler);

    registry.setActive(null);
    expect(handler).toHaveBeenCalledWith(null, 'a.md');
  });

  test('unsubscribe stops further notifications', () => {
    const registry = new EntryRegistry();
    const idA = registry.upsert('a.md', false);
    const idB = registry.upsert('b.md', false);
    const handler = vi.fn();
    const unsub = registry.subscribeActivePath(handler);

    registry.setActive(idA);
    unsub();
    registry.setActive(idB);

    expect(handler).toHaveBeenCalledTimes(1);
  });

  test('subscriber that throws does not abort the chain', () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    const throwing = vi.fn(() => { throw new Error('boom'); });
    const ok = vi.fn();
    registry.subscribeActivePath(throwing);
    registry.subscribeActivePath(ok);

    expect(() => registry.setActive(id)).not.toThrow();
    expect(ok).toHaveBeenCalledWith('a.md', null);
  });
});
