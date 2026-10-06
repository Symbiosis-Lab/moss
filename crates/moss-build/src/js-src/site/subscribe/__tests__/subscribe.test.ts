import { beforeEach, afterEach, describe, expect, test, vi } from 'vitest';
import { hydrateSubscribeForm } from '../subscribe.js';
import { COPY, langBucket } from '../i18n.js';

function mount(lang = 'en'): HTMLFormElement {
  document.documentElement.lang = lang;
  document.body.innerHTML = `
    <form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true" data-state="idle"
          method="post" action="https://api.example.com/api/sites/my-site/subscribe">
      <input type="email" name="email" class="moss-input" required />
      <span class="moss-btn-slot">
        <button type="submit" class="moss-btn">
          <span class="moss-btn__label"></span>
          <span class="moss-btn__spinner"></span>
          <span class="moss-btn__check"><svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7"/></svg></span>
        </button>
      </span>
      <div class="moss-subscribe-status" aria-live="polite">
        <span data-copy="check-email" hidden></span>
        <span data-copy="already-subscribed" hidden></span>
        <span data-copy="error" hidden></span>
      </div>
    </form>`;
  return document.querySelector('form')!;
}

describe('langBucket', () => {
  test('routes zh-Hant, zh-TW, zh-HK to zh-hant', () => {
    expect(langBucket('zh-Hant')).toBe('zh-hant');
    expect(langBucket('zh-TW')).toBe('zh-hant');
    expect(langBucket('zh-HK')).toBe('zh-hant');
  });
  test('routes zh-Hans, zh-CN, bare zh to zh-hans', () => {
    expect(langBucket('zh-Hans')).toBe('zh-hans');
    expect(langBucket('zh-CN')).toBe('zh-hans');
    expect(langBucket('zh')).toBe('zh-hans');
  });
  test('routes unknown/empty to en', () => {
    expect(langBucket('')).toBe('en');
    expect(langBucket(null)).toBe('en');
    expect(langBucket('fr')).toBe('en');
  });
  // Kept in sync with Rust `from_bcp47_lenient` (crates/moss-build/src/i18n.rs).
  test('underscore forms bucket by region, not collapse to Simplified', () => {
    expect(langBucket('zh_TW')).toBe('zh-hant');
    expect(langBucket('zh_HK')).toBe('zh-hant');
    expect(langBucket('zh_CN')).toBe('zh-hans');
  });
  test('Sinitic codes and bare scripts are Chinese, not en', () => {
    expect(langBucket('cmn')).toBe('zh-hans');
    expect(langBucket('zho')).toBe('zh-hans');
    expect(langBucket('yue-HK')).toBe('zh-hant');
    expect(langBucket('Hans')).toBe('zh-hans');
    expect(langBucket('Hant')).toBe('zh-hant');
    expect(langBucket('zh-Hans-HK')).toBe('zh-hans');
  });
});

