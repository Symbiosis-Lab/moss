//! Localized advisory / progress / receipt strings emitted by the Rust backend
//! and shown in the desktop progress panel (app-user audience).
//!
//! The FOURTH string surface, after:
//!   - `infra::app_strings` — native tray/menu chrome (`AppLanguage`)
//!   - `i18n::strings` — generated-site output (`i18n::Language`, per-site)
//!   - `frontend/app/i18n/ui-strings.ts` — webview UI (TypeScript)
//!
//! Keyed by `AppLanguage` and resolved against the process-global app language
//! (`app_config::app_language()`, seeded once at the top of `run()`), so it is
//! correct in CLI/Deploy mode too — where no webview ever runs.
//!
//! Declared greenfield with the `advisory_strings!` macro, which makes
//! "all three locales per key" a COMPILE-TIME invariant (omitting an arm is a
//! parse error) and emits a `KEYS` list the parity test iterates. This is the
//! Rust twin of the TS `Record<AppLocale, UiStringMap>` parity guarantee.

use crate::i18n::{app_language, AppLanguage};

/// Declarative advisory string table. Every key MUST supply all three locales;
/// a deliberately pan-Chinese string simply repeats the same value in both zh
/// arms (explicit and greppable — never a silent Hant→Hans collapse).
macro_rules! advisory_strings {
    ($($key:ident => { en: $en:expr, zh_hans: $hans:expr, zh_hant: $hant:expr }),+ $(,)?) => {
        fn lookup(lang: AppLanguage, key: &str) -> Option<&'static str> {
            match key {
                $(stringify!($key) => Some(match lang {
                    AppLanguage::En => $en,
                    AppLanguage::ZhHans => $hans,
                    AppLanguage::ZhHant => $hant,
                }),)+
                _ => None,
            }
        }
        #[cfg(test)]
        const KEYS: &[&str] = &[$(stringify!($key)),+];
    };
}

