/**
 * Copy table for the moss-hosted subscribe form.
 *
 * Intentionally duplicated from the Rust `SubscribeI18n` in
 * `crates/moss-build/src/build/features/email.rs`: the set is tiny (5 strings × 4
 * locales) and both sides only need read access. Keeping them in sync by hand
 * is cheaper than plumbing the strings through the DOM as data- attributes.
 */

export type Lang = 'en' | 'de' | 'zh-hans' | 'zh-hant';

export interface SubscribeCopy {
  /** Placeholder inside the email input. */
  placeholder: string;
  /** Submit button label. */
  label: string;
  /** Shown once the link is on its way. */
  checkEmail: string;
  /** Shown in place of the field and button to a browser signed in to the site. */
  subscribed: string;
  /** Shown on network / server error. */
  error: string;
}

export const COPY: Record<Lang, SubscribeCopy> = {
  en: {
    placeholder: 'your@email.com',
    label: 'Subscribe',
    checkEmail: 'Check your email.',
    subscribed: 'Subscribed',
    error: 'Something went wrong. Try again?',
  },
  de: {
    placeholder: 'ihre@email.de',
    label: 'Abonnieren',
    checkEmail: 'Bitte prüfen Sie Ihre E-Mail.',
    subscribed: 'Abonniert',
    error: 'Es ist ein Fehler aufgetreten. Bitte versuchen Sie es erneut.',
  },
  'zh-hans': {
    placeholder: '邮箱',
    label: '订阅',
    checkEmail: '请查收邮件',
    subscribed: '已订阅',
    error: '出错了，请重试。',
  },
  'zh-hant': {
    placeholder: '電子郵件',
    label: '訂閱',
    checkEmail: '請查收郵件',
    subscribed: '已訂閱',
    error: '出錯了，請重試。',
  },
};

/**
 * Bucket an arbitrary BCP-47 language tag into one of the four copy keys.
 *
 * The blessed TS twin of Rust's `Language::from_bcp47_lenient` in
 * `crates/moss-build/src/i18n.rs` — any change here needs a corresponding Rust
 * change (and vice versa). Do not re-implement zh bucketing elsewhere;
 * import this.
 */
export function langBucket(raw: string | null | undefined): Lang {
  // Parse by subtags (mirrors Rust `from_bcp47_lenient`): normalize `_`→`-`,
  // recognize Sinitic language codes + bare script subtags, and pick script by
  // an explicit Hant/Hans subtag or a Traditional-using region.
  const normalized = (raw ?? '').toLowerCase().replace(/_/g, '-');
  const subtags = normalized.split('-').filter((s) => s.length > 0);
  const primary = subtags[0] ?? '';

  // German tags (`de`, `de-AT`, `deu`) are the German edition.
  if (primary === 'de' || primary === 'deu') {
    return 'de';
  }

  const SINITIC = ['zh', 'zho', 'cmn', 'yue', 'wuu', 'nan', 'hak', 'gan', 'hsn', 'lzh', 'czh'];
  const isChinese = SINITIC.includes(primary) || primary === 'hans' || primary === 'hant';
  if (!isChinese) {
    return 'en';
  }

  const HANT_REGIONS = ['tw', 'hk', 'mo'];
  const hasHantScript = subtags.includes('hant');
  const hasHansScript = subtags.includes('hans');
  const hasHantRegion = subtags.some((s) => HANT_REGIONS.includes(s));
  if (hasHantScript || (!hasHansScript && hasHantRegion)) {
    return 'zh-hant';
  }
  return 'zh-hans';
}
