/**
 * resend-control — the "Resend" button, with a cooldown and a visible outcome.
 *
 * Two surfaces ask a user to wait for a magic link (the first-publish modal
 * and the settings Deployment tab). Both must show what a Resend click did,
 * success or failure; this is the one implementation of that button.
 *
 * No spinner, no size change. The only moving pixel is a digit.
 */

import { t } from '../../i18n';

/**
 * Client-side resend cooldown.
 *
 * MARGIN CONSTRAINT — do NOT lower this. The publishing service enforces a
 * 25 s per-address cooldown, set 5 s UNDER this 30 s client timer. Anything
 * below 30 s here turns the ordinary Resend click into a 429.
 */
export const RESEND_COOLDOWN_MS = 30_000;

/** Feedback written into the consuming surface's live region. */
export interface ResendNotice {
  tone: 'calm' | 'error';
  text: string;
}

export interface ResendControlOptions {
  cooldownMs?: number;
  /**
   * Class list for the button. Defaults to a subtle text link.
   *
   * The first-publish surface overrides it to a real secondary button: when a
   * link expires, resending is the ONE thing left to do, and a text link
   * sitting at the same weight as "Use a different address" gives the user two
   * identical-looking choices where one is the answer.
   */
  className?: string;
  /** `left` reads right in a text link, `center` inside a button box. */
  textAlign?: 'left' | 'center';
  /** Idle label, e.g. t('publish.resend'). */
  label(): string;
  /** Counting-down label, e.g. t('publish.resend_in', { s }). */
  countdownLabel(sec: number): string;
  onResend(): Promise<{ ok: true } | { ok: false; error: string }>;
  /** Called with the outcome, or null to clear. */
  onStatus(notice: ResendNotice | null): void;
}

export interface ResendControl {
  el: HTMLButtonElement;
  /** Start (or restart) the cooldown. Call on EVERY successful send. */
  arm(): void;
  /**
   * End the cooldown now. For the one case where the surface knows more than
   * the timer does: the session expired (~300s in), so the 30s cooldown is
   * long past and the button must be usable the instant the user reads why.
   */
  enable(): void;
  destroy(): void;
}

/** How long a success notice stays before the surface reverts to its status. */
const NOTICE_LINGER_MS = 4000;

export function createResendControl(o: ResendControlOptions): ResendControl {
  const cooldownMs = o.cooldownMs ?? RESEND_COOLDOWN_MS;

  let tick: ReturnType<typeof setInterval> | null = null;
  let linger: ReturnType<typeof setTimeout> | null = null;
  let remaining = 0;
  let inFlight = false;
  let destroyed = false;

  const el = document.createElement('button');
  el.type = 'button';
  el.className = o.className ?? 'moss-link--subtle moss-resend-btn';
  el.setAttribute('data-action', 'resend');
  // The digit must never change the button's width, so nothing in the
  // surrounding surface reflows while it ticks.
  el.style.fontVariantNumeric = 'tabular-nums';
  el.style.textAlign = o.textAlign ?? 'left';
  el.textContent = o.label();

  /**
   * Reserve the width of the widest label this button will ever show, so the
   * cooldown digit changing width (font, locale, or the button's own padding)
   * never reflows the row around it.
   *
   * A `ch`-based guess drifts with the font and the label's script — a wider
   * font, or a CJK countdown string, doesn't fit the same character count as
   * the Latin one it was sized for. Measuring the actual rendered text is the
   * only reservation that stays correct across both. Every digit is the same
   * width under `tabular-nums`, so the countdown's widest form is always its
   * highest second count — no need to measure every tick.
   *
   * `getBoundingClientRect().width` is a border-box measurement (global
   * `box-sizing: border-box`), so a padded caller's own padding is already
   * included — nothing extra to add on top, unlike the old `ch` + padding
   * calc each caller had to hand-write.
   *
   * The element has to be connected to the document to measure anything
   * (`getBoundingClientRect()` is 0×0 on a detached node). Both callers
   * append `el` synchronously right after calling this factory, so the first
   * `requestAnimationFrame` after construction — which always runs before the
   * next paint — lands after that append.
   */
  function measureReservedWidth(): void {
    if (destroyed || !el.isConnected) return;
    const savedText = el.textContent;
    const savedMinWidth = el.style.minWidth;
    el.style.minWidth = '0';
    const maxCountdownSeconds = Math.ceil(cooldownMs / 1000);
    let widest = 0;
    for (const candidate of [o.label(), o.countdownLabel(maxCountdownSeconds)]) {
      el.textContent = candidate;
      widest = Math.max(widest, el.getBoundingClientRect().width);
    }
    el.style.minWidth = widest > 0 ? `${Math.ceil(widest)}px` : savedMinWidth;
    el.textContent = savedText;
  }
  requestAnimationFrame(measureReservedWidth);

  function clearTick(): void {
    if (tick !== null) {
      clearInterval(tick);
      tick = null;
    }
  }

  /**
   * Inert state is `aria-disabled` + the `blocked()` guard, NEVER the
   * `disabled` property.
   *
   * Per the HTML spec an element that becomes non-focusable while focused is
   * unfocused — activeElement falls to <body>. Clicking Resend focuses it on
   * the Linux and Windows shells, and the click ARMS the cooldown, so a real
   * `disabled` drops focus out of the surrounding surface on the user's own
   * action: out of the first-publish modal (past BaseModal's container-scoped
   * Tab trap) or out of the settings pill row.
   */
  function blocked(): boolean {
    return remaining > 0 || inFlight;
  }

  function paint(): void {
    if (blocked()) {
      el.setAttribute('aria-disabled', 'true');
    } else {
      el.removeAttribute('aria-disabled');
    }
    el.textContent = remaining > 0 ? o.countdownLabel(remaining) : o.label();
  }

  function arm(): void {
    if (destroyed) return;
    clearTick();
    remaining = Math.ceil(cooldownMs / 1000);
    paint();
    tick = setInterval(() => {
      remaining -= 1;
      if (remaining <= 0) {
        remaining = 0;
        clearTick();
      }
      paint();
    }, 1000);
  }

  el.addEventListener('click', () => {
    if (blocked()) return;
    // WebKit blurs a button on its own mousedown, even one focused a moment
    // earlier by JS — activeElement falls straight to <body>, outside the
    // modal, before this handler ever runs. Re-focusing the control's own
    // element (it never leaves the DOM) restores it: a `click` listener fires
    // after the browser's own mousedown/mouseup focus handling, so this runs
    // last and wins.
    el.focus();
    inFlight = true;
    paint();
    if (linger !== null) {
      clearTimeout(linger);
      linger = null;
    }
    void (async () => {
      const res = await o.onResend();
      inFlight = false;
      if (destroyed) return;
      if (res.ok) {
        o.onStatus({ tone: 'calm', text: t('publish.resend_sent') });
        linger = setTimeout(() => {
          linger = null;
          if (!destroyed) o.onStatus(null);
        }, NOTICE_LINGER_MS);
      } else {
        // A 429 means the server refused because it is STILL cooling. Re-arming
        // on failure is the honest state — the address's budget was consumed
        // either way, and an enabled button would just 429 again.
        o.onStatus({ tone: 'error', text: t('publish.resend_failed', { reason: res.error }) });
      }
      arm();
    })();
  });

  return {
    el,
    arm,
    enable() {
      if (destroyed) return;
      clearTick();
      remaining = 0;
      paint();
    },
    destroy() {
      destroyed = true;
      clearTick();
      if (linger !== null) {
        clearTimeout(linger);
        linger = null;
      }
    },
  };
}
