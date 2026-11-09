import type { FileFilter } from '../ui/host';
import { t } from '../ui/i18n/index';
import { showToast } from '../ui/shared/ui/toast/toast-manager';

/**
 * A browser's file pick: a file input, then one upload per target folder to
 * `/__moss/upload`. Answers the absolute paths of the copies that landed in
 * the folder, since a browser cannot hand back a path on disk. `dirFor` names
 * each file's root-relative folder.
 */
export async function pickAndUpload(
  filters: FileFilter[],
  multiple: boolean,
  dirFor?: (fileName: string) => Promise<string>,
): Promise<string[]> {
  const input = document.createElement('input');
  input.type = 'file';
  input.multiple = multiple;
  input.hidden = true;
  input.accept = filters.flatMap((f) => f.extensions.map((e) => `.${e}`)).join(',');
  const files = await new Promise<File[]>((resolve) => {
    input.addEventListener('change', () => resolve([...(input.files ?? [])]), { once: true });
    input.addEventListener('cancel', () => resolve([]), { once: true });
    document.body.appendChild(input);
    input.click();
  });
  input.remove();

  const byDir = new Map<string, File[]>();
  for (const f of files) {
    const dir = dirFor ? await dirFor(f.name) : '';
    byDir.set(dir, [...(byDir.get(dir) ?? []), f]);
  }
  const landed: string[] = [];
  for (const [dir, batch] of byDir) {
    const form = new FormData();
    if (dir) form.append('dir', dir);
    for (const f of batch) form.append('file', f, f.name);
    const res = await fetch('/__moss/upload', { method: 'POST', credentials: 'same-origin', body: form }).catch(
      (e: unknown) => {
        console.warn('[upload] failed', e);
        return null;
      },
    );
    if (!res?.ok) {
      // Said out loud: a refused upload must not look like a cancelled pick.
      if (res) console.warn('[upload] refused', res.status, await res.text().catch(() => ''));
      showToast({ what: t(res?.status === 413 ? 'upload.too_large' : 'upload.refused'), severity: 'Blocking' });
      return [];
    }
    const report = (await res.json()) as { copied: Array<{ final_path: string }>; skipped: Array<{ original_name: string }> };
    if (report.skipped.length > 0) {
      showToast({
        what: report.skipped.length === 1
          ? t('dnd.import_skipped_one', { name: report.skipped[0].original_name })
          : t('dnd.import_skipped_many', { n: report.skipped.length }),
        severity: 'Blocking',
      });
    }
    landed.push(...report.copied.map((c) => c.final_path));
  }
  return landed;
}
