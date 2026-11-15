import type { ImportOutcome } from '../host';
import { t } from '../i18n';

/**
 * What an import did to the folder's pages, for the line shown when it ends;
 * null when it reached no page at all.
 */
export function importedPagesText(counts: Pick<ImportOutcome, 'total_pages' | 'kept_pages'>): string | null {
  const total = counts.total_pages;
  const kept = String(counts.kept_pages);
  if (!total) {
    if (!counts.kept_pages) return null;
    return counts.kept_pages === 1 ? t('onboard.flow.url_all_kept_one') : t('onboard.flow.url_all_kept', { kept });
  }
  if (!counts.kept_pages) {
    return total === 1 ? t('onboard.flow.url_imported_pages_one') : t('onboard.flow.url_imported_pages', { total: String(total) });
  }
  return total === 1
    ? t('onboard.flow.url_imported_and_kept_one', { kept })
    : t('onboard.flow.url_imported_and_kept', { total: String(total), kept });
}
