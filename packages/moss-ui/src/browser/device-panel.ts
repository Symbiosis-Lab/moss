/**
 * "Open on another device": a fresh one-time sign-in link, as a QR code and
 * as text to copy. Another device's browser sends the headers the UI origin
 * requires only to an HTTPS address, so without one the panel says what is
 * missing instead of offering a link that cannot sign in.
 */
import { t } from '../ui/i18n/index';
import type { ServeConfig } from './shell-host';

interface MintedLink { url: string; qr_svg: string | null }

/** Each call retires the previous link, so the panel mints on every open. */
export async function mintLink(): Promise<MintedLink> {
  const res = await fetch('/__moss/sign-in/new', { method: 'POST', credentials: 'same-origin' });
  if (!res.ok) throw new Error(`moss: could not mint a sign-in link (${res.status})`);
  return (await res.json()) as MintedLink;
}

export function mountDevicePanel(button: HTMLElement | null, config: Pick<ServeConfig, 'public_url'>): void {
  if (!button) return;
  const dialog = document.createElement('dialog');
  dialog.className = 'moss-device-panel';
  dialog.addEventListener('click', (e) => {
    if (e.target === dialog) dialog.close();
  });
  document.body.append(dialog);
  button.addEventListener('click', () => void open(dialog, config));
}

async function open(dialog: HTMLDialogElement, config: Pick<ServeConfig, 'public_url'>): Promise<void> {
  const title = document.createElement('h2');
  title.textContent = t('device.open_on_another');
  dialog.replaceChildren(title);
  dialog.showModal();
  if (!config.public_url) {
    dialog.append(line(t('device.needs_https')));
    return;
  }
  let link: MintedLink;
  try {
    link = await mintLink();
  } catch (e) {
    console.warn('[device-panel]', e);
    dialog.append(line(t('device.mint_failed')));
    return;
  }
  if (link.qr_svg) {
    const qr = document.createElement('img');
    qr.className = 'moss-device-panel__qr';
    qr.alt = '';
    qr.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(link.qr_svg)}`;
    dialog.append(qr);
  }
  const text = document.createElement('input');
  text.readOnly = true;
  text.value = link.url;
  text.setAttribute('aria-label', t('device.link'));
  const copy = document.createElement('button');
  copy.type = 'button';
  copy.textContent = t('device.copy');
  copy.addEventListener('click', () => {
    void navigator.clipboard?.writeText(link.url).then(() => (copy.textContent = t('device.copied')));
  });
  dialog.append(line(t('device.scan')), text, copy);
}

function line(text: string): HTMLParagraphElement {
  const p = document.createElement('p');
  p.textContent = text;
  return p;
}
