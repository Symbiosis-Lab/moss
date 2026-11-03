import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { hydrateForm, navigation, prefill } from '../forms.js';

const COPY = `<div hidden>
  <span data-copy="required">This field is required.</span>
  <span data-copy="format">Check this value.</span>
  <span data-copy="invalid">Some fields need another look.</span>
  <span data-copy="rate_limited">Too many requests.</span>
  <span data-copy="unknown_form">Not available.</span>
  <span data-copy="network">Could not send.</span>
</div>`;

function mount(): HTMLFormElement {
  document.body.removeAttribute('data-moss-preview');
  document.body.innerHTML = `
  <div class="moss-form" data-state="idle">
  <form method="post" action="https://forms.example/f/booking" data-moss-form="booking" data-next="/booking/received/">
    <div class="moss-form__field" data-field="your_name">
      <label for="booking-your_name">Your name</label>
      <input id="booking-your_name" name="your_name" type="text" required aria-describedby="booking-your_name-help">
      <p id="booking-your_name-help">Help</p>
      <p class="moss-form__error" id="booking-your_name-error" hidden></p>
    </div>
    <div class="moss-form__field" data-field="email">
      <label for="booking-email">Email</label>
      <input id="booking-email" name="email" type="email" required>
      <p class="moss-form__error" id="booking-email-error" hidden></p>
    </div>
    <div class="moss-form__field" data-field="note">
      <textarea id="booking-note" name="note"></textarea>
      <p class="moss-form__error" id="booking-note-error" hidden></p>
    </div>
    <div class="moss-form__field" data-field="colour">
      <select id="booking-colour" name="colour"><option value=""></option><option value="Red">Red</option><option value="Blue">Blue</option></select>
      <p class="moss-form__error" id="booking-colour-error" hidden></p>
    </div>
    <fieldset class="moss-form__group" data-field="session">
      <legend>Session</legend>
      <div><input type="radio" id="booking-session-0" name="session" value="Portrait"><label for="booking-session-0">Portrait</label></div>
      <div><input type="radio" id="booking-session-1" name="session" value="Family"><label for="booking-session-1">Family</label></div>
      <p class="moss-form__error" id="booking-session-error" hidden></p>
    </fieldset>
    <div class="moss-form__field" data-field="news">
      <input type="checkbox" id="booking-news" name="news" value="on"><label for="booking-news">News</label>
      <p class="moss-form__error" id="booking-news-error" hidden></p>
    </div>
    <input type="hidden" name="_next" value="/booking/received/">
    <div class="moss-form__hp" aria-hidden="true"><input type="text" name="_hp" value="" tabindex="-1" autocomplete="off"></div>
    <span class="moss-btn-slot"><button type="submit" class="moss-btn"><span class="moss-btn__label">Send</span></button></span>
    <p class="moss-form__status" aria-live="polite"></p>
    ${COPY}
  </form></div>`;
  const form = document.querySelector('form')!;
  (form.querySelector('[name="your_name"]') as HTMLInputElement).value = 'Ada Example';
  (form.querySelector('[name="email"]') as HTMLInputElement).value = 'ada@example.test';
  return form;
}

const reply = (status: number, body: unknown) =>
  Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } }));
const submit = async (form: HTMLFormElement) => {
  form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
  await new Promise((r) => setTimeout(r, 0));
};

