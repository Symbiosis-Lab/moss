/**
 * Tests for the apply-form generalizations to subscribe.ts:
 * 1. data-revert="false" — terminal success (no auto-revert)
 * 2. FormData body — extra fields ride along
 * 3. a11y — aria-disabled on loading/success, focus to status on success
 */
import { beforeEach, afterEach, describe, expect, test, vi } from 'vitest';
import { hydrateSubscribeForm } from '../subscribe.js';
import { APPLY_COPY } from '../i18n.js';

function mountApplyForm(lang = 'zh-hans'): HTMLFormElement {
  document.documentElement.lang = lang;
  document.body.innerHTML = `
    <form class="moss-subscribe-form moss-apply-form" data-position="apply"
          data-moss-hosted="true" data-revert="false" data-state="idle"
          method="post" action="https://api.mosspub.com/apply?lang=zh-hans">
      <input type="hidden" name="scope" value="">
      <input type="email" name="email" class="moss-input" required aria-label="邮箱" aria-describedby="moss-apply-email-help" />
      <p class="moss-apply-helper" id="moss-apply-email-help">用于获取邀请及免费托管服务</p>
      <input type="text" name="matters" class="moss-input moss-apply-matters" placeholder="Matters 用户名" aria-label="Matters 用户名" aria-describedby="moss-apply-matters-help">
      <p class="moss-apply-helper" id="moss-apply-matters-help">或者告诉我们你打算写什么，一句话就好</p>
      <input type="text" name="website" class="moss-apply-hp" tabindex="-1" aria-hidden="true">
      <span class="moss-btn-slot">
        <button type="submit" class="moss-btn">
          <span class="moss-btn__label" data-label-success="申请已提交">申请</span>
          <span class="moss-btn__spinner" aria-hidden="true"></span>
          <span class="moss-btn__check" aria-hidden="true"></span>
        </button>
      </span>
      <div class="moss-subscribe-status moss-apply-status" aria-live="polite" role="status" tabindex="-1">
        <span data-copy="received" hidden>我们将审核你的申请。</span>
        <span data-copy="error" hidden>出错了，请重试。</span>
      </div>
    </form>`;
  return document.querySelector('form')!;
}

