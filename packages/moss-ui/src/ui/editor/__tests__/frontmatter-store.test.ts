// packages/moss-ui/src/ui/editor/__tests__/frontmatter-store.test.ts
import { describe, it, expect, vi, beforeEach, afterEach, test } from 'vitest';
import { FrontmatterStore } from '../frontmatter-store';

describe('FrontmatterStore', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('notifies subscribers synchronously on setField', () => {
    const store = new FrontmatterStore({ initial: { title: 'A' }, requestSave: vi.fn().mockResolvedValue(undefined) });
    const cb = vi.fn();
    store.subscribe(cb);
    store.setField('title', 'B');
    expect(cb).toHaveBeenCalledWith({ title: 'B' });
  });

  it('debounces write by 150ms for text-like changes', async () => {
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: {}, requestSave });
    store.setField('description', 'hi');
    store.setField('description', 'hi there');
    expect(requestSave).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(149);
    expect(requestSave).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(2);
    expect(requestSave).toHaveBeenCalledTimes(1);
    expect(store.getValues()).toEqual({ description: 'hi there' });
  });

  it('writes immediately (no debounce) when immediate=true (toggles/selects)', async () => {
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: {}, requestSave });
    store.setField('draft', true, { immediate: true });
    // Allow microtasks to flush
    await Promise.resolve();
    expect(requestSave).toHaveBeenCalledTimes(1);
  });

  it('reconciles external edits via diff-merge (only changed keys trigger notify)', () => {
    const store = new FrontmatterStore({
      initial: { x: 1, y: 2 }, requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const cb = vi.fn();
    store.subscribe(cb);

    store.onFileChanged({ x: 1, y: 99 });
    expect(cb).toHaveBeenCalledTimes(1);
    expect(cb).toHaveBeenCalledWith({ x: 1, y: 99 });
    expect(store.getValues()).toEqual({ x: 1, y: 99 });
  });

  it('does not notify when external state matches in-memory state', () => {
    const store = new FrontmatterStore({
      initial: { x: 1 }, requestSave: vi.fn().mockResolvedValue(undefined),
    });
    const cb = vi.fn();
    store.subscribe(cb);
    store.onFileChanged({ x: 1 });
    expect(cb).not.toHaveBeenCalled();
  });

  it('removeField writes immediately (no debounce)', async () => {
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: { x: 1 }, requestSave });
    store.removeField('x');
    await Promise.resolve();
    expect(requestSave).toHaveBeenCalledTimes(1);
    expect(store.getValues()).toEqual({});  // empty after removal
  });

  test('store holds no path and no write fn', () => {
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: { title: 'A' }, requestSave });
    expect((store as any).filePath).toBeUndefined();
    expect((store as any).write).toBeUndefined();
    expect(typeof (store as any).getFilePath).toBe('undefined');
  });

  test('removeField asks the single writer to save exactly once', async () => {
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: { title: 'A' }, requestSave });
    store.removeField('title'); await Promise.resolve();
    expect(requestSave).toHaveBeenCalledTimes(1);
    expect(store.getValues()).toEqual({});
  });

  test('debounced setField coalesces to one requestSave; immediate fires now', async () => {
    vi.useFakeTimers();
    const requestSave = vi.fn().mockResolvedValue(undefined);
    const store = new FrontmatterStore({ initial: {}, requestSave, debounceMs: 150 });
    store.setField('a', 1); store.setField('a', 2);
    expect(requestSave).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(150);
    expect(requestSave).toHaveBeenCalledTimes(1);
    store.setField('b', true, { immediate: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(requestSave).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  test('hasPendingWrite reflects a queued debounce timer', () => {
    vi.useFakeTimers();
    const store = new FrontmatterStore({ initial: {}, requestSave: vi.fn().mockResolvedValue(undefined), debounceMs: 150 });
    expect(store.hasPendingWrite()).toBe(false);
    store.setField('a', 1);
    expect(store.hasPendingWrite()).toBe(true);
    vi.advanceTimersByTime(150);
    expect(store.hasPendingWrite()).toBe(false);
    vi.useRealTimers();
  });
});
