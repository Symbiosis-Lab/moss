/**
 * "Open on another device" offers a link only where another device can sign
 * in with it, an HTTPS origin, and otherwise says in one line what is missing.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { t } from '../../ui/i18n/index';
import { mountDevicePanel } from '../device-panel';

const fetchMock = vi.fn();

function mount(publicUrl: string | null): HTMLDialogElement {
  document.body.replaceChildren();
  const button = document.createElement('button');
  document.body.append(button);
  mountDevicePanel(button, { public_url: publicUrl });
  button.click();
  return document.querySelector('dialog')!;
}

describe('the "open on another device" panel', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
    // jsdom has no modal dialogs.
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) { this.open = true; };
  });
  afterEach(() => vi.unstubAllGlobals());

  it('without an HTTPS address says what is missing and mints nothing', async () => {
    const dialog = mount(null);
    await vi.waitFor(() => expect(dialog.textContent).toContain(t('device.needs_https')));
    expect(dialog.querySelector('img, input')).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('behind one shows a fresh link as a QR code and as text', async () => {
    const url = 'https://edit.example.test/__moss/sign-in#code=abc';
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ url, qr_svg: '<svg/>' })));
    const dialog = mount('https://edit.example.test');
    await vi.waitFor(() => expect(dialog.querySelector('input')?.value).toBe(url));
    expect(fetchMock).toHaveBeenCalledWith('/__moss/sign-in/new', { method: 'POST', credentials: 'same-origin' });
    expect(dialog.querySelector('img')!.src).toBe(`data:image/svg+xml;charset=utf-8,${encodeURIComponent('<svg/>')}`);
    expect(dialog.textContent).not.toContain(t('device.needs_https'));
  });

  it('says when the link could not be made', async () => {
    fetchMock.mockResolvedValue(new Response('', { status: 401 }));
    const dialog = mount('https://edit.example.test');
    await vi.waitFor(() => expect(dialog.textContent).toContain(t('device.mint_failed')));
  });
});