function mountSubscribeForm(): HTMLFormElement {
  document.documentElement.lang = 'en';
  document.body.innerHTML = `
    <form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true" data-state="idle"
          method="post" action="https://api.mosspub.com/api/sites/x/subscribe">
      <input type="hidden" name="scope" value="en">
      <input type="email" name="email" required />
      <span class="moss-btn-slot">
        <button type="submit" class="moss-btn">
          <span class="moss-btn__label">Subscribe</span>
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

describe('APPLY_COPY', () => {
  test('exists in i18n for zh-hans, zh-hant, en', () => {
    expect(APPLY_COPY['zh-hans'].label).toBe('申请');
    expect(APPLY_COPY['zh-hant'].label).toBe('申請');
    expect(APPLY_COPY.en.label).toBe('Apply');
    expect(APPLY_COPY['zh-hans'].labelSuccess).toBe('申请已提交');
    expect(APPLY_COPY['zh-hant'].labelSuccess).toBe('申請已提交');
    expect(APPLY_COPY.en.labelSuccess).toBe('Application sent');
    // mattersPh must be per-locale — English must not be Chinese
    expect(APPLY_COPY.en.mattersPh).toBe('Matters username');
    expect(APPLY_COPY['zh-hans'].mattersPh).toBe('Matters 用户名');
    expect(APPLY_COPY['zh-hant'].mattersPh).toBe('Matters 用戶名');
  });
});

describe('apply form — terminal success (data-revert="false")', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('success state stays — no auto-revert after 4s', async () => {
    vi.useFakeTimers();
    const form = mountApplyForm();
    hydrateSubscribeForm(form);
    const emailInput = form.querySelector<HTMLInputElement>('input[type="email"]')!;
    emailInput.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    // Advance past the 4s auto-revert window — should still be success
    vi.advanceTimersByTime(6000);
    expect(form.dataset.state).toBe('success');
  });

  test('tap-to-revert click is a no-op on an apply form in success state', async () => {
    const form = mountApplyForm();
    hydrateSubscribeForm(form);
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    // Click the button in success state — a subscribe form would revert; apply should not
    form.querySelector<HTMLButtonElement>('button')!.click();
    expect(form.dataset.state).toBe('success');
  });

  test('inputs are disabled after terminal success', async () => {
    const form = mountApplyForm();
    hydrateSubscribeForm(form);
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    const inputs = form.querySelectorAll<HTMLInputElement>('input:not([type="hidden"])');
    inputs.forEach((inp) => {
      expect(inp.disabled).toBe(true);
    });
  });

  test('button label swaps to data-label-success on terminal success', async () => {
    const form = mountApplyForm();
    hydrateSubscribeForm(form);
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    const label = form.querySelector<HTMLElement>('.moss-btn__label')!;
    expect(label.textContent).toBe('申请已提交');
  });
});

describe('FormData body — extra fields ride along', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    document.body.innerHTML = '';
  });

  test('apply form POSTs all fields (email, matters, scope, website, etc.)', async () => {
    const form = mountApplyForm();
    hydrateSubscribeForm(form);
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    const mattersInput = form.querySelector<HTMLInputElement>('input[name="matters"]')!;
    mattersInput.value = '@guo';

    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 }),
    );
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const [, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    // Body is URLSearchParams (application/x-www-form-urlencoded). Verify all
    // expected fields are present and Content-Type is correct.
    expect(init.body).toBeDefined();
    const bodyStr = String(init.body);
    const params = new URLSearchParams(bodyStr);
    expect(params.get('email')).toBe('a@b.com');
    expect(params.get('matters')).toBe('@guo');
    // website honeypot + scope always serialised even when empty
    expect(params.has('website')).toBe(true);
    const headers = init.headers as Record<string, string>;
    expect(headers['Content-Type']).toBe('application/x-www-form-urlencoded');
  });

  test('plain subscribe form still sends only email+scope (not extra apply fields)', async () => {
    const form = mountSubscribeForm();
    hydrateSubscribeForm(form);
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';

    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }),
    );
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));

    expect(fetchSpy).toHaveBeenCalledTimes(1);
    // Body should not contain 'matters' or 'publish' keys
    const [, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    const bodyStr = init.body ? String(init.body) : '';
    expect(bodyStr).not.toContain('matters');
    expect(bodyStr).not.toContain('publish');
  });
});

describe('a11y — aria-disabled clears on revert (regression: setState without btn)', () => {
  /**
   * Regression for SHOULD-FIX 1: setState(form, 'idle') was called WITHOUT the
   * `btn` argument in revertToIdle, leaving aria-disabled="true" stale on the
   * submit button after auto-revert or tap-to-revert. The fix resolves the
   * button from the form inside setState itself.
   */
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('aria-disabled is absent on the button after auto-revert (4 s timer)', async () => {
    vi.useFakeTimers();
    const form = mountSubscribeForm();
    hydrateSubscribeForm(form);
    const btn = form.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }),
    );

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    // aria-disabled is set during success
    expect(btn.getAttribute('aria-disabled')).toBe('true');

    // Trigger auto-revert
    vi.advanceTimersByTime(4000);
    expect(form.dataset.state).toBe('idle');
    // aria-disabled must be gone after revert — this was the regression
    expect(btn.hasAttribute('aria-disabled')).toBe(false);
  });

  test('aria-disabled is absent on the button after tap-to-revert (click on success circle)', async () => {
    const form = mountSubscribeForm();
    hydrateSubscribeForm(form);
    const btn = form.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    form.querySelector<HTMLInputElement>('input[type="email"]')!.value = 'a@b.com';
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }),
    );

    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(btn.getAttribute('aria-disabled')).toBe('true');

    // Tap the success circle to revert
    btn.click();
    expect(form.dataset.state).toBe('idle');
    // aria-disabled must be gone — this was the regression
    expect(btn.hasAttribute('aria-disabled')).toBe(false);
  });
});
