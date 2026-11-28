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
 * only time the frame shows, as a modal, and later `passed`.
 *
 * A person always gets through: when the check cannot finish (the page says
 * `failed`, says nothing within `QUIET_MS`, or nothing within `OPEN_MS` of
 * `interactive`, as when an in-app browser blocks the frame), this reports it
 * to the page's `report`, which lets the visitor's next request through, and
 * resolves true. Resolves false only when the visitor closes the modal
 * (backdrop click, or Escape in this page or in the frame).
 */
export const QUIET_MS = 20000;
export const OPEN_MS = 60000;

export function solveCaptcha(pageUrl: string): Promise<boolean> {
  const label = VERIFYING[langBucket(document.documentElement.lang)];
  return new Promise<boolean>((resolve) => {
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
    // A no-cors form post: the service hears it from any page, and its answer is not needed.
    const failOpen = (): void => finish(() => {
      fetch(pageUrl + "report", { method: "POST", mode: "no-cors", body: new URLSearchParams({ reason: "frame" }) })
        .catch(() => {})
        .then(() => resolve(true));
    });
    const onClick = (ev: Event): void => {
      if (ev.target === overlay) cancel();
    };
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key === "Escape" && shown()) cancel();
    };
    const onMessage = (ev: MessageEvent): void => {
      if (ev.source !== frame.contentWindow) return;
      const state = (ev.data as { moss_captcha?: string } | null)?.moss_captcha;
      if (state === "interactive") {
        // A puzzle the visitor is solving gets the long wait.
        window.clearTimeout(timer);
        timer = window.setTimeout(failOpen, OPEN_MS);
        // A modal takes focus while open and gives it back when it closes.
        overlay.style.visibility = "";
        frame.focus();
      } else if (state === "passed") finish(() => resolve(true));
      else if (state === "failed") failOpen();
      // Escape pressed inside the frame, where this window cannot hear it.
      else if (state === "cancelled") cancel();
    };
    window.addEventListener("message", onMessage);
    overlay.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    let timer = window.setTimeout(failOpen, QUIET_MS);
  });
}
