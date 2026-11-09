// Term navigation for the chip bar (cmd+click on a name chip) and vault-term
// completion. Design rule: a term page always exists — claimed → open the
// claiming page's source (same gesture as a wikilink), unclaimed → the MAIN
// preview navigates to the generated URL, which doubles as discovery of what
// readers see.
//
// Which fields carry terms is a build fact, not a constant here: a site
// declaring `[terms.people] fields = ["author", "editor", "jury"]` makes all
// three term fields, and `author` stops being one the moment a declared kind
// takes it. The kinds table comes from `list_vault_terms()` and nowhere else.

import { toNameList } from "../utils/name-list";
import { host as uiHost } from "../host";
import { engine } from '../engine';

// The last answer, flattened for the two questions asked of it: which kind a
// field feeds, and which names that kind has. `followTermChip` has to decide
// synchronously whether the gesture is a term gesture, so the answer is
// fetched ahead of the click — the chip bar refreshes it whenever it renders,
// and completion refreshes it again when a popover opens.
let kindOfField = new Map<string, string>();
let namesOfKind = new Map<string, string[]>();

/** Re-read the build's kinds table. Idempotent and failure-tolerant: a vault
 *  with no build answers nothing, and every term gesture then falls through to
 *  the ordinary chip click until the next build. */
export async function refreshVaultTerms(): Promise<void> {
  let result;
  try {
    result = await engine.listVaultTerms();
  } catch {
    return;
  }
  if (result.status !== 'ok') return;
  const terms = result.data;
  const fields = new Map<string, string>();
  const names = new Map<string, string[]>();
  for (const [key, kind] of Object.entries(terms?.kinds ?? {})) {
    names.set(key, kind?.names ?? []);
    for (const field of kind?.fields ?? []) fields.set(field, key);
  }
  kindOfField = fields;
  namesOfKind = names;
}

/** Handle cmd/ctrl+click on a term field's surface. `name` is the clicked
 *  token where one exists (tag editor tokens); the bar chip falls back to the
 *  field's first value. Returns true when the gesture was consumed. */
export function followTermChip(e: MouseEvent, key: string, fieldValue: unknown, name?: string): boolean {
  if (!(e.metaKey || e.ctrlKey) || !uiHost.resolveTerm) return false;
  // A field no kind feeds is not a term field: the gesture falls through and
  // the plain click opens the chip editor.
  if (!kindOfField.has(key)) return false;
  const term = (name ?? toNameList(fieldValue)[0])?.trim();
  if (!term) return false;
  e.preventDefault();
  e.stopPropagation();
  void navigateToTerm(key, term);
  return true;
}

async function navigateToTerm(field: string, name: string): Promise<void> {
  const resolve = uiHost.resolveTerm;
  if (!resolve) return;
  let result;
  try {
    result = await resolve(field, name);
  } catch {
    return;
  }
  // An error: the field feeds no kind in the build's own record (declared
  // away, or no build yet). The gesture quietly does nothing; the plain click
  // still opens the editor.
  if (result.status !== 'ok') return;
  const res = result.data;
  if (res.claimed_by) {
    document.dispatchEvent(
      new CustomEvent('editor:file-selected', { detail: { filePath: res.claimed_by } }),
    );
  } else {
    uiHost.bus.emit('editor:navigate-preview', { urlPath: res.url });
  }
}

/** Native datalist completion over the vault's existing terms, for any
 *  name-list field's input. Every field of one kind completes against that
 *  kind's single name list — an `editor:` on a `[terms.people]` site offers
 *  the same people an `author:` does. A field no kind feeds gets no datalist
 *  at all, so callers need no field test of their own. */
export function attachTermSuggestions(inp: HTMLInputElement, field: string, parent: HTMLElement): void {
  void refreshVaultTerms().then(() => {
    const kind = kindOfField.get(field);
    if (kind === undefined) return;
    const dl = document.createElement('datalist');
    dl.id = `term-suggestions-${field}`;
    inp.setAttribute('list', dl.id);
    parent.appendChild(dl);
    for (const name of namesOfKind.get(kind) ?? []) {
      const opt = document.createElement('option');
      opt.value = name;
      dl.appendChild(opt);
    }
  });
}
