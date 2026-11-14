/**
 * Page-template prompts: "Save as Template…" (tree caller), and the rename
 * popover + delete confirm the ＋▾ menu's Manage Templates… levels reach
 * (create-flow.ts). Menu composition is NOT here — the surface that shows a
 * menu decides what it contains. The template rows and manage levels are built in
 * create-flow's `toggleMenu`.
 *
 * Reaches its commands through an injected structural interface, as
 * create-flow.ts does.
 */

import { t } from "../i18n/index";
import { langName } from "../i18n/lang-names";
import { mossConfirm } from "../shared/ui/moss-alert";
import { openInputPopover } from "../shared/ui/input-popover";
import { showAck, showToast } from "../shared/ui/toast/toast-manager";
import { toRelative } from './path-domain';
import { existingPages, folderName, translationSlots, type LanguageSource } from './languages';

type CommandResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

export interface TemplateMeta {
  id: string;
  name: string;
  kind: 'page' | 'folder';
  created_at: string;
  /** The folder its pages belong in (`work/`), when it names one. */
  folder: string | null;
  /** The languages it has a file for besides the default. */
  langs: string[];
}

export interface TemplateCommands {
  createTemplate(sourcePath: string, name: string, isHome: boolean, translations?: [string, string][] | null): Promise<CommandResult<string>>;
  listTemplates(): Promise<CommandResult<TemplateMeta[]>>;
  renameTemplate(id: string, newName: string): Promise<CommandResult<null>>;
  setTemplateFolder(id: string, folder: string | null): Promise<CommandResult<null>>;
  deleteTemplate(id: string): Promise<CommandResult<null>>;
}

const point = (ev: MouseEvent) => ({ x: ev.clientX, y: ev.clientY });

export function templateFailed(error: string): void {
  showToast({ what: t('tree.template_failed', { error }), severity: 'Blocking' });
}

/** Menu-row text for a template: folder templates carry the kind suffix. */
export function templateLabel(meta: TemplateMeta): string {
  return meta.kind === 'folder' ? meta.name + t('tree.template_folder_suffix') : meta.name;
}

/** A row for a template that names a folder says where it goes. */
export function templateRowLabel(meta: TemplateMeta): string {
  const base = templateLabel(meta);
  if (meta.folder === null) return base;
  return t('create.template_in_folder', { name: base, folder: folderName(meta.folder) });
}

/** `source` is the page as the tree sees it: `isHome` is the tree's own election
 *  for the source row (a folder's home file captures as a folder template).
 *  When the page has translations, the author is asked whether to save them too. */
export function promptSaveAsTemplate(
  commands: Pick<TemplateCommands, 'createTemplate'> & Pick<LanguageSource, 'pageTranslations'>,
  source: { absPath: string; projectRoot: string; isHome: boolean },
  at: MouseEvent,
): void {
  openInputPopover({
    label: t('tree.template_name_label'),
    at: point(at),
    onSubmit: (name) => {
      void translationsToSave(commands, source).then((translations) =>
        commands.createTemplate(source.absPath, name, source.isHome, translations).then((result) => {
          if (result.status !== 'ok') { templateFailed(result.error); return; }
          showAck(t('tree.template_saved', { name }));
        }),
      );
    },
  });
}

async function translationsToSave(
  commands: Pick<LanguageSource, 'pageTranslations'>,
  source: { absPath: string; projectRoot: string },
): Promise<[string, string][] | null> {
  const rel = toRelative(source.absPath, source.projectRoot);
  const found = existingPages(await translationSlots(rel, commands, source.projectRoot), source.projectRoot);
  if (found.length === 0) return null;
  const names = found.map(([lang]) => langName(lang)).join(', ');
  const save = await mossConfirm(t('tree.template_translations_offer', { language: names }), {
    confirmLabel: t('tree.template_translations_confirm'),
  });
  return save ? found : null;
}

/** Manage Templates… → Folder…: the folder the template's pages go in. */
export function promptTemplateFolder(
  commands: Pick<TemplateCommands, 'setTemplateFolder'>,
  meta: TemplateMeta,
  at: MouseEvent,
): void {
  openInputPopover({
    label: t('tree.template_folder_label'),
    at: point(at),
    initial: meta.folder ?? '',
    onSubmit: (folder) => {
      void commands.setTemplateFolder(meta.id, folder).then((result) => {
        if (result.status !== 'ok') templateFailed(result.error);
      });
    },
  });
}

/** Manage Templates… → Clear folder: the template's pages go back to the menu's folder. */
export function clearTemplateFolder(commands: Pick<TemplateCommands, 'setTemplateFolder'>, meta: TemplateMeta): void {
  void commands.setTemplateFolder(meta.id, null).then((result) => {
    if (result.status !== 'ok') templateFailed(result.error);
  });
}

/** The shared input popover, prefilled with the current name, at `at` — the
 *  Manage Templates… row that was activated (keyboard activation synthesizes
 *  the row's rectangle). */
export function promptRenameTemplate(
  commands: Pick<TemplateCommands, 'renameTemplate'>,
  meta: TemplateMeta,
  at: MouseEvent,
): void {
  openInputPopover({
    label: t('tree.template_name_label'),
    at: point(at),
    initial: meta.name,
    onSubmit: (name) => {
      void commands.renameTemplate(meta.id, name).then((result) => {
        if (result.status !== 'ok') templateFailed(result.error);
      });
    },
  });
}

export async function deleteTemplate(
  commands: Pick<TemplateCommands, 'deleteTemplate'>,
  meta: TemplateMeta,
): Promise<void> {
  // Destructive: `delete_template` unlinks the file directly (no OS Trash).
  const proceed = await mossConfirm(t('tree.template_delete_confirm', { name: meta.name }), {
    confirmLabel: t('tree.delete'),
    destructive: true,
  });
  if (!proceed) return;
  const result = await commands.deleteTemplate(meta.id);
  if (result.status !== 'ok') templateFailed(result.error);
}