describe('hydrateSubscribeForm', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    document.body.innerHTML = '';
  });

  test('seeds the button label and placeholder from the active lang', () => {
    const form = mount('zh-Hant');
    hydrateSubscribeForm(form);
    const input = form.querySelector('input')!;
    const label = form.querySelector('.moss-btn__label')!;
    expect(input.placeholder).toBe(COPY['zh-hant'].placeholder);
    expect(label.textContent).toBe(COPY['zh-hant'].label);
  });

  test('data-button-override leaves the author label and placeholder untouched', () => {
    const form = mount('zh-Hant');
    // Simulate an author override block: :::subscribe{button="申請" placeholder="您的邮箱"}
    form.dataset.buttonOverride = 'true';
    const input = form.querySelector('input')!;
    const label = form.querySelector('.moss-btn__label')!;
    input.placeholder = '您的邮箱';
    label.textContent = '申請';
    hydrateSubscribeForm(form);
    expect(input.placeholder).toBe('您的邮箱');
    expect(label.textContent).toBe('申請');
  });

  test('is idempotent — hydrating twice binds one submit handler', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    hydrateSubscribeForm(form);
    const input = form.querySelector('input')!;
    input.value = 'test@example.com';
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  test('re-hydrating after the runtime marker is stripped (idiomorph behavior) does not bind a second submit listener', () => {
    // Regression guard for the double-POST bug: idiomorph strips any
    // attribute the runtime added that isn't present in the freshly
    // rendered HTML — including a `data-moss-hydrated` guard flag, on the
    // very node whose listener it's supposed to protect. Simulate that by
    // clearing the (now-vestigial) dataset attribute directly, on the SAME
    // form element idiomorph would have preserved, and re-hydrating it the
    // way a post-morph `autoHydrate()` sweep does. Assert on listener
    // REGISTRATION count directly — asserting on fetch-call count would
    // pass even with two bound listeners, because the handler's own
    // `state === 'loading'` check makes the second (synchronously
    // re-entrant) invocation within the same submit a no-op regardless of
    // whether the identity guard worked.
    const form = mount('en');
    const addSpy = vi.spyOn(form, 'addEventListener');
    hydrateSubscribeForm(form);
    delete form.dataset.mossHydrated; // what idiomorph would do to the old attribute guard
    hydrateSubscribeForm(form);

    const submitBindCount = addSpy.mock.calls.filter(([type]) => type === 'submit').length;
    expect(submitBindCount).toBe(1);
  });

  test('moss-morph-patched hydrates a form that appears for the first time after a morph', () => {
    // The morph listener must also cover the "never hydrated yet" case —
    // e.g. a `:::subscribe` block that only exists in the post-morph DOM
    // (conditionally rendered, or added by a later edit) — not just the
    // re-bind-guard case above.
    document.body.innerHTML = '';
    const form = mount('en');
    // Not hydrated yet — no explicit hydrateSubscribeForm(form) call.
    document.dispatchEvent(new CustomEvent('moss-morph-patched'));

    const label = form.querySelector('.moss-btn__label')!;
    expect(label.textContent).toBe(COPY.en.label);
  });

  test('happy path: state goes idle → loading → success, check-email copy visible', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    const input = form.querySelector('input')!;
    input.value = 'test@example.com';

    let resolve!: (r: Response) => void;
    fetchSpy.mockReturnValue(new Promise<Response>((r) => { resolve = r; }));

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('loading'));

    resolve(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    const checkEmail = form.querySelector<HTMLElement>('[data-copy="check-email"]')!;
    expect(checkEmail.hidden).toBe(false);
    expect(checkEmail.textContent).toBe(COPY.en.checkEmail);
  });

  test('alreadySubscribed: shows the already-subscribed copy, not check-email', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    form.querySelector('input')!.value = 'already@example.com';
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, alreadySubscribed: true }), { status: 200 }));

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    const already = form.querySelector<HTMLElement>('[data-copy="already-subscribed"]')!;
    const checkEmail = form.querySelector<HTMLElement>('[data-copy="check-email"]')!;
    expect(already.hidden).toBe(false);
    expect(checkEmail.hidden).toBe(true);
  });

  test('error path: state = error, error copy visible', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    form.querySelector('input')!.value = 'bad@example.com';
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ error: 'Invalid email' }), { status: 400 }));

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('error'));

    const err = form.querySelector<HTMLElement>('[data-copy="error"]')!;
    expect(err.hidden).toBe(false);
    expect(err.textContent).toBe(COPY.en.error);
  });

  test('empty input: submit is a no-op, fetch not called, state stays idle', () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    form.querySelector('input')!.value = '   ';
    form.requestSubmit();
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(form.dataset.state).toBe('idle');
  });

  test('auto-reverts to idle after 4s and clears the input', async () => {
    vi.useFakeTimers();
    try {
      const form = mount('en');
      hydrateSubscribeForm(form);
      const input = form.querySelector<HTMLInputElement>('input')!;
      input.value = 'test@example.com';
      fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));

      form.requestSubmit();
      await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

      vi.advanceTimersByTime(4000);
      expect(form.dataset.state).toBe('idle');
      expect(input.value).toBe('');
    } finally {
      vi.useRealTimers();
    }
  });

  test('tap-to-revert: clicking the circle in success state returns to idle', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    const input = form.querySelector<HTMLInputElement>('input')!;
    input.value = 'test@example.com';
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    form.querySelector<HTMLButtonElement>('button')!.click();
    expect(form.dataset.state).toBe('idle');
    expect(input.value).toBe('');
  });

  test('alreadySubscribed branch also auto-reverts', async () => {
    vi.useFakeTimers();
    try {
      const form = mount('en');
      hydrateSubscribeForm(form);
      form.querySelector('input')!.value = 'already@example.com';
      fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, alreadySubscribed: true }), { status: 200 }));

      form.requestSubmit();
      await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
      vi.advanceTimersByTime(4000);
      expect(form.dataset.state).toBe('idle');
    } finally {
      vi.useRealTimers();
    }
  });

  test('second submit after revert reaches success cleanly', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    const input = form.querySelector<HTMLInputElement>('input')!;
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));

    input.value = 'first@example.com';
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    form.querySelector<HTMLButtonElement>('button')!.click();
    expect(form.dataset.state).toBe('idle');
    expect(input.value).toBe('');

    input.value = 'second@example.com';
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  test('click on the button while idle does NOT trigger revert', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    const btn = form.querySelector<HTMLButtonElement>('button')!;
    // In idle the button click should behave as a normal submit-button click
    // (no revert side effects, no state change on its own).
    btn.click();
    expect(form.dataset.state).toBe('idle');
  });

  test('drift regression: slot width locks on first submit and is stable across re-submits', async () => {
    const form = mount('en');
    hydrateSubscribeForm(form);
    const input = form.querySelector<HTMLInputElement>('input')!;
    const btn = form.querySelector<HTMLButtonElement>('button')!;
    const slot = form.querySelector<HTMLElement>('.moss-btn-slot')!;
    // jsdom has no layout engine (offsetWidth is 0). Stub it to simulate the
    // real-browser path where lockSlotWidth() captures a natural width.
    Object.defineProperty(btn, 'offsetWidth', { configurable: true, value: 96 });
    fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));

    input.value = 'first@example.com';
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(slot.style.width).toBe('96px');

    btn.click();
    expect(form.dataset.state).toBe('idle');

    // Simulate what would happen if lockSlotWidth re-measured a button that
    // has already been styled for success (36px circle). If the lock didn't
    // hold, the slot would shrink on re-submit — the original drift bug.
    Object.defineProperty(btn, 'offsetWidth', { configurable: true, value: 36 });
    input.value = 'second@example.com';
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(slot.style.width).toBe('96px');
  });

  test('revert is a no-op on a detached form — stale timer cannot clobber state', async () => {
    vi.useFakeTimers();
    try {
      const form = mount('en');
      hydrateSubscribeForm(form);
      const input = form.querySelector<HTMLInputElement>('input')!;
      input.value = 'detached@example.com';
      fetchSpy.mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));

      form.requestSubmit();
      // Flush microtasks so the fetch resolves and the 4s timer is set.
      await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

      form.remove();
      vi.advanceTimersByTime(4000);
      // The timer fired but the isConnected guard in revertToIdle should have
      // left data-state untouched on the detached node.
      expect(form.dataset.state).toBe('success');
    } finally {
      vi.useRealTimers();
    }
  });
});