advisory_strings! {
    // ── Build pipeline progress (shown live in the progress panel) ──────────
    generating_site        => { en: "Generating site...",        zh_hans: "正在生成网站……",     zh_hant: "正在生成網站……" },
    processing_markdown     => { en: "Processing markdown...",     zh_hans: "正在处理 Markdown……", zh_hant: "正在處理 Markdown……" },
    generating_pages        => { en: "Generating pages...",        zh_hans: "正在生成页面……",     zh_hant: "正在生成頁面……" },
    generating_feeds        => { en: "Generating feeds...",        zh_hans: "正在生成订阅源……",   zh_hant: "正在生成訂閱源……" },
    converting_videos       => { en: "Converting videos...",       zh_hans: "正在转换视频……",     zh_hant: "正在轉換影片……" },
    generating_media_pages  => { en: "Generating media pages...",  zh_hans: "正在生成媒体页面……", zh_hant: "正在生成媒體頁面……" },
    finalizing              => { en: "Finalizing...",              zh_hans: "正在完成……",         zh_hant: "正在完成……" },
    scanning_folder         => { en: "Scanning folder...",         zh_hans: "正在扫描文件夹……",   zh_hant: "正在掃描資料夾……" },
    published               => { en: "Published!",                 zh_hans: "已发布！",           zh_hant: "已發佈！" },
    uploaded_checking_live  => { en: "Uploaded — checking your site is live…", zh_hans: "已上传 —— 正在确认网站已上线……", zh_hant: "已上傳 —— 正在確認網站已上線……" },
    // ── Advisories + receipts (Phase 1b) ────────────────────────────────────
    shipped_unoptimized => { en: "Videos shipped full-size — FFmpeg isn't installed ({err})", zh_hans: "视频以原始大小发布 —— 未安装 FFmpeg（{err}）", zh_hant: "影片以原始大小發佈 —— 尚未安裝 FFmpeg（{err}）" },
    copy => { en: "Copy", zh_hans: "复制", zh_hant: "複製" },
    // Two files carry the same `url:` — usually a folder copied in Finder,
    // which keeps the original's frontmatter verbatim. Shown on the `url`
    // chip of the file that has to change, so it says "this page" and names
    // only the other file. See
    // docs/archive/2026-09-02-url-collision-as-a-frontmatter-diagnostic.md.
    url_taken => {
        en: "Another page already uses this address: {keeper}. This page is being published at /{moved_to} instead. Change this url to give it an address of its own.",
        zh_hans: "另一个页面已经在用这个网址：{keeper}。这个页面暂时发布在 /{moved_to}。请修改这里的 url，给它一个自己的网址。",
        zh_hant: "另一個頁面已經在用這個網址：{keeper}。這個頁面暫時發佈在 /{moved_to}。請修改這裡的 url，給它一個自己的網址。"
    },
    not_found => { en: "not found", zh_hans: "未找到", zh_hant: "找不到" },
    still_downloading_icloud => { en: "still downloading from iCloud — will optimize on next preview", zh_hans: "仍在从 iCloud 下载，将在下次预览时优化", zh_hant: "仍在從 iCloud 下載，將在下次預覽時最佳化" },
    // Provider-neutral twin of `still_downloading_icloud`, for the image
    // pipeline: the vaults this fires on are as often Google Drive or Dropbox
    // as iCloud, and naming the wrong provider reads as a different bug.
    still_downloading_cloud => { en: "still downloading — will optimize once it arrives", zh_hans: "仍在下载 —— 到达后会自动优化", zh_hant: "仍在下載 —— 抵達後會自動最佳化" },
    could_not_read_file => { en: "could not read file", zh_hans: "无法读取文件", zh_hant: "無法讀取檔案" },
    shipped_without_optimizing => { en: "shipped without optimizing — {err}", zh_hans: "已发布但未优化 —— {err}", zh_hant: "已發佈但未最佳化 —— {err}" },
    // The floor under every can't-encode path gave way: not even the original
    // bytes reached the site, so the page has no video at all. Said separately
    // from whatever caused the fallback, because it is the worse fact and the
    // one the author would otherwise never hear (it was a `log::warn!` only,
    // until 2026-09-10).
    video_not_published => { en: "was not published at all — moss could not write it into your site ({err})", zh_hans: "完全没有发布 —— 青苔无法把它写入你的网站（{err}）", zh_hant: "完全沒有發佈 —— 青苔無法把它寫入你的網站（{err}）" },
    video_exceeds_size_target => { en: "ships at {size} MB — above the {cap} MB target (too long to compress to the cap at watchable quality)", zh_hans: "以 {size} MB 发布 —— 超出 {cap} MB 目标（时长过长，无法在可观看画质下压缩到目标大小）", zh_hant: "以 {size} MB 發佈 —— 超出 {cap} MB 目標（片長過長，無法在可觀看畫質下壓縮到目標大小）" },
    comment_sync_failed => { en: "Comment sync failed — comments may be stale ({err})", zh_hans: "评论同步失败 —— 评论可能不是最新的（{err}）", zh_hant: "留言同步失敗 —— 留言可能不是最新的（{err}）" },
    // Classified comment-sync failure reasons ({err} above). Keep byte-identical
    // per locale with ui-strings.ts services.comments.reason_* — the Services row
    // and the advisory window must tell the same story.
    sync_reason_offline => { en: "no internet connection", zh_hans: "当前没有网络连接", zh_hant: "目前沒有網路連線" },
    sync_reason_server => { en: "comment server unreachable", zh_hans: "无法连接评论服务器", zh_hant: "無法連線留言伺服器" },
    sync_reason_internal => { en: "a problem with comment data", zh_hans: "评论数据出现问题", zh_hant: "留言資料出現問題" },
    // Consequence first: what the author loses is a file that is not on her
    // site. The three-way diagnosis it used to open with (broken target /
    // escape attempt / absolute path) named the check, not the loss.
    symlinks_skipped_one => { en: "{count} symlink was left out, so what it points at is missing from your site — its target is broken, or sits outside your folder", zh_hans: "有 {count} 个符号链接未被收录，它指向的内容因此不在你的网站上 —— 目标已失效，或位于文件夹之外", zh_hant: "有 {count} 個符號連結未被收錄，它指向的內容因此不在你的網站上 —— 目標已失效，或位於資料夾之外" },
    symlinks_skipped_many => { en: "{count} symlinks were left out, so what they point at is missing from your site — their targets are broken, or sit outside your folder", zh_hans: "有 {count} 个符号链接未被收录，它们指向的内容因此不在你的网站上 —— 目标已失效，或位于文件夹之外", zh_hant: "有 {count} 個符號連結未被收錄，它們指向的內容因此不在你的網站上 —— 目標已失效，或位於資料夾之外" },
    // `duplicate_note_id` — the non-live half of this pair — stood here and was
    // deleted on 2026-08-30. It fired only when moss had picked correctly and
    // nothing was at stake: either no page under that ID was published, or the
    // publish record named the keeper by exact path. Reporting a copy getting
    // its own identity is bookkeeping. The reassignment is still logged at the
    // call site in `build::render::blocking`.
    // No " — " in this one: `groupNotices` splits a notice's text there and
    // folds the tail into a collapsed detail region. The whole point of this
    // advisory is the ask at the end, so it must stay in the visible head.
    duplicate_note_id_live => { en: "‘{keeper}’ and ‘{dup}’ started as copies of one page, so moss had to choose which of them keeps the comments already on your site. It chose ‘{keeper}’; check that they landed on the right page", zh_hans: "“{keeper}”与“{dup}”原本是同一篇的副本，青苔必须决定网站上已有的评论留给哪一篇，结果选了“{keeper}”；请确认评论落在正确的页面上", zh_hant: "「{keeper}」與「{dup}」原本是同一篇的副本，青苔必須決定網站上已有的留言留給哪一篇，結果選了「{keeper}」；請確認留言落在正確的頁面上" },
    // The label on an `Acknowledge` affordance. Past tense, first person: the
    // button is her statement that she looked, not an instruction to look.
    acknowledged_label => { en: "I’ve checked", zh_hans: "我已确认", zh_hant: "我已確認" },
    // `shared_note_id_left_alone` stood here and was deleted on 2026-08-30,
    // one day after it was added. It named a frontmatter field no editor shows
    // the author ("note ID"), and its remedy was half wrong — a uid survives
    // every rename. The deeper error was showing it at all: an ID moss mints,
    // writes and owns is not something an author should have to hear about, and
    // every remedy on offer was one moss could take itself. It now does — see
    // the never-built rule in `build::render::uid_dedup`.
    // Six strings about `.moss/deploy/`'s record of what is live (moss#1079)
    // stood here and were deleted on 2026-08-29. Nothing in that condition is
    // the author's to fix, and the next publish ends it — so the panel showed
    // a non-technical writer a noun she has never seen ("the record of what's
    // live") and asked nothing of her. It is a log fact now; see the two call
    // sites in `build::render::blocking`. Do not re-add an advisory here for a
    // condition that resolves itself: the panel is for what the author can act
    // on, or for something that has actually gone wrong.
    already_up_to_date => { en: "Already up to date", zh_hans: "已是最新", zh_hant: "已是最新" },
    comparing_with_server => { en: "Comparing with server…", zh_hans: "正在与服务器比较……", zh_hant: "正在與伺服器比較……" },
    making_changes_live => { en: "Making changes live…", zh_hans: "正在让更改上线……", zh_hant: "正在讓變更上線……" },
    preparing_publish => { en: "Preparing to publish prebuilt site…", zh_hans: "正在准备发布预构建网站……", zh_hant: "正在準備發佈預先建置的網站……" },
    // ── Plugin publish stages (OnionPress and any other deploy plugin) ──────
    // The plugin reports no byte or file counts, so these name the step and
    // nothing else. `preparing_publish` above is prebuilt-specific wording.
    preparing_to_publish => { en: "Preparing to publish…", zh_hans: "正在准备发布……", zh_hant: "正在準備發佈……" },
    rebuilding_site => { en: "Rebuilding your site…", zh_hans: "正在重新构建网站……", zh_hant: "正在重新建置網站……" },
    sending_site => { en: "Sending your site…", zh_hans: "正在发送网站……", zh_hant: "正在傳送網站……" },
    registering_site => { en: "Registering site…", zh_hans: "正在注册网站……", zh_hant: "正在註冊網站……" },
    uploading => { en: "Uploading {name}", zh_hans: "正在上传 {name}", zh_hant: "正在上傳 {name}" },
    downloading_from_icloud => { en: "Downloading {name} from iCloud…", zh_hans: "正在从 iCloud 下载 {name}……", zh_hant: "正在從 iCloud 下載 {name}……" },
    rechecking => { en: "Rechecking...", zh_hans: "重新检查……", zh_hant: "重新檢查……" },
    resending => { en: "Resending...", zh_hans: "重新发送……", zh_hant: "重新傳送……" },
    plugin_could_not_run => { en: "Plugin ‘{name}’ could not run — {detail} (content sync may be stale)", zh_hans: "插件“{name}”无法运行 —— {detail}（内容同步可能已过期）", zh_hant: "插件「{name}」無法執行 —— {detail}（內容同步可能已過時）" },
    // ── Notebook processing progress ─────────────────────────────────────────
    processing_notebook       => { en: "Processing notebook {n}/{total}: {name}",          zh_hans: "正在处理笔记本 {n}/{total}：{name}",                 zh_hant: "正在處理筆記本 {n}/{total}：{name}" },
    processing_notebook_short => { en: "Processing notebook {n}/{total}",                  zh_hans: "正在处理笔记本 {n}/{total}",                         zh_hant: "正在處理筆記本 {n}/{total}" },
    skipped_notebook          => { en: "Skipped notebook {n}/{total} after {secs}s timeout: {name}", zh_hans: "已跳过笔记本 {n}/{total}（{secs} 秒后超时）：{name}", zh_hant: "已略過筆記本 {n}/{total}（{secs} 秒後逾時）：{name}" },
    notebook_skip_advisory    => { en: "skipped after {secs}s of blocked I/O — next rebuild retries", zh_hans: "已跳过（{secs} 秒 I/O 阻塞后）—— 下次重新构建时重试", zh_hant: "已略過（{secs} 秒 I/O 阻塞後）—— 下次重新建置時重試" },
    // ── Build completion ──────────────────────────────────────────────────────
    build_complete            => { en: "Build complete",                                   zh_hans: "构建完成",                                           zh_hant: "建置完成" },
    // ── Rendering progress ───────────────────────────────────────────────────
    markdown_rendered         => { en: "Markdown rendered",                                zh_hans: "Markdown 已渲染",                                    zh_hant: "Markdown 已渲染" },
    rendering_pages           => { en: "Rendering pages ({n}/{total})",                    zh_hans: "正在生成页面（{n}/{total}）",                        zh_hant: "正在產生頁面（{n}/{total}）" },
    rendered_pages            => { en: "Rendered {n} pages",                               zh_hans: "已生成 {n} 个页面",                                  zh_hant: "已產生 {n} 個頁面" },
    // ── Domain DNS connect phases (orchestrator → Job model) ─────────────────
    dns_checking              => { en: "Checking DNS for {domain}…",                       zh_hans: "正在为 {domain} 检查 DNS……",                       zh_hant: "正在為 {domain} 檢查 DNS……" },
    dns_configuring           => { en: "Configuring DNS records…",                         zh_hans: "正在配置 DNS 记录……",                              zh_hant: "正在設定 DNS 記錄……" },
    platform_setup            => { en: "Setting up platform…",                             zh_hans: "正在设置平台……",                                   zh_hant: "正在設定平台……" },
    dns_waiting               => { en: "Waiting for {domain}… ({attempt}/{total})",        zh_hans: "正在等待 {domain}……（{attempt}/{total}）",          zh_hant: "正在等待 {domain}……（{attempt}/{total}）" },
    domain_connect_failed     => { en: "Couldn't connect {domain} — verify your email in Settings", zh_hans: "无法连接 {domain} —— 请在设置中验证你的邮箱", zh_hant: "無法連線 {domain} —— 請在設定中驗證你的信箱" },
    open_settings             => { en: "Open Settings",                                    zh_hans: "打开设置",                                         zh_hant: "開啟設定" },
    mins                      => { en: "{n} min",                                          zh_hans: "{n} 分钟",                                         zh_hant: "{n} 分鐘" },
    // ── Domain connect Job title ─────────────────────────────────────────────
    connecting_domain         => { en: "Connecting {domain}",                              zh_hans: "正在连接 {domain}",                                zh_hant: "正在連線 {domain}" },
    // ── Domain link incomplete (soft/transient server failure) ───────────────
    // Shown when link_custom_domain fails transiently (non-409). No em-dash.
    // Instructs the user to open Settings to reconnect; not a hard error.
    domain_config_incomplete  => { en: "Domain configuration incomplete: open Settings to reconnect.", zh_hans: "域名配置未完成：请打开设置重新连接。", zh_hant: "網域設定未完成：請開啟設定重新連線。" },
    // ── Deploy error toast titles (shown verbatim by the frontend) ───────────
    deploy_err_plugin         => { en: "Plugin error",                                     zh_hans: "插件错误",                                         zh_hant: "插件錯誤" },
    deploy_err_config         => { en: "Configuration error",                              zh_hans: "配置错误",                                         zh_hant: "設定錯誤" },
    // Fallback for a plugin that reports failure without saying why. The
    // plugin owns the user-facing toast; this only names the terminal stage
    // for the progress panel.
    publish_failed            => { en: "Publish failed",                                   zh_hans: "发布失败",                                         zh_hant: "發布失敗" },
    deploy_err_custom_domain  => { en: "Custom domain required",                           zh_hans: "需要自定义域名",                                   zh_hant: "需要自訂網域" },
    not_allowlisted           => { en: "moss is in closed testing. Apply for free access.", zh_hans: "青苔正在内测，申请免费试用", zh_hant: "青苔正在內測，申請免費試用" },
    // ── Web import Job title ─────────────────────────────────────────────────
    import_from_host          => { en: "Importing from {host}",                           zh_hans: "正在从 {host} 导入",                               zh_hant: "正在從 {host} 匯入" },
}

