/**
 * The browser's host record fills what the HTTP carrier and the page provide,
 * and nothing a browser lacks; the carrier rides the session cookie.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { host, inPageBus } from '../../ui/host';
import { resetTestHost } from '../../ui/__tests__/helpers/test-host';
import { fillBrowserHost } from '../host';
import { SignedOut } from '../carrier';

const fetchMock = vi.fn();

function fill() {
  for (const key of Object.keys(host)) delete (host as unknown as Record<string, unknown>)[key];
  fillBrowserHost(inPageBus());
  return host;
}

function stream(...frames: string[]): ReadableStream<Uint8Array> {
  const bytes = new TextEncoder().encode(frames.join(''));
  return new ReadableStream({ start: (c) => { c.enqueue(bytes); } });
}

describe('the browser host record', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    resetTestHost();
  });

  it('has the engine, the bus, links and file picks, and nothing desktop-only', () => {
    expect(Object.keys(fill()).sort()).toEqual(['bus', 'engine', 'openExternal', 'pickFiles', 'sourceAssetBase']);
  });

  it("finds the folder's sources behind the session, on its own origin", () => {
    expect(fill().sourceAssetBase).toBe('/__moss/source');
  });

  it('sends each command to the route for its tier with the cookie and no credential of its own', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify(['a']), { status: 200 }));
    expect(await fill().engine.call('list_vault_terms', { field: 'tags' })).toEqual(['a']);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe('/__moss/read/list_vault_terms');
    expect(init.credentials).toBe('same-origin');
    expect(init.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(init.body)).toEqual({ field: 'tags' });

    fetchMock.mockResolvedValue(new Response('', { status: 200 }));
    await host.engine.call('save_editor_content', {});
    expect(fetchMock.mock.calls[1][0]).toBe('/__moss/mutate/save_editor_content');
  });

  it("throws the command's own error as its string, and a refused session as signed out", async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: 'no such file' }), { status: 500 }));
    await expect(fill().engine.call('read_source_text', {})).rejects.toBe('no such file');
    fetchMock.mockResolvedValue(new Response('', { status: 401 }));
    await expect(host.engine.call('read_source_text', {})).rejects.toBeInstanceOf(SignedOut);
  });

  it("hears the engine's events on the carrier's stream, as payloads", async () => {
    fetchMock.mockResolvedValue(new Response(stream(': keep-alive\n\n', 'data: {"name":"moss-event","payload":{"kind":"BuildComplete"}}\n\n')));
    const seen: unknown[] = [];
    const off = await fill().engine.on('moss-event', (p) => seen.push(p));
    await vi.waitFor(() => expect(seen).toEqual([{ kind: 'BuildComplete' }]));
    expect(fetchMock.mock.calls[0][0]).toBe('/__moss/events');
    expect(fetchMock.mock.calls[0][1].credentials).toBe('same-origin');
    off();
  });
});
