/**
 * Files with a special role on a site, and whether the user has asked not to
 * be told about one again. The kind comes from moss (`TreeNode.special`); the
 * app decides nothing by file name.
 *
 * "Don't ask again" is per kind and for the whole app, kept in localStorage
 * like the editor font size: the editor and shell webviews share one origin.
 */

/** Mirrors the bindings' `SpecialFileKind` (a test pins the two together). */
export type SpecialFileKind = 'agent-instructions' | 'welcome-email' | 'footer' | 'not-found' | 'page-settings';

const MUTE_KEY_PREFIX = 'moss.specialFile.skipConfirm.';

export function isMuted(kind: SpecialFileKind): boolean {
  try {
    return localStorage.getItem(MUTE_KEY_PREFIX + kind) === '1';
  } catch {
    return false;
  }
}

export function mute(kind: SpecialFileKind): void {
  try {
    localStorage.setItem(MUTE_KEY_PREFIX + kind, '1');
  } catch {
    // Unremembered, the dialog comes back next time; nothing is lost.
  }
}

export interface SpecialFileAnswer {
  open: boolean;
  dontAskAgain: boolean;
}

/**
 * May this file open? Plain files and kinds the user muted open at once;
 * otherwise `ask` shows the dialog. Only an Open answer can mute a kind.
 */
export async function mayOpenSpecialFile(
  kind: SpecialFileKind | null | undefined,
  ask: (kind: SpecialFileKind) => Promise<SpecialFileAnswer>,
): Promise<boolean> {
  if (!kind || isMuted(kind)) return true;
  const answer = await ask(kind);
  if (!answer.open) return false;
  if (answer.dontAskAgain) mute(kind);
  return true;
}