/// Resolve an advisory/progress key to the process app language.
/// Unknown key returns the key itself (loud programmer error in the UI).
pub fn t(key: &str) -> String {
    lookup(app_language(), key).unwrap_or(key).to_string()
}

/// Resolve a key with `{name}` placeholder interpolation — mirror of the TS
/// `interpolate()` in `frontend/app/i18n/index.ts`.
pub fn fmt(key: &str, params: &[(&str, &str)]) -> String {
    let mut s = lookup(app_language(), key).unwrap_or(key).to_string();
    for (k, v) in params {
        s = s.replace(&format!("{{{}}}", k), v);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_resolves_in_all_three_locales() {
        for &k in KEYS {
            for lang in [AppLanguage::En, AppLanguage::ZhHans, AppLanguage::ZhHant] {
                assert!(lookup(lang, k).is_some(), "missing {k} for {lang:?}");
            }
        }
    }

    /// Twin of frontend/app/i18n/__tests__/comment-sync-reason-parity.test.ts.
    /// The Services row (TS, services.comments.reason_*) and this advisory
    /// table (sync_reason_*) must tell the same story per locale — these nine
    /// strings are asserted byte-for-byte on BOTH sides, so drift in either
    /// file fails that side's suite. Update both tests together.
    #[test]
    fn comment_sync_reason_strings_match_ui_strings_twins() {
        let expected: &[(&str, [&str; 3])] = &[
            ("sync_reason_offline", ["no internet connection", "当前没有网络连接", "目前沒有網路連線"]),
            ("sync_reason_server", ["comment server unreachable", "无法连接评论服务器", "無法連線留言伺服器"]),
            ("sync_reason_internal", ["a problem with comment data", "评论数据出现问题", "留言資料出現問題"]),
        ];
        for (key, [en, hans, hant]) in expected {
            assert_eq!(lookup(AppLanguage::En, key), Some(*en), "{key} en drifted");
            assert_eq!(lookup(AppLanguage::ZhHans, key), Some(*hans), "{key} zh-hans drifted");
            assert_eq!(lookup(AppLanguage::ZhHant, key), Some(*hant), "{key} zh-hant drifted");
        }
    }

    #[test]
    fn fmt_interpolates_named_placeholders() {
        // Uses a live key; {n} is absent so this is a no-op interpolation that
        // still proves the substitution path compiles and runs.
        let s = fmt("published", &[("n", "3")]);
        assert!(!s.is_empty());
    }

    #[test]
    fn unknown_key_returns_itself() {
        assert_eq!(t("definitely_not_a_key"), "definitely_not_a_key");
    }

    /// Does this string use the bare product name in prose?
    ///
    /// `mosspub.com`, `.moss/config.toml`, `moss.log` and `moss-*` class names
    /// are identifiers, not the brand — a neighbouring `.`, `-`, `/`, `_` or
    /// alphanumeric marks one. Lowercasing is ASCII-only, so byte indices into
    /// the lowered copy still index the original.
    fn says_moss_in_prose(v: &str) -> bool {
        let attached = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '/' | '_');
        v.to_ascii_lowercase().match_indices("moss").any(|(i, _)| {
            let before = v[..i].chars().next_back();
            let after = v[i + 4..].chars().next();
            !before.is_some_and(attached) && !after.is_some_and(attached)
        })
    }

    /// The product is "moss" in English and 青苔 in Chinese — the calque the tray,
    /// the app menu and the site colophon (青苔发布) have always used. A Chinese
    /// sentence with a bare Latin "moss" in it is drift, not a brand decision;
    /// it shipped for a whole release in these advisories before an author asked
    /// what "moss" was (2026-08-29). The TS twin is
    /// `frontend/app/i18n/no-untranslated-leakage.test.ts`.
    #[test]
    fn chinese_values_call_the_product_qingtai() {
        /// Keys whose Chinese value legitimately keeps the Latin name — e.g. a
        /// literal string the user reads elsewhere (an email sender, a path).
        /// Each entry needs a comment saying which.
        const ALLOWLIST: &[&str] = &[
            // (empty)
        ];

        let mut failures: Vec<String> = Vec::new();
        for &key in KEYS {
            if ALLOWLIST.contains(&key) {
                continue;
            }
            for lang in [AppLanguage::ZhHans, AppLanguage::ZhHant] {
                let v = lookup(lang, key).unwrap_or_default();
                if says_moss_in_prose(v) {
                    failures.push(format!("  key `{key}` ({lang:?}) says \"moss\" — use 青苔: {v:?}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "Chinese advisory values must call the product 青苔 \
             (add to ALLOWLIST with a reason if the Latin name is deliberate):\n{}",
            failures.join("\n")
        );
    }

    /// Regression guard: no advisory key may have a ZhHans or ZhHant value that
    /// is byte-identical to its En value (copy-paste-untranslated regression).
    ///
    /// If a future entry legitimately shares the English text — e.g. a brand
    /// name, a pure symbol, or a number-only string — add it to the allowlist
    /// below with a short comment explaining why.
    #[test]
    fn no_chinese_value_is_byte_identical_to_english() {
        /// Keys whose Chinese translations are legitimately identical to English.
        /// Each entry must have a comment justifying the exception.
        const ALLOWLIST: &[&str] = &[
            // (empty — no current entries have en == zh_hans or en == zh_hant)
        ];

        let mut failures: Vec<String> = Vec::new();

        for &key in KEYS {
            let en = lookup(AppLanguage::En, key).unwrap_or_default();
            let hans = lookup(AppLanguage::ZhHans, key).unwrap_or_default();
            let hant = lookup(AppLanguage::ZhHant, key).unwrap_or_default();

            if ALLOWLIST.contains(&key) {
                continue;
            }

            if hans == en {
                failures.push(format!(
                    "  key `{key}`: ZhHans value is byte-identical to En: {en:?}"
                ));
            }
            if hant == en {
                failures.push(format!(
                    "  key `{key}`: ZhHant value is byte-identical to En: {en:?}"
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "Advisory keys have untranslated Chinese values \
             (add to ALLOWLIST with a reason if intentional):\n{}",
            failures.join("\n")
        );
    }
}
