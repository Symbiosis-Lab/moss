import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../../engine', () => ({ engine: { resolvePageSource: vi.fn() } }));

import { engine } from '../../engine';
import { eventBus, Events } from '../../context/event-bus';
import { pageSource } from '../page-source';

const resolve = vi.mocked(engine.resolvePageSource);
const answer = (data: Record<string, unknown>) =>
  ({ status: 'ok', data: { is_dir: false, is_article: false, takeover: null, ...data } }) as never;

function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise((r) => { resolve = r; });
  return { promise, resolve };
}

let heard: ReturnType<typeof vi.fn>;
beforeEach(() => {
  pageSource.invalidate();
  resolve.mockReset();
  eventBus.clear();
  heard = vi.fn();
  eventBus.on(Events.PREVIEW_URL_CHANGED, heard);
});
afterEach(() => vi.useRealTimers());

describe('PageSource', () => {
  test('a re-ask during a navigation joins its lookup: one announcement, for the new page, with its source', async () => {
    const lookup = deferred();
    resolve.mockReturnValueOnce(lookup.promise as never);
    pageSource.invalidate('/old/');
    const navigation = pageSource.navigate('/new/', false);
    expect(await pageSource.refresh('/old/')).toBeNull();
    const reask = pageSource.refresh('/new/');
    lookup.resolve(answer({ source_path: '/site/new.md', is_article: true }));
    await Promise.all([navigation, reask]);

    expect(resolve).toHaveBeenCalledOnce();
    expect(heard).toHaveBeenCalledOnce();
    expect(heard).toHaveBeenCalledWith(expect.objectContaining({ urlPath: '/new/', sourcePath: '/site/new.md', isReload: false, pending: null }));
    expect(pageSource.sourceFile('/new/')).toBe('/site/new.md');
  });

  test('a lookup for a page the preview left publishes nothing', async () => {
    const lookup = deferred();
    resolve.mockReturnValueOnce(lookup.promise as never);
    const first = pageSource.navigate('/a/', false);
    resolve.mockResolvedValueOnce(answer({ source_path: '/site/b.md' }));
    await pageSource.navigate('/b/', false);
    lookup.resolve(answer({ source_path: '/site/a.md' }));
    expect(await first).toBeNull();
    expect(heard).toHaveBeenCalledOnce();
    expect(pageSource.current?.sourcePath).toBe('/site/b.md');
  });

  test('a failed lookup asks again a bounded number of times, and heals when one succeeds', async () => {
    vi.useFakeTimers();
    resolve.mockResolvedValue({ status: 'error', error: 'busy' } as never);
    await pageSource.navigate('/a/', false);
    expect(pageSource.current?.pending).toBe('lookup');

    resolve.mockResolvedValueOnce({ status: 'error', error: 'busy' } as never);
    resolve.mockResolvedValueOnce(answer({ source_path: '/site/a.md', is_article: true }));
    await vi.advanceTimersByTimeAsync(1000);
    expect(pageSource.current?.pending).toBe('lookup');
    await vi.advanceTimersByTimeAsync(3000);
    expect(pageSource.current).toEqual(expect.objectContaining({ sourcePath: '/site/a.md', pending: null }));
    // The healed answer changes what the page is, so the bus hears it.
    expect(heard).toHaveBeenLastCalledWith(expect.objectContaining({ sourcePath: '/site/a.md', isArticle: true }));

    resolve.mockClear();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(resolve).not.toHaveBeenCalled();
  });

  test('a lookup that keeps failing stops asking', async () => {
    vi.useFakeTimers();
    resolve.mockResolvedValue({ status: 'error', error: 'busy' } as never);
    await pageSource.navigate('/a/', false);
    await vi.advanceTimersByTimeAsync(60_000);
    const asked = resolve.mock.calls.length;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(resolve).toHaveBeenCalledTimes(asked);
    expect(asked).toBeLessThan(10);
  });

  test('a page the build has not mapped yet waits for the reload, with no retry', async () => {
    vi.useFakeTimers();
    resolve.mockResolvedValue(answer({ source_path: null, source_known: false }));
    await pageSource.navigate('/a/', false);
    expect(pageSource.current?.pending).toBe('lookup');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(resolve).toHaveBeenCalledOnce();
  });

  test('a failed re-ask keeps the answer it could not improve on', async () => {
    resolve.mockResolvedValueOnce(answer({ source_path: '/site/a.md' }));
    await pageSource.navigate('/a/', false);
    resolve.mockResolvedValueOnce({ status: 'error', error: 'busy' } as never);
    await pageSource.navigate('/a/', true);
    expect(pageSource.current).toEqual(expect.objectContaining({ sourcePath: '/site/a.md', pending: null }));
  });

  test('an answer looked up before the preview arrives is announced when it does', async () => {
    resolve.mockResolvedValueOnce(answer({ source_path: '/site/a.md' }));
    await pageSource.ensure('/a/');
    expect(heard).not.toHaveBeenCalled();
    await pageSource.navigate('/a/', false);
    expect(resolve).toHaveBeenCalledOnce();
    expect(heard).toHaveBeenCalledWith(expect.objectContaining({ sourcePath: '/site/a.md', isReload: false }));
  });
});
