/**
 * Opening a link outside moss, and the toast runtime that opens toast links.
 *
 * The one http(s) gate, for the UI and the desktop's own surfaces alike: no
 * caller wants another scheme, and a page's link or a plugin's help URL must
 * never reach a host's opener as `file:`, `javascript:` or `command:`.
 */
import { host } from '../host';
import { error, info, warn } from '../log';
import { isHttpUrl } from '../shared/url-scheme';
import { configureToastRuntime } from '../shared/ui/toast/toast-runtime';

/**
 * Opens an http(s) `url` where the person can see it. Anything else is
 * refused and logged, not thrown: most callers fire this as `void`. Rejects
 * when an allowed open did not happen (a blocked popup).
 */
export async function openExternal(url: string): Promise<void> {
  if (!isHttpUrl(url)) {
    warn(`moss: refusing to open a non-http(s) URL: ${url}`);
    return;
  }
  return host.openExternal(url);
}

/** Installs the toasts' log sinks and opener; each entry that mounts toasts calls it once, before the first renders. */
export function installHostToastRuntime(): void {
  configureToastRuntime({ logError: error, logInfo: info, logUiEvent: info, openUrl: openExternal });
}
