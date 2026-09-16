/**
 * Copy table for the comment widget's client-side status messages.
 *
 * Follows the `frontend/site/subscribe/i18n.ts` pattern: a tiny set
 * (6 strings × 3 locales) duplicated by hand rather than plumbed through
 * the DOM. The server-rendered parts of the comment section (form labels,
 * counts, dates) come from the Rust registry
 * (`src-tauri/src/i18n/strings.rs`); this table covers only the strings
 * the widget JS itself injects after page load.
 *
 * Bucketing reuses `langBucket` from the subscribe module — the blessed
 * TS twin of Rust's `Language::from_bcp47_lenient`.
 */

import { langBucket, type Lang } from '../subscribe/i18n';

export interface CommentWidgetCopy {
  /** Shown when submit is attempted without a name. */
  nameRequired: string;
  /** Shown while the POST is in flight. */
  submitting: string;
  /** Shown while the captcha challenge is being solved. */
  verifying: string;
  /** Shown after a successful post. */
  posted: string;
  /** Author label for the optimistic insert when no name was given. */
  anonymous: string;
  /** Timestamp label for the optimistic insert. */
  justNow: string;
  /** Prefix for network/server error messages. */
  errorPrefix: string;
}

export const COPY: Record<Lang, CommentWidgetCopy> = {
  en: {
    nameRequired: 'Name is required',
    submitting: 'Submitting...',
    verifying: 'Verifying…',
    posted: 'Sent',
    anonymous: 'Anonymous',
    justNow: 'just now',
    errorPrefix: 'Error: ',
  },
  'zh-hans': {
    nameRequired: '请填写名字',
    submitting: '提交中…',
    verifying: '验证中…',
    posted: '已发送',
    anonymous: '匿名',
    justNow: '刚刚',
    errorPrefix: '出错了：',
  },
  'zh-hant': {
    nameRequired: '請填寫名字',
    submitting: '提交中…',
    verifying: '驗證中…',
    posted: '已發送',
    anonymous: '匿名',
    justNow: '剛剛',
    errorPrefix: '出錯了：',
  },
};

/** Resolve the widget copy for the current page's `<html lang>`. */
export function widgetCopy(): CommentWidgetCopy {
  return COPY[langBucket(document.documentElement.lang)];
}
