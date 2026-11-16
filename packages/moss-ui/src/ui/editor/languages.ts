/**
 * Language trees for the create flow: where a page's version in another
 * language lives, which languages a page lacks, and which template a folder
 * matches. Path arithmetic over what `siteLanguages` reports; which
 * versions a page has is the Rust side's answer (`pageTranslations`), the
 * rule a new version is refused by.
 */

import { toAbsolute } from './path-domain';
import type { TranslationSlot } from '../../engine.generated';

export type { TranslationSlot };

type CommandResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

export interface SiteLanguagesInfo {
  default_lang: string;
  /** Folder names of the other language trees, e.g. `zh-hans`. */
  trees: string[];
}

/** The two reads this module makes; the bindings `commands` object satisfies it. */
export interface LanguageSource {
  siteLanguages(): Promise<CommandResult<SiteLanguagesInfo>>;
  pageTranslations(absPath: string): Promise<CommandResult<TranslationSlot[]>>;
}

/** A site with no other language trees: every page is in the default language. */
export const ONE_LANGUAGE: SiteLanguagesInfo = { default_lang: '', trees: [] };

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
 * whether the page already has a version there. A read that fails offers nothing.
 */
export async function translationSlots(rel: string, source: Pick<LanguageSource, 'pageTranslations'>, projectRoot: string): Promise<TranslationSlot[]> {
  try {
    const slots = await source.pageTranslations(toAbsolute(rel, projectRoot));
    return slots.status === 'ok' ? slots.data : [];
  } catch {
    return [];
  }
}

/** Absolute `(language, page)` pairs for the translations that exist, the shape `create_template` takes. */
export function existingPages(slots: readonly TranslationSlot[], projectRoot: string): Array<[string, string]> {
  return slots.filter((s) => s.exists).map((s) => [s.lang, toAbsolute(s.rel, projectRoot)]);
}
