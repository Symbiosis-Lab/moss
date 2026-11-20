import { beforeEach, afterEach, expect, test, vi } from 'vitest';
import { hydrateSubscribeForm } from '../subscribe.js';
import { prewarmCaptcha } from '../captcha.js';

let intersect: IntersectionObserverCallback;
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('IntersectionObserver', class {
    constructor(callback: IntersectionObserverCallback) { intersect = callback; }
    observe() {} disconnect() {}
  });
  document.body.innerHTML = `<form action="https://api.example.com/sites/demo/subscribe" data-state="idle">
    <input type="email" value="reader@example.com"><span class="moss-btn-slot"><button type="submit"><span class="moss-btn__label">Subscribe</span></button></span>
    <div class="moss-subscribe-status"><span data-copy="check-email" hidden></span></div></form>`;
});
afterEach(() => {
  window.dispatchEvent(new Event('pagehide'));
  document.body.innerHTML = '';
  vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
const form = () => document.querySelector('form')!;
const frame = () => document.querySelector<HTMLIFrameElement>('[data-moss-captcha-prewarm]')!;
function tell(state: string, token?: string, origin = 'https://api.example.com') {
  window.dispatchEvent(new MessageEvent('message', { source: frame().contentWindow, origin, data: { moss_prewarm: state, token } }));
}

test.each(['focus', 'intersection'])('prewarms on %s and consumes only a ready unexpired token from the expected origin/frame', trigger => {
  const take = prewarmCaptcha(form());
  expect(frame()).toBeNull();
  if (trigger === 'focus') form().dispatchEvent(new FocusEvent('focusin'));
  else intersect([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
  expect(frame().src).toContain('/captcha/subscription-prewarm/demo/');
  expect(frame().hidden).toBe(true);
  expect(frame().tabIndex).toBe(-1);
  expect(frame().getAttribute('aria-hidden')).toBe('true');
  tell('ready', 'forged', 'https://wrong.example.com');
  expect(take()).toEqual({ turnstile_started: false });
  window.dispatchEvent(new MessageEvent('message', { source: window, origin: 'https://api.example.com', data: { moss_prewarm: 'ready', token: 'forged' } }));
  expect(take()).toEqual({ turnstile_started: false });
  tell('started');
  expect(take()).toEqual({ turnstile_started: true });
  tell('ready', 'valid-token');
  expect(take()).toEqual({ turnstile_started: true, turnstile_token: 'valid-token' });
  expect(take()).toEqual({ turnstile_started: true });
  expect(frame()).toBeNull();
});

test.each(['age', 'callback'])('does not send a token expired by %s', how => {
  const take = prewarmCaptcha(form());
  form().dispatchEvent(new FocusEvent('focusin'));
  tell('ready', 'expired-token');
  if (how === 'age') vi.advanceTimersByTime(240_000);
  else tell('expired');
  expect(take()).toEqual({ turnstile_started: true });
});

test.each([false, true])('press shows loading immediately, submits without waiting, and accepts independently of prewarm (ready=%s)', ready => {
  let resolve!: (response: Response) => void;
  const post = vi.fn(() => new Promise<Response>(r => { resolve = r; }));
  vi.stubGlobal('fetch', post);
  hydrateSubscribeForm(form());
  form().dispatchEvent(new FocusEvent('focusin'));
  tell('started');
  if (ready) tell('ready', 'ready-token');
  const hp = form().querySelector<HTMLInputElement>('input[name="website"]')!;
  expect(hp.closest('label')?.hidden).toBe(true);
  expect(hp.labels?.[0].textContent).toBe('Leave this field empty');
  expect(hp.tabIndex).toBe(-1);
  expect(hp.value).toBe('');
  expect(hp.autocomplete).toBe('off');
  hp.value = 'trap';
  form().requestSubmit();
  expect(form().dataset.state).toBe('loading');
  expect(post).toHaveBeenCalledOnce();
  const sent = JSON.parse((post.mock.calls as unknown as [string, RequestInit][])[0][1].body as string);
  expect(sent).toMatchObject({ website: 'trap', load_to_submit_ms: expect.any(Number), turnstile_started: true });
  expect(sent.turnstile_token).toBe(ready ? 'ready-token' : undefined);
  resolve(new Response(JSON.stringify({ subscribed: true, confirmationSent: true })));
  return vi.advanceTimersByTimeAsync(0).then(() => expect(form().dataset.state).toBe('success'));
});
