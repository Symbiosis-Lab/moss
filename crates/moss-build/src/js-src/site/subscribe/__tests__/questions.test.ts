import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { hydrateSubscribeForm } from '../subscribe.js';

// The markup the build writes for a subscribe block that asks questions.
function mount(): HTMLFormElement {
  document.documentElement.lang = 'zh-Hant';
  document.body.innerHTML = `
    <div class="moss-subscribe moss-form">
      <form class="moss-subscribe-form" data-position="stacked" data-moss-hosted="true" data-state="idle"
            method="post" action="https://api.example.com/api/sites/my-site/subscribe">
        <input type="hidden" name="scope" value="">
        <div class="moss-form__field" data-field="email">
          <label class="moss-form__label" for="f-email">電郵</label>
          <input class="moss-input" type="email" id="f-email" name="email" required>
        </div>
        <div class="moss-form__field" data-field="name">
          <label class="moss-form__label" for="f-name">怎麼稱呼你</label>
          <input class="moss-input" type="text" id="f-name" name="name" required>
        </div>
        <fieldset class="moss-form__field moss-form__group" data-field="source" data-required="true">
          <legend>從哪裡知道</legend>
          <div class="moss-form__choice"><input type="checkbox" id="f-s0" name="source" value="Facebook"><label for="f-s0">Facebook</label></div>
          <div class="moss-form__choice"><input type="checkbox" id="f-s1" name="source" value="微信群"><label for="f-s1">微信群</label></div>
          <div class="moss-form__choice"><input type="checkbox" id="f-s2" name="source" value="其他"><label for="f-s2">其他</label></div>
          <div class="moss-form__other"><label for="f-s2-other">其他管道</label><input class="moss-input" type="text" id="f-s2-other" name="source_other"></div>
        </fieldset>
        <fieldset class="moss-form__field moss-form__group" data-field="city">
          <legend>城市</legend>
          <div class="moss-form__choice"><input type="radio" id="f-c0" name="city" value="台北"><label for="f-c0">台北</label></div>
          <div class="moss-form__choice"><input type="radio" id="f-c1" name="city" value="其他"><label for="f-c1">其他</label></div>
          <div class="moss-form__other"><label for="f-c1-other">哪裡</label><input class="moss-input" type="text" id="f-c1-other" name="city_other"></div>
        </fieldset>
        <span class="moss-btn-slot"><button type="submit" class="moss-btn"><span class="moss-btn__label"></span></button></span>
        <p class="moss-form__status"></p>
        <div class="moss-subscribe-status" aria-live="polite" tabindex="-1"><span data-copy="check-email" hidden></span><span data-copy="error" hidden></span></div>
      </form>
    </div>`;
  return document.querySelector('form')!;
}

let fetchSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));
});
afterEach(() => {
  fetchSpy.mockRestore();
  document.body.innerHTML = '';
});

const set = (form: HTMLFormElement, id: string, value: string | boolean) => {
  const input = form.querySelector<HTMLInputElement>(`#${id}`)!;
  if (typeof value === 'boolean') input.checked = value;
  else input.value = value;
};

test('sends the answers beside the address: picks as a list, an Other box only while its choice is picked', async () => {
  const form = mount();
  hydrateSubscribeForm(form);
  set(form, 'f-email', 'reader@example.com');
  set(form, 'f-name', ' 小明 ');
  set(form, 'f-s1', true);
  set(form, 'f-s2', true);
  set(form, 'f-s2-other', '朋友');
  set(form, 'f-c1-other', 'typed, then its choice left unpicked');
  form.requestSubmit();
  await vi.waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1));
  const body = JSON.parse((fetchSpy.mock.calls[0][1] as RequestInit).body as string);
  expect(body.email).toBe('reader@example.com');
  expect(body.answers).toEqual({ name: '小明', source: ['微信群', '其他'], source_other: '朋友' });
});

test('a form without questions sends no answers key', async () => {
  const form = mount();
  document.querySelector('.moss-subscribe')!.classList.remove('moss-form');
  form.querySelectorAll('[data-field]:not([data-field="email"])').forEach((f) => f.remove());
  hydrateSubscribeForm(form);
  set(form, 'f-email', 'reader@example.com');
  form.requestSubmit();
  await vi.waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1));
  expect(JSON.parse((fetchSpy.mock.calls[0][1] as RequestInit).body as string)).not.toHaveProperty('answers');
});

test('on success the status takes the focus, and the one-line form keeps it on its button', async () => {
  fetchSpy.mockImplementation(async () => new Response(JSON.stringify({ subscribed: true, confirmationSent: true }), { status: 200 }));
  for (const asks of [true, false]) {
    const form = mount();
    if (!asks) {
      document.querySelector('.moss-subscribe')!.classList.remove('moss-form');
      form.querySelectorAll('[data-field]:not([data-field="email"])').forEach((f) => f.remove());
    }
    hydrateSubscribeForm(form);
    set(form, 'f-email', 'reader@example.com');
    if (asks) { set(form, 'f-name', '小明'); set(form, 'f-s1', true); }
    const button = form.querySelector<HTMLButtonElement>('.moss-btn')!;
    button.focus();
    form.requestSubmit();
    await vi.waitFor(() => expect(form.dataset.state).toBe('success'));
    expect(document.activeElement).toBe(asks ? form.querySelector('.moss-subscribe-status') : button);
  }
});
