import { t } from '../../i18n';

export interface CopyButtonOptions {
  /** The text to write to the clipboard when the button is clicked. */
  value: string;
  /**
   * Optional label text shown next to the icon.
   * - When omitted (undefined): renders a label span with the default `t('publish.copy')` text.
   * - When set to `''` (empty string): icon-only mode -- no label span is rendered.
   *   In this case you MUST pass `ariaLabel` for screen-reader accessibility.
   * - When set to any other string: renders a label span with that text.
   */
  label?: string;
  /** Accessible label for the button. Defaults to `ariaLabel ?? label ?? t('publish.copy')`. */
  ariaLabel?: string;
  /** Called once after a successful clipboard write (before the 2-second revert). */
  onCopied?: () => void;
}

/**
 * Creates the ONE canonical "copy" button used everywhere in moss.
 *
 * Returns a `<button type="button" class="moss-copy-btn">` with:
 * - A copy icon (`.moss-copy-btn__icon--copy`)
 * - A check icon (`.moss-copy-btn__icon--check`), hidden by default via CSS
 * - An optional label span (`.moss-copy-btn__label`)
 *
 * On click: writes `opts.value` to the clipboard, enters the `--copied` state
 * for 2000ms, then reverts. Rapid clicks re-arm the timer from scratch.
 */
export function createCopyButton(opts: CopyButtonOptions): HTMLButtonElement {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'moss-copy-btn';

  // Resolve initial aria-label
  const defaultLabel = opts.label ?? t('publish.copy');
  const resolvedAriaLabel = opts.ariaLabel ?? (opts.label !== '' ? defaultLabel : t('publish.copy'));
  btn.setAttribute('aria-label', resolvedAriaLabel);

  // Icon-only mode: add data-tooltip so the moss portal tooltip fires on hover
  // Tooltips use data-tooltip, not title=; aria-label is for screen readers only.
  const hasTooltip = opts.label === '';
  const restingTooltip = opts.ariaLabel ?? t('publish.copy');
  if (hasTooltip) {
    btn.setAttribute('data-tooltip', restingTooltip);
    // Top: the icon button trails a horizontal command row, and its other
    // host — the toast — is anchored to the bottom edge, where below has no room.
    btn.setAttribute('data-tooltip-position', 'top');
  }

  // Copy icon: two offset rounded rects that read as "copy"
  const copyIcon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  copyIcon.setAttribute('width', '14');
  copyIcon.setAttribute('height', '14');
  copyIcon.setAttribute('viewBox', '0 0 14 14');
  copyIcon.setAttribute('fill', 'none');
  copyIcon.setAttribute('stroke', 'currentColor');
  copyIcon.setAttribute('stroke-width', '1.5');
  copyIcon.setAttribute('stroke-linecap', 'round');
  copyIcon.setAttribute('stroke-linejoin', 'round');
  copyIcon.setAttribute('aria-hidden', 'true');
  copyIcon.setAttribute('class', 'moss-copy-btn__icon--copy');

  // Both rects are 9x9 and inset by 0.5 from the viewBox edges so their 1.5px
  // strokes do not clip at the 14x14 boundary (back rect at 0.5, front at 4.5).
  const rect1 = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  rect1.setAttribute('x', '0.5');
  rect1.setAttribute('y', '0.5');
  rect1.setAttribute('width', '9');
  rect1.setAttribute('height', '9');
  rect1.setAttribute('rx', '2');

  const rect2 = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  rect2.setAttribute('x', '4.5');
  rect2.setAttribute('y', '4.5');
  rect2.setAttribute('width', '9');
  rect2.setAttribute('height', '9');
  rect2.setAttribute('rx', '2');

  copyIcon.appendChild(rect1);
  copyIcon.appendChild(rect2);

  // Check icon: a checkmark path, hidden by default via CSS (.moss-copy-btn__icon--check)
  const checkIcon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  checkIcon.setAttribute('width', '14');
  checkIcon.setAttribute('height', '14');
  checkIcon.setAttribute('viewBox', '0 0 14 14');
  checkIcon.setAttribute('fill', 'none');
  checkIcon.setAttribute('stroke', 'currentColor');
  checkIcon.setAttribute('stroke-width', '1.5');
  checkIcon.setAttribute('stroke-linecap', 'round');
  checkIcon.setAttribute('stroke-linejoin', 'round');
  checkIcon.setAttribute('aria-hidden', 'true');
  checkIcon.setAttribute('class', 'moss-copy-btn__icon--check');

  const checkPath = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  checkPath.setAttribute('d', 'M2 7l4 4 6-6');
  checkIcon.appendChild(checkPath);

  btn.appendChild(copyIcon);
  btn.appendChild(checkIcon);

  // Optional label span
  let labelSpan: HTMLSpanElement | null = null;
  if (opts.label !== '') {
    labelSpan = document.createElement('span');
    labelSpan.className = 'moss-copy-btn__label';
    labelSpan.textContent = opts.label ?? t('publish.copy');
    btn.appendChild(labelSpan);
  }

  // Click handler with re-armable timer
  let revertTimer: ReturnType<typeof setTimeout> | null = null;

  btn.addEventListener('click', () => {
    navigator.clipboard.writeText(opts.value).then(() => {
      // Enter copied state
      btn.classList.add('moss-copy-btn--copied');
      btn.setAttribute('aria-label', t('publish.copied'));
      if (hasTooltip) {
        btn.setAttribute('data-tooltip', t('publish.copied'));
      }
      if (labelSpan) {
        labelSpan.textContent = t('publish.copied');
      }

      opts.onCopied?.();

      // Re-arm: clear any previous timer before setting a new one
      if (revertTimer !== null) {
        clearTimeout(revertTimer);
      }
      revertTimer = setTimeout(() => {
        revertTimer = null;
        btn.classList.remove('moss-copy-btn--copied');
        btn.setAttribute('aria-label', resolvedAriaLabel);
        if (hasTooltip) {
          btn.setAttribute('data-tooltip', restingTooltip);
        }
        if (labelSpan) {
          labelSpan.textContent = opts.label ?? t('publish.copy');
        }
      }, 2000);
    }).catch(() => {
      // Clipboard failure: stay idle, no state change, no crash
    });
  });

  return btn;
}

/**
 * A mono `<code>` line with the canonical copy button beside it — the app's
 * one symbol for "a value you'll paste elsewhere" (onion addresses, wp-admin
 * credentials). Lives here (not a new file) beside the button it composes.
 *
 * `copyValue` covers display ≠ copy (the publish success screen shows the
 * bare host but copies the full URL). Omit `copyAriaLabel` to render the
 * code line without a copy button (a username needs no clipboard).
 */
export function createCodeWithCopy(opts: {
  text: string;
  /** `data-slot` on the `<code>` so tests/tools can address the value. */
  slot: string;
  copyAriaLabel?: string;
  copyValue?: string;
}): HTMLParagraphElement {
  const line = document.createElement('p');
  line.className = 'moss-copy-line';
  const code = document.createElement('code');
  code.setAttribute('data-slot', opts.slot);
  code.textContent = opts.text;
  line.appendChild(code);
  if (opts.copyAriaLabel !== undefined) {
    line.appendChild(
      createCopyButton({ value: opts.copyValue ?? opts.text, label: '', ariaLabel: opts.copyAriaLabel }),
    );
  }
  return line;
}
