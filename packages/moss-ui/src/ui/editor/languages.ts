/**
 * Language trees for the create flow: where a page's version in another
 * language lives, which languages a page lacks, and which template a folder
 * matches. Path arithmetic over what `siteLanguages` reports, plus one
 * existence probe per target. The placement rules are the Rust side's
 * (`moss_core::template`); this file only mirrors them for the menu.
 */

import { toAbsolute } from './path-domain';

type CommandResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

export interface SiteLanguagesInfo {
  default_lang: string;
  /** Folder names of the other language trees, e.g. `zh-hans`. */
  trees: string[];
}

/** The two reads this module makes; the bindings `commands` object satisfies it. */
export interface LanguageSource {
  siteLanguages(): Promise<CommandResult<SiteLanguagesInfo>>;
  listDirectory(absDir: string, showInternal: boolean): Promise<CommandResult<Array<{ name: string }>>>;
}

/** A site with no other language trees: every page is in the default language. */
export const ONE_LANGUAGE: SiteLanguagesInfo = { default_lang: '', trees: [] };

export interface TranslationSlot {
  lang: string;
  /** Project-relative path the translation is (or would be) at. */
  rel: string;
  exists: boolean;
  /** The page is a folder's home file and that folder is missing in this language. */
  folderMissing: boolean;
}

/** A folder spelling as the Rust side stores it: no leading `./` or `/`, no trailing `/`. */
export function folderKey(raw: string): string {
  return raw.replace(/^(\.\/|\/)+/, '').replace(/\/+$/, '');
}

/** The folder's name, for a row label. */
export function folderName(raw: string): string {
  return folderKey(raw).split('/').pop() ?? '';
}

/** Split a project-relative path into its language tree and the path inside the default tree. */
export function splitTree(rel: string, langs: SiteLanguagesInfo): { lang: string; inner: string } {
  const [head, ...rest] = rel.split('/');
  if (langs.trees.includes(head)) return { lang: head, inner: rest.join('/') };
  return { lang: langs.default_lang, inner: rel };
}

/** `inner` placed in `lang`'s tree; the default tree is not prefixed. */
export function inTree(inner: string, lang: string, langs: SiteLanguagesInfo): string {
  if (lang === langs.default_lang || lang === '') return inner;
  return inner ? `${lang}/${inner}` : lang;
}

/** The folder a template's pages go in, from a menu anchored on `anchorDir`. */
export function templateDestination(anchorDir: string, folder: string | null, langs: SiteLanguagesInfo): string {
  if (folder === null) return anchorDir;
  return inTree(folderKey(folder), splitTree(anchorDir, langs).lang, langs);
}

/** The templates whose folder is the one `dir` is in, in either language tree. */
export function templatesNamingFolder<T extends { folder: string | null }>(dir: string, templates: readonly T[], langs: SiteLanguagesInfo): T[] {
  const here = splitTree(dir, langs).inner;
  return templates.filter((m) => m.folder !== null && folderKey(m.folder) === here);
}

/**
 * Every other language's place for the page at `rel` (project-relative), and
 * whether a page already sits there. A probe that fails counts as absent.
 */
export async function translationSlots(
  rel: string,
  langs: SiteLanguagesInfo,
  source: LanguageSource,
  projectRoot: string,
): Promise<TranslationSlot[]> {
  if (langs.trees.length === 0 || !rel.endsWith('.md')) return [];
  const own = splitTree(rel, langs);
  const parts = own.inner.split('/');
  const stem = (parts.pop() ?? '').replace(/\.md$/, '');
  const isHome = parts.length > 0 && parts[parts.length - 1] === stem;
  const slots: TranslationSlot[] = [];
  for (const lang of [langs.default_lang, ...langs.trees]) {
    if (lang === own.lang) continue;
    const target = inTree(own.inner, lang, langs);
    const slash = target.lastIndexOf('/');
    const dirRel = slash < 0 ? '' : target.slice(0, slash);
    const name = target.slice(slash + 1);
    const { dirExists, exists } = await probe(source, dirRel, name, projectRoot);
    slots.push({ lang, rel: target, exists, folderMissing: isHome && !dirExists });
  }
  return slots;
}

async function probe(source: LanguageSource, dirRel: string, name: string, projectRoot: string): Promise<{ dirExists: boolean; exists: boolean }> {
  const absDir = dirRel ? toAbsolute(dirRel, projectRoot) : projectRoot;
  try {
    const listed = await source.listDirectory(absDir, true);
    if (listed.status !== 'ok') return { dirExists: false, exists: false };
    return { dirExists: true, exists: listed.data.some((e) => e.name === name) };
  } catch {
    return { dirExists: false, exists: false };
  }
}

/** Absolute `(language, page)` pairs for the translations that exist, the shape `create_template` takes. */
export function existingPages(slots: readonly TranslationSlot[], projectRoot: string): Array<[string, string]> {
  return slots.filter((s) => s.exists).map((s) => [s.lang, toAbsolute(s.rel, projectRoot)]);
}
