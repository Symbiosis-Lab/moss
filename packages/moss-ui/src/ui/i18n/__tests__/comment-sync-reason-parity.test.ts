/**
 * Twin of the Rust test `comment_sync_reason_strings_match_ui_strings_twins`
 * in crates/moss-build/src/infra/app_advisory.rs.
 *
 * The Services > Comments failure line (services.comments.reason_*) and the
 * advisory window (app_advisory sync_reason_*) must tell the same story per
 * locale. Both tables are hand-maintained, so these strings are asserted
 * byte-for-byte on BOTH sides — drift in either file fails that side's suite.
 * This side checks four locales, the Rust side three (it does not check German).
 * Update both tests together.
 */

import { describe, it, expect } from 'vitest';
import { UI_STRINGS, type AppLocale } from '../ui-strings';

const EXPECTED = [
  {
    key: 'services.comments.reason_offline',
    en: 'no internet connection',
    de: 'keine Internetverbindung',
    'zh-hans': '当前没有网络连接',
    'zh-hant': '目前沒有網路連線',
  },
  {
    key: 'services.comments.reason_server',
    en: 'comment server unreachable',
    de: 'Kommentarserver nicht erreichbar',
    'zh-hans': '无法连接评论服务器',
    'zh-hant': '無法連線留言伺服器',
  },
  {
    key: 'services.comments.reason_internal',
    en: 'a problem with comment data',
    de: 'ein Problem mit den Kommentardaten',
    'zh-hans': '评论数据出现问题',
    'zh-hant': '留言資料出現問題',
  },
] as const;

describe('comment-sync reason strings match the app_advisory.rs twins', () => {
  for (const row of EXPECTED) {
    for (const locale of Object.keys(UI_STRINGS) as AppLocale[]) {
      it(`${row.key} (${locale})`, () => {
        expect(UI_STRINGS[locale][row.key]).toBe(row[locale]);
      });
    }
  }
});
