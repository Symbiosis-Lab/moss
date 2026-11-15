import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { syncReader, watchReader } from '../reader.js';

let cookie = '';
const events: boolean[] = [];
const onEvent = (e: Event) => events.push((e as CustomEvent<{ subscribed: boolean }>).detail.subscribed);
const subscribed = () => document.documentElement.hasAttribute('data-moss-subscribed');

describe('the reader state', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    cookie = '';
    events.length = 0;
    // jsdom will not store a `__Host-` cookie over http, so the page's view of document.cookie is stubbed.
    Object.defineProperty(document, 'cookie', { configurable: true, get: () => cookie });
    document.documentElement.removeAttribute('data-moss-subscribed');
    document.addEventListener('moss:subscribed', onEvent);
    fetchSpy = vi.spyOn(globalThis, 'fetch');
  });
  afterEach(() => {
    document.removeEventListener('moss:subscribed', onEvent);
    fetchSpy.mockRestore();
  });

  test('asks nothing of a browser with no hint', async () => {
    await syncReader();
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(subscribed()).toBe(false);
    expect(events).toEqual([]);
  });

  test('shows "Subscribed" from the hint at once, before the host answers, and keeps it on a 200', async () => {
    cookie = 'a=1; __Host-moss_subscribed=1';
    let answer!: (r: Response) => void;
    fetchSpy.mockReturnValue(new Promise<Response>((r) => (answer = r)));
    const done = syncReader();
    expect(subscribed()).toBe(true);
    expect(fetchSpy).toHaveBeenCalledWith('/_moss/account/session', expect.anything());
    answer(new Response('{"signed_in":true}', { status: 200 }));
    await done;
    expect(subscribed()).toBe(true);
    expect(events).toEqual([true]);
  });

  test('brings the form back when the host answers 401', async () => {
    cookie = '__Host-moss_subscribed=1';
    fetchSpy.mockResolvedValue(new Response('{"signed_in":false}', { status: 401 }));
    await syncReader();
    expect(subscribed()).toBe(false);
    expect(events).toEqual([true, false]);
  });

  test('keeps "Subscribed" through an outage', async () => {
    cookie = '__Host-moss_subscribed=1';
    fetchSpy.mockRejectedValueOnce(new TypeError('offline'));
    await syncReader();
    expect(subscribed()).toBe(true);
    fetchSpy.mockResolvedValueOnce(new Response('', { status: 502 }));
    await syncReader();
    expect(subscribed()).toBe(true);
  });

  test('picks up a sign-in from another tab when this one comes back', async () => {
    fetchSpy.mockResolvedValue(new Response('{"signed_in":true}', { status: 200 }));
    watchReader();
    expect(subscribed()).toBe(false);
    cookie = '__Host-moss_subscribed=1';
    document.dispatchEvent(new Event('visibilitychange'));
    expect(subscribed()).toBe(true);
    await vi.waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1));
  });
});
