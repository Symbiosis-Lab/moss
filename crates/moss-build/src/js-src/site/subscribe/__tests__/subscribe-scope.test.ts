import { describe, it, expect, vi, beforeEach } from 'vitest';
import { hydrateSubscribeForm } from '../subscribe.js';

describe('subscribe form sends scope in POST body', () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    global.fetch = fetchMock as unknown as typeof fetch;
    document.body.innerHTML = '';
    document.documentElement.lang = 'en';
  });

  function buildForm(scope: string | null): HTMLFormElement {
    const form = document.createElement('form');
    form.classList.add('moss-subscribe-form');
    form.dataset.mossHosted = 'true';
    form.dataset.position = 'inline';
    form.action = 'https://api.mosspub.com/api/sites/abc/subscribe';

    if (scope !== null) {
      const scopeInput = document.createElement('input');
      scopeInput.type = 'hidden';
      scopeInput.name = 'scope';
      scopeInput.value = scope;
      form.appendChild(scopeInput);
    }

    const emailInput = document.createElement('input');
    emailInput.type = 'email';
    emailInput.name = 'email';
    emailInput.value = 'alice@example.com';
    form.appendChild(emailInput);

    const slot = document.createElement('span');
    slot.classList.add('moss-btn-slot');
    const btn = document.createElement('button');
    btn.type = 'submit';
    const label = document.createElement('span');
    label.classList.add('moss-btn__label');
    label.textContent = 'Subscribe';
    btn.appendChild(label);
    slot.appendChild(btn);
    form.appendChild(slot);

    const status = document.createElement('div');
    status.classList.add('moss-subscribe-status');
    for (const key of ['check-email', 'already-subscribed', 'error']) {
      const span = document.createElement('span');
      span.dataset.copy = key;
      span.hidden = true;
      status.appendChild(span);
    }
    form.appendChild(status);

    document.body.appendChild(form);
    return form;
  }

  it('includes scope from the hidden input in the JSON body', async () => {
    const form = buildForm('zh');
    hydrateSubscribeForm(form);
    form.requestSubmit();
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body).toEqual({ email: 'alice@example.com', scope: 'zh' });
  });

  it('sends scope="" when the hidden input value is empty', async () => {
    const form = buildForm('');
    hydrateSubscribeForm(form);
    form.requestSubmit();
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.scope).toBe('');
  });

  it('omits scope when the hidden input is absent (defensive)', async () => {
    const form = buildForm(null);
    hydrateSubscribeForm(form);
    form.requestSubmit();
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect('scope' in body).toBe(false);
  });
});