describe('forms', () => {
  let fetchSpy: ReturnType<typeof vi.spyOn>;
  let go: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch');
    go = vi.spyOn(navigation, 'to').mockImplementation(() => {});
  });
  afterEach(() => {
    fetchSpy.mockRestore();
    go.mockRestore();
  });

  test('posts the fields urlencoded with Accept: application/json, reserved fields included', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(200, { ok: true }));
    hydrateForm(form, '');
    await submit(form);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('https://forms.example/f/booking');
    expect(init.method).toBe('POST');
    const headers = init.headers as Record<string, string>;
    expect(headers['Content-Type']).toBe('application/x-www-form-urlencoded');
    expect(headers['Accept']).toBe('application/json');
    const body = new URLSearchParams(init.body as string);
    expect(body.get('your_name')).toBe('Ada Example');
    expect(body.get('email')).toBe('ada@example.test');
    expect(body.get('_next')).toBe('/booking/received/');
    expect(body.get('_hp')).toBe('');
    expect(typeof init.body).toBe('string');
  });

  test('accepted goes to the next page, held too', async () => {
    for (const answer of [{ ok: true }, { ok: true, held: true }]) {
      const form = mount();
      fetchSpy.mockReturnValue(reply(200, answer));
      hydrateForm(form, '');
      await submit(form);
      expect(go).toHaveBeenLastCalledWith('/booking/received/');
    }
  });

  test('422 marks each named field invalid, ties its message, focuses the first, keeps values', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(422, { ok: false, error: 'invalid', fields: { email: 'format', your_name: 'required' } }));
    hydrateForm(form, '');
    await submit(form);
    const name = form.querySelector<HTMLInputElement>('[name="your_name"]')!;
    const email = form.querySelector<HTMLInputElement>('[name="email"]')!;
    expect(name.getAttribute('aria-invalid')).toBe('true');
    expect(email.getAttribute('aria-invalid')).toBe('true');
    expect(name.getAttribute('aria-describedby')).toBe('booking-your_name-help booking-your_name-error');
    expect(email.getAttribute('aria-describedby')).toBe('booking-email-error');
    const err = form.querySelector<HTMLElement>('#booking-email-error')!;
    expect(err.hidden).toBe(false);
    expect(err.textContent).toBe('Check this value.');
    expect(form.querySelector('#booking-your_name-error')!.textContent).toBe('This field is required.');
    expect(document.activeElement).toBe(name);
    expect(name.value).toBe('Ada Example');
    expect(go).not.toHaveBeenCalled();
    expect(form.closest('.moss-form')!.getAttribute('data-state')).toBe('error');
  });

  test('a refused radio group marks its buttons and focuses the first', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(422, { ok: false, error: 'invalid', fields: { session: 'required' } }));
    hydrateForm(form, '');
    await submit(form);
    const first = form.querySelector<HTMLInputElement>('#booking-session-0')!;
    expect(first.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(first);
  });

  test('typing in a refused field clears its mark', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(422, { ok: false, error: 'invalid', fields: { email: 'format' } }));
    hydrateForm(form, '');
    await submit(form);
    const email = form.querySelector<HTMLInputElement>('[name="email"]')!;
    email.dispatchEvent(new Event('input', { bubbles: true }));
    expect(email.hasAttribute('aria-invalid')).toBe(false);
    expect(form.querySelector<HTMLElement>('#booking-email-error')!.hidden).toBe(true);
  });

  test.each([
    [429, { ok: false, error: 'rate_limited' }, 'Too many requests.'],
    [404, { ok: false, error: 'unknown_form' }, 'Not available.'],
    [500, {}, 'Could not send.'],
  ])('%i writes one status line and keeps the answers', async (status, body, text) => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(status, body));
    hydrateForm(form, '');
    await submit(form);
    expect(form.querySelector('.moss-form__status')!.textContent).toBe(text);
    expect(form.querySelector<HTMLInputElement>('[name="your_name"]')!.value).toBe('Ada Example');
    expect(go).not.toHaveBeenCalled();
  });

  test('no connection writes the status line and keeps the answers', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(Promise.reject(new TypeError('offline')));
    hydrateForm(form, '');
    await submit(form);
    expect(form.querySelector('.moss-form__status')!.textContent).toBe('Could not send.');
    expect(form.closest('.moss-form')!.getAttribute('data-state')).toBe('error');
    expect(form.querySelector<HTMLInputElement>('[name="email"]')!.value).toBe('ada@example.test');
  });

  test('moss preview simulates loading, goes to next and posts nothing', async () => {
    vi.useFakeTimers();
    const form = mount();
    document.body.setAttribute('data-moss-preview', '');
    hydrateForm(form, '');
    form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
    vi.advanceTimersByTime(700);
    vi.useRealTimers();
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(go).toHaveBeenCalledWith('/booking/received/');
  });

  test('submitting twice while loading posts once', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(new Promise(() => {}));
    hydrateForm(form, '');
    await submit(form);
    await submit(form);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  test('hydrating twice binds one listener', async () => {
    const form = mount();
    fetchSpy.mockReturnValue(reply(200, { ok: true }));
    hydrateForm(form, '');
    hydrateForm(form, '');
    await submit(form);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });
});

describe('prefill from the address', () => {
  test('fills visible fields and choices', () => {
    const form = mount();
    prefill(form, '?your_name=Grace&note=Hello&colour=Blue&session=Family&news=on&unknown=x');
    expect(form.querySelector<HTMLInputElement>('[name="your_name"]')!.value).toBe('Grace');
    expect(form.querySelector<HTMLTextAreaElement>('[name="note"]')!.value).toBe('Hello');
    expect(form.querySelector<HTMLSelectElement>('[name="colour"]')!.value).toBe('Blue');
    expect(form.querySelector<HTMLInputElement>('#booking-session-1')!.checked).toBe(true);
    expect(form.querySelector<HTMLInputElement>('#booking-session-0')!.checked).toBe(false);
    expect(form.querySelector<HTMLInputElement>('#booking-news')!.checked).toBe(true);
  });

  test('never touches _next, _hp or any hidden input', () => {
    const form = mount();
    prefill(form, '?_hp=x&_next=/evil/');
    expect(form.querySelector<HTMLInputElement>('[name="_hp"]')!.value).toBe('');
    expect(form.querySelector<HTMLInputElement>('[name="_next"]')!.value).toBe('/booking/received/');
    expect(form.dataset.next).toBe('/booking/received/');
  });

  test('a hidden input of any other name is not filled either', () => {
    const form = mount();
    form.insertAdjacentHTML('beforeend', '<input type="hidden" name="source" value="page">');
    prefill(form, '?source=evil');
    expect(form.querySelector<HTMLInputElement>('[name="source"]')!.value).toBe('page');
  });

  test('a value that is no option of a select or radio is ignored', () => {
    const form = mount();
    prefill(form, '?colour=Green&session=Nobody');
    expect(form.querySelector<HTMLSelectElement>('[name="colour"]')!.value).toBe('');
    expect(form.querySelectorAll('input[name="session"]:checked').length).toBe(0);
  });
});
