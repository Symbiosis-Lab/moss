import { langBucket, type Lang } from "./i18n";

/** Names the modal for screen readers while the check runs. */
export const VERIFYING: Record<Lang, string> = {
  en: "Verifying…",
  de: "Wird überprüft …",
  "zh-hans": "验证中…",
  "zh-hant": "驗證中…",
};

/**
 * The bot check a hosted service asks for before it accepts a post. The
 * service's captcha page (`pageUrl`, ending in a slash) loads in a hidden
 * frame: on the service's own origin it renders Turnstile and verifies the
 * token itself, so moss never sees the site key or the token, and clears the
 * visitor's IP. `quiet=1` asks the page to render the widget only when
 * Turnstile needs the visitor; it then tells this window `interactive`, the
 * only time the frame shows, as a modal, and later `passed` or `failed`.
 *
 * Resolves true on `passed`, false when the visitor closes the modal (backdrop
 * click, or Escape in this page or in the frame). Rejects on `failed`, after
 * `QUIET_MS` with nothing from the page before `interactive` (Turnstile never
 * loaded or never answered), and two minutes after `interactive`. An expired
 * check can return `retry`: the caller must repeat its original request and
 * handle a new challenge URL, without treating the old check as clearance.
 */
export const QUIET_MS = 20000;
const OPEN_MS = 120000;

export function solveCaptcha(pageUrl: string): Promise<boolean | "retry"> {
  const label = VERIFYING[langBucket(document.documentElement.lang)];
  return new Promise<boolean | "retry">((resolve, reject) => {
    const overlay = document.createElement("div");
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-label", label);
    overlay.style.cssText =
      "position:fixed;inset:0;z-index:2147483646;display:flex;align-items:center;" +
      "justify-content:center;background:rgba(0,0,0,.5);visibility:hidden";
    const frame = document.createElement("iframe");
    frame.title = label;
    frame.src = pageUrl + "?quiet=1&t=" + Date.now();
    frame.style.cssText =
      "width:340px;max-width:92vw;height:240px;border:0;border-radius:12px;" +
      "background:var(--moss-color-surface,#fff);box-shadow:var(--moss-elevation-3)";
    overlay.appendChild(frame);
    document.body.appendChild(overlay);
    const returnFocus = document.activeElement as HTMLElement | null;

    // Removes every listener and the timer, so it runs at most once.
    const finish = (act: () => void): void => {
      window.clearTimeout(timer);
      window.removeEventListener("message", onMessage);
      overlay.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
      const hadFocus = overlay.contains(document.activeElement);
      overlay.remove();
      if (hadFocus) returnFocus?.focus();
      act();
    };
    const shown = (): boolean => overlay.style.visibility !== "hidden";
    const cancel = (): void => finish(() => resolve(false));
    const onClick = (ev: Event): void => {
      if (ev.target === overlay) cancel();
    };
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key === "Escape" && shown()) cancel();
    };
    const onMessage = (ev: MessageEvent): void => {
      if (ev.source !== frame.contentWindow) return;
      const data = ev.data as { moss_captcha?: string; retry?: boolean } | null;
      const state = data?.moss_captcha;
      if (state === "interactive") {
        // A puzzle the visitor is solving gets the long wait.
        window.clearTimeout(timer);
        timer = giveUpAfter(OPEN_MS);
        // A modal takes focus while open and gives it back when it closes.
        overlay.style.visibility = "";
        frame.focus();
      } else if (state === "passed") finish(() => resolve(true));
      else if (state === "failed") finish(() => reject(new Error("captcha failed")));
      // Escape pressed inside the frame, where this window cannot hear it.
      else if (state === "cancelled") {
        if (data?.retry === true) finish(() => resolve("retry"));
        else cancel();
      }
    };
    window.addEventListener("message", onMessage);
    overlay.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    const giveUpAfter = (ms: number): number =>
      window.setTimeout(() => finish(() => reject(new Error("captcha timeout"))), ms);
    let timer = giveUpAfter(QUIET_MS);
  });
}

/** Background measurement never opens a modal or changes the subscription result. */
export function observeCaptcha(pageUrl: string): void {
  try {
    const url = new URL(pageUrl, location.href);
    if (url.protocol !== "https:") return;
    const frame = document.createElement("iframe");
    frame.hidden = true;
    frame.tabIndex = -1;
    frame.setAttribute("aria-hidden", "true");
    frame.dataset.mossCaptchaObservation = "";
    frame.src = url.href;
    const finish = (): void => {
      window.clearTimeout(timer);
      window.removeEventListener("message", onMessage);
      frame.remove();
    };
    const onMessage = (ev: MessageEvent): void => {
      if (ev.source === frame.contentWindow && ev.origin === url.origin && ev.data?.moss_captcha === "observed") finish();
    };
    const timer = window.setTimeout(finish, 30000);
    window.addEventListener("message", onMessage);
    frame.addEventListener("error", finish, { once: true });
    document.body.appendChild(frame);
  } catch { /* Observation cannot change a successful form result. */ }
}

/** The API-origin frame warms a token without taking focus or delaying a press. */
export function prewarmCaptcha(form: HTMLFormElement): () => { turnstile_started: boolean; turnstile_token?: string } {
  let frame: HTMLIFrameElement | undefined;
  let token: { value: string; at: number } | undefined;
  let started = false;
  let disposed = false;
  let observer: IntersectionObserver | undefined;
  let timer: number | undefined;
  const action = new URL(form.action, location.href);
  const site = action.pathname.match(/\/sites\/([a-z0-9][a-z0-9-]{0,62})\/subscribe$/)?.[1];
  const url = new URL(`/captcha/subscription-prewarm/${site}/`, action.origin);
  url.searchParams.set('origin', location.origin);
  const stop = (): void => {
    disposed = true;
    observer?.disconnect();
    window.clearTimeout(timer);
    form.removeEventListener('focusin', start);
    window.removeEventListener('message', message);
    window.removeEventListener('pagehide', stop);
    frame?.remove();
  };
  const message = (event: MessageEvent): void => {
    if (!frame || event.source !== frame.contentWindow || event.origin !== action.origin) return;
    if (!form.isConnected) { stop(); return; }
    if (event.data?.moss_prewarm === 'started') started = true;
    if (event.data?.moss_prewarm === 'expired') token = undefined;
    if (event.data?.moss_prewarm === 'ready' && typeof event.data.token === 'string' && event.data.token.length <= 2048) {
      started = true;
      token = { value: event.data.token, at: Date.now() };
    }
  };
  const start = (): void => {
    if (frame || disposed || !site || action.protocol !== 'https:') return;
    observer?.disconnect();
    frame = document.createElement('iframe');
    frame.hidden = true;
    frame.tabIndex = -1;
    frame.setAttribute('aria-hidden', 'true');
    frame.title = 'Subscription check';
    frame.dataset.mossCaptchaPrewarm = '';
    frame.src = url.href;
    window.addEventListener('message', message);
    window.addEventListener('pagehide', stop);
    document.body.appendChild(frame);
    timer = window.setTimeout(stop, 300_000);
  };
  form.addEventListener('focusin', start);
  if (typeof IntersectionObserver !== 'undefined') {
    observer = new IntersectionObserver(entries => { if (entries.some(e => e.isIntersecting)) start(); });
    observer.observe(form);
  }
  return () => {
    const ready = token && Date.now() - token.at < 240_000 ? token.value : undefined;
    token = undefined;
    // Tokens are single use. Never replay one on an enforcing resubmission.
    if (ready) stop();
    return { turnstile_started: started, ...(ready ? { turnstile_token: ready } : {}) };
  };
}