function makePreviewForm(): HTMLFormElement {
  document.body.innerHTML = `
    <form class="moss-subscribe-form" data-moss-hosted="true" data-position="inline" data-button-override="true"
          method="post" action="https://api.mosspub.com/api/sites/x/subscribe">
      <input type="email" />
      <span class="moss-btn-slot">
        <button type="submit"><span class="moss-btn__label">Request access</span></button>
      </span>
      <div class="moss-subscribe-status" aria-live="polite">
        <span data-copy="check-email" hidden></span>
        <span data-copy="already-subscribed" hidden></span>
        <span data-copy="error" hidden></span>
      </div>
    </form>`;
  return document.querySelector('form')!;
}

describe('subscribe preview mode', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
    vi.useFakeTimers();
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    vi.useRealTimers();
    document.body.removeAttribute('data-moss-preview');
  });

  test('runs success animation without fetching or inline hint in preview', async () => {
    document.body.setAttribute('data-moss-preview', '');
    const form = makePreviewForm();
    hydrateSubscribeForm(form);
    (form.querySelector('input')!).value = 'a@b.com';

    form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
    expect(form.dataset.state).toBe('loading');

    await vi.advanceTimersByTimeAsync(700);

    expect(fetchSpy).not.toHaveBeenCalled();
    expect(form.dataset.state).toBe('success');
    // The "local preview" hint is now a CSS hover tooltip on the button —
    // it must NOT be rendered inline in the status area anymore.
    expect(form.querySelector('[data-copy="preview-hint"]')).toBeNull();
  });

  test('preview success reveals the check-email status copy like a real submit', async () => {
    document.body.setAttribute('data-moss-preview', '');
    const form = makePreviewForm();
    hydrateSubscribeForm(form);
    (form.querySelector('input')!).value = 'a@b.com';

    form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
    await vi.advanceTimersByTimeAsync(700);

    expect(form.dataset.state).toBe('success');
    const span = form.querySelector<HTMLElement>('[data-copy="check-email"]')!;
    expect(span.hidden).toBe(false);
    expect(span.textContent).toBe(COPY.en.checkEmail);
    expect(form.querySelector<HTMLElement>('.moss-subscribe-status')!.dataset.state).toBe('check-email');
  });
});
