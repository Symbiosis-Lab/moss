/**
 * The browser's host record: a frame on the UI origin `moss edit --serve`
 * serves, reaching the engine over its HTTP carrier. It fills only what the
 * carrier and the page provide; what a browser lacks (a file manager, native
 * windows, paths outside the folder) is left out, and the UI shows no control
 * for it. Each frame fills it before the UI's first module reads it.
 */
import { fillHost, type Bus, type UiHost } from '../ui/host';
import * as carrier from './carrier';
import { pickAndUpload } from './upload';

/** Rejects when the open did not happen: a blocked popup is a real failure. */
async function openInTab(url: string): Promise<void> {
  const opened = window.open(url, '_blank', 'noopener,noreferrer');
  if (!opened) throw new Error(`moss: the browser blocked opening ${url}`);
}

/** The members every browser frame has. */
export function browserHost(bus: Bus): UiHost {
  return {
    engine: { call: carrier.call, on: carrier.on },
    bus,
    // The UI origin mounts the folder's sources here.
    sourceAssetBase: '/__moss/source',
    openExternal: openInTab,
    pickFiles: pickAndUpload,
  };
}

/** Fills the record with the browser's members and the frame's own. */
export function fillBrowserHost(bus: Bus, own: Partial<UiHost> = {}): void {
  fillHost({ ...browserHost(bus), ...own });
}
