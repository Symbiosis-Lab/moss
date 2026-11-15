/**
 * Whether this browser is signed in to the site, as the site's host says. The
 * host sets a readable hint cookie beside its session, so with no hint nothing
 * is asked and a visitor who never subscribed costs no request. While signed in
 * the root carries `data-moss-subscribed`, which moss's CSS and a theme read,
 * and each change is announced as `moss:subscribed` on `document`.
 */

const HINT = '__Host-moss_subscribed=1';
const SESSION = '/_moss/account/session';

const hinted = (): boolean => document.cookie.split('; ').includes(HINT);

function show(subscribed: boolean): void {
  const root = document.documentElement;
  if (root.hasAttribute('data-moss-subscribed') === subscribed) return;
  root.toggleAttribute('data-moss-subscribed', subscribed);
  document.dispatchEvent(new CustomEvent('moss:subscribed', { detail: { subscribed } }));
}

let asking: Promise<void> | null = null;

/**
 * Shows the hint at once, then asks the host. Only a 401 takes "Subscribed"
 * back (the host clears the hint with it); an outage keeps what the hint said.
 */
export function syncReader(): Promise<void> {
  if (!hinted()) {
    show(false);
    return Promise.resolve();
  }
  show(true);
  asking ??= fetch(SESSION, { headers: { Accept: 'application/json' } })
    .then(
      (res) => {
        if (res.status === 401) show(false);
      },
      () => {},
    )
    .finally(() => {
      asking = null;
    });
  return asking;
}

/** Runs before the body paints, and again when a tab comes back, as after the link signed in another tab. */
export function watchReader(): void {
  if (typeof document === 'undefined') return;
  void syncReader();
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') void syncReader();
  });
}
