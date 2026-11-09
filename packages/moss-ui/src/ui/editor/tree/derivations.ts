/**
 * The vault's folder derivations: which folders hold files a plugin can show as
 * pages, and what the author decided for each. Read on the tree's refresh
 * (`TreeRefresh.refreshTree`, so the watcher, an import and a mount all reach
 * it) and never per render, because `derivationState` walks the site.
 *
 * The state is the only source of the hint row and the folder menu items: a
 * click calls `set`, which writes the decision, reads the state back, and lets
 * the row disappear because the state says so — nothing hides it by hand.
 */

import type { EngineResult, FolderDerivation, FolderDerivationChoice } from '@symbiosis-lab/moss-ui';
import { t } from "../../i18n/index";
import { warn as logWarn } from "../../log";
import { PROJECT_ROOT_REL } from '../path-domain';
import { DERIVATIONS_REFRESHED } from '../expand-derived/plan';

export interface DerivationCommands {
  derivationState(): Promise<EngineResult<FolderDerivation[]>>;
  setFolderDerivation(folder: string, choice: FolderDerivationChoice): Promise<EngineResult<string[]>>;
}

export interface DerivationHost {
  projectRoot(): string;
  readonly commands: DerivationCommands;
  /** Repaint after the state changed. */
  render(): void;
}

/** One unreadable file and why, in the order the Why surface lists them. */
export interface DerivationProblem {
  file: string;
  why: string;
}

/** The registry path (root `''`, no trailing slash) a backend `folder` names. */
function toRegistryPath(folder: string): string {
  return folder === '/' ? PROJECT_ROOT_REL : folder.replace(/\/$/, '').normalize('NFC');
}

/** The backend `folder` for a registry path. */
function toBackendFolder(path: string): string {
  return path === PROJECT_ROOT_REL ? '/' : `${path}/`;
}

/** Files a deriver skipped or failed on, skipped first. */
export function problemsOf(d: FolderDerivation): DerivationProblem[] {
  return [
    ...d.skipped.map((s) => ({ file: s.file, why: s.reason })),
    ...d.failed.map((f) => ({ file: f.file, why: f.message })),
  ];
}

/**
 * The hint row of a folder with a derivation to offer (undecided) or a problem
 * to report (shown, with files a deriver could not read): its first line,
 * indented to the CHILDREN's column plus the icon box, so its text starts
 * after the sibling rail and lines up with the file names beneath it.
 *
 * A state row, not a `.seg`: no path identity, never selected, dragged or
 * renamed, and out of aria-posinset/setsize. Its two actions are plain
 * buttons the controller's delegated click handler resolves from
 * `data-derive-action`; like every row, it carries no listener of its own.
 * The nouns come from the plugin through `d`, so core has no format copy.
 */
export function mkHintRow(d: FolderDerivation, path: string, depth: number, offsetPx: number): HTMLElement {
  const el = document.createElement('div');
  el.className = 'nav-hint-row';
  el.style.setProperty('--ft-offset', `${offsetPx}px`);
  el.setAttribute('data-depth', String(depth));
  el.setAttribute('data-state-row', 'derivation');
  el.dataset.derivePath = path;

  const label = document.createElement('span');
  label.className = 'nav-hint-label';
  const sep = (): HTMLElement => {
    const s = document.createElement('span');
    s.className = 'nav-hint-sep';
    s.textContent = '·';
    return s;
  };
  const action = (name: 'show' | 'keep' | 'why', text: string, subtle: boolean): HTMLButtonElement => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = subtle ? 'moss-link moss-link--subtle' : 'moss-link';
    b.dataset.deriveAction = name;
    b.textContent = text;
    return b;
  };

  const actions = document.createElement('span');
  actions.className = 'nav-hint-actions';
  if (d.decision === 'undecided') {
    label.textContent = t('tree.derive_count', { count: d.count, noun: d.count === 1 ? d.noun_one : d.noun_other });
    // The separator stays with the first action, so a wrapped line never opens with it.
    const first = document.createElement('span');
    first.className = 'nav-hint-nowrap';
    first.append(action('show', t('tree.derive_show'), false), sep());
    actions.append(first, action('keep', t('tree.derive_keep'), false));
  } else {
    const n = problemsOf(d).length;
    el.classList.add('nav-hint-row--failed');
    label.textContent = t('tree.derive_unreadable', { count: n, noun: n === 1 ? d.noun_one : d.noun_other });
    actions.append(action('why', t('tree.derive_why'), true));
  }
  // The separator stays with the label, so a wrapped action line never opens with it.
  label.append(' ', sep());
  el.append(label, actions);
  return el;
}

export class TreeDerivations {
  private byFolder = new Map<string, FolderDerivation>();

  constructor(private readonly host: DerivationHost) {}

  /** Re-read the state. A failed read keeps the last good state. */
  async refresh(): Promise<void> {
    if (!this.host.projectRoot()) return;
    let result: EngineResult<FolderDerivation[]>;
    try {
      result = await this.host.commands.derivationState();
    } catch {
      return;
    }
    if (result.status !== 'ok') return;
    this.byFolder = new Map(result.data.map((d) => [toRegistryPath(d.folder), d]));
    // The editor re-asks which derived embeds are ready (expand-derived/extension.ts).
    document.dispatchEvent(new CustomEvent(DERIVATIONS_REFRESHED));
  }

  /** The derivation of the folder at registry path `path`, or null when none applies. */
  at(path: string): FolderDerivation | null {
    return this.byFolder.get(path.normalize('NFC')) ?? null;
  }

  /** Folders offering the undecided hint row: no decision yet and files to show. */
  hasHint(path: string): boolean {
    const d = this.at(path);
    if (d === null) return false;
    if (d.decision === 'undecided') return d.count > 0;
    return d.decision === 'show' && problemsOf(d).length > 0;
  }

  /** Write the author's choice for the folder, then read the state back and repaint. */
  async set(path: string, choice: FolderDerivationChoice): Promise<void> {
    try {
      const r = await this.host.commands.setFolderDerivation(toBackendFolder(path), choice);
      if (r.status === 'error') void logWarn(`[tree] setFolderDerivation(${path}): ${r.error}`).catch(() => {});
    } catch (err) {
      void logWarn(`[tree] setFolderDerivation(${path}): ${String(err)}`).catch(() => {});
    }
    await this.refresh();
    this.host.render();
  }
}
