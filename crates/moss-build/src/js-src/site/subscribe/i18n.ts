/**
 * Copy table for the moss-hosted subscribe form.
 *
 * Intentionally duplicated from the Rust `SubscribeI18n` in
 * `crates/moss-build/src/build/features/email.rs`: the set is tiny (5 strings × 3
 * locales) and both sides only need read access. Keeping them in sync by hand
 * is cheaper than plumbing the strings through the DOM as data- attributes.
 */

export type Lang = 'en' | 'zh-hans' | 'zh-hant';

export interface SubscribeCopy {
  /** Placeholder inside the email input. */
  placeholder: string;
  /** Submit button label. */
  label: string;
  /** Shown after a fresh subscribe — double-opt-in confirmation prompt. */
  checkEmail: string;
  /** Shown when the email was already confirmed. */
  alreadySubscribed: string;
  /** Shown on network / server error. */
  error: string;
}

export const COPY: Record<Lang, SubscribeCopy> = {
  en: {
    placeholder: 'your@email.com',
    label: 'Subscribe',
    checkEmail: 'Check your email to confirm.',
    alreadySubscribed: "You're already subscribed.",
    error: 'Something went wrong. Try again?',
  },
  'zh-hans': {
    placeholder: '邮箱',
    label: '订阅',
    checkEmail: '请在邮箱中确认订阅',
    alreadySubscribed: '你已订阅。',
    error: '出错了，请重试。',
  },
  'zh-hant': {
    placeholder: '電子郵件',
    label: '訂閱',
    checkEmail: '請在電子郵件中確認訂閱',
    alreadySubscribed: '你已訂閱。',
    error: '出錯了，請重試。',
  },
};

export interface ApplyCopy {
  /** Submit button label (idle). */
  label: string;
  /** Submit button label after terminal success. */
  labelSuccess: string;
  /** Placeholder for the second apply field (a Matters username OR a one-line pitch).
   * Must stay in sync with `matters_ph` in `MossApplyCopy` (email.rs). */
  mattersPh: string;
  /** Shown after a successful application — replaces check-email. */
  received: string;
  /** Shown on network / server error. */
  error: string;
}

export const APPLY_COPY: Record<Lang, ApplyCopy> = {
  en: {
    label: 'Apply',
    labelSuccess: 'Application sent',
    mattersPh: 'Matters username',
    received: "Thanks for applying. Please confirm the email we just sent; we'll email you when your turn comes.",
    error: 'Something went wrong. Try again?',
  },
  'zh-hans': {
    label: '申请',
    labelSuccess: '申请已提交',
    mattersPh: 'Matters 用户名',
    received: '谢谢你的申请，请查收确认邮件。我们会在轮到你时通过邮件邀请。',
    error: '出错了，请重试。',
  },
  'zh-hant': {
    label: '申請',
    labelSuccess: '申請已提交',
    mattersPh: 'Matters 用戶名',
    received: '謝謝你的申請，請查收確認郵件。我們會在輪到你時透過郵件邀請。',
    error: '出錯了，請重試。',
  },
};

/**
 * Bucket an arbitrary BCP-47 language tag into one of the three copy keys.
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
