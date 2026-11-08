/**
 * Iframe Message Handlers for Preview
 *
 * Handles postMessage events from the preview iframe:
 *
 * 1. open-external-link: Opens external URLs in system browser
 *    (Workaround: target="_blank" links inside iframes don't work on Tauri)
 *
 * 2. moss-open-in-system: Opens non-renderable files (PDFs, binaries) in the system
 *    browser by constructing a raw URL that bypasses the content wrapper.
 */

import { openExternal } from "../utils/open-external";
import { info, warn, error } from "../log";

/**
 * Message format for external links
 */
interface LinkMessage {
  type: 'open-external-link';
  url: string;
}

/**
 * Message format for opening files in system viewer
 */
interface OpenInSystemMessage {
  type: 'moss-open-in-system';
  path: string;
}

/**
 * Initialize the external link handler
 *
 * Sets up a message listener that receives link URLs from the preview iframe
 * and opens them in the system browser.
 *
 * @example
 * ```typescript
 * // In your main app initialization:
 * initializeLinkHandler();
 * ```
 */
export function initializeLinkHandler(): void {
  window.addEventListener('message', async (event) => {
    // Handle external link messages
    if (isLinkMessage(event.data)) {
      const { url } = event.data;

      // http(s)-only is enforced by openExternal itself; a refusal logs there.
      try {
        await openExternal(url);
      } catch (err) {
        error(`❌ [LinkHandler] Failed to open external link: ${url}, ${err}`);
      }
      return;
    }

    // Handle "open in system viewer" messages from content wrapper
    if (isOpenInSystemMessage(event.data)) {
      const { path } = event.data;
      const origin = event.origin;

      if (!origin || origin === 'null') {
        warn(`⚠️ [LinkHandler] Cannot open in system: no origin on message`);
        return;
      }

      // Validate path: must start with '/' and not contain query/fragment
      if (!path.startsWith('/') || path.includes('?') || path.includes('#')) {
        warn(`⚠️ [LinkHandler] Invalid path for open in system: ${path}`);
        return;
      }

      // Construct raw URL that bypasses the content wrapper
      const rawUrl = `${origin}${path}?__moss_raw=1`;
      info(`[LinkHandler] Opening in system browser: ${rawUrl}`);

      try {
        await openExternal(rawUrl);
      } catch (err) {
        error(`❌ [LinkHandler] Failed to open in system: ${rawUrl}, ${err}`);
      }
      return;
    }
  });
}

/**
 * Type guard to check if message is a valid link message
 */
function isLinkMessage(data: unknown): data is LinkMessage {
  return (
    typeof data === 'object' &&
    data !== null &&
    'type' in data &&
    data.type === 'open-external-link' &&
    'url' in data &&
    typeof data.url === 'string'
  );
}

/**
 * Type guard to check if message is a valid open-in-system message
 */
function isOpenInSystemMessage(data: unknown): data is OpenInSystemMessage {
  return (
    typeof data === 'object' &&
    data !== null &&
    'type' in data &&
    data.type === 'moss-open-in-system' &&
    'path' in data &&
    typeof data.path === 'string'
  );
}
