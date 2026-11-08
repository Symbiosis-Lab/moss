/**
 * The shortcode catalog: what the slash menu, the insert bar and the fence
 * completion offer, how each entry is presented (localized strings), and the
 * one template-insertion path they all share. Data, with one function; the
 * completion flow that reads it is `cm-completion.ts`.
 */

import { snippet } from '@codemirror/autocomplete';
import type { EditorView } from '@codemirror/view';
import { getLocale } from "../i18n/index";
import type { AppLocale } from "../i18n/index";
import type { ExtKind } from '@symbiosis-lab/moss-syntax';
import { SHORTCODES } from '@symbiosis-lab/moss-syntax';

// ---------------------------------------------------------------------------
// Slash-menu / fence-autocomplete localised strings
// Keys are added to UiStringMap by the orchestrator; strings live here so
// the catalog can be built at call time (after locale is known), not at
// module-load time.
// ---------------------------------------------------------------------------

export type SlashKey =
  | 'slash.grid.label'
  | 'slash.grid.detail'
  | 'slash.grid.per_line_hint'
  | 'slash.grid.wide_hint'
  | 'slash.grid.scroll_hint'
  | 'slash.grid.label_hint'
  | 'slash.hero.label'
  | 'slash.hero.detail'
  | 'slash.hero.image_hint'
  | 'slash.hero.wide_hint'
  | 'slash.hero.alt_hint'
  | 'slash.hero.caption_hint'
  | 'slash.hero.header_hint'
  | 'slash.hero.mobile_hint'
  | 'slash.hero.align_hint'
  | 'slash.hero.panel_hint'
  | 'slash.hero.fade_hint'
  | 'slash.hero.max_height_hint'
  | 'slash.hero.dark_hint'
  | 'slash.hero.from_hint'
  | 'slash.gallery.label'
  | 'slash.gallery.detail'
  | 'slash.gallery.per_line_hint'
  | 'slash.buttons.label'
  | 'slash.buttons.detail'
  | 'slash.subscribe.label'
  | 'slash.subscribe.detail'
  | 'slash.subscribe.button_hint'
  | 'slash.subscribe.placeholder_hint'
  | 'slash.recent.label'
  | 'slash.recent.detail'
  | 'slash.recent.count_hint'
  | 'slash.recent.since_hint'
  | 'slash.recent.last_hint'
  | 'slash.form.label'
  | 'slash.form.detail'
  | 'slash.form.next_hint'
  | 'slash.form.button_hint'
  | 'slash.h1.label'
  | 'slash.h1.detail'
  | 'slash.h2.label'
  | 'slash.h2.detail'
  | 'slash.h3.label'
  | 'slash.h3.detail'
  | 'slash.ul.label'
  | 'slash.ul.detail'
  | 'slash.task.label'
  | 'slash.task.detail'
  | 'slash.ol.label'
  | 'slash.ol.detail'
  | 'slash.bq.label'
  | 'slash.bq.detail'
  | 'slash.callout.label'
  | 'slash.callout.detail'
  | 'slash.code.label'
  | 'slash.code.detail'
  | 'slash.math.label'
  | 'slash.math.detail'
  | 'slash.imath.label'
  | 'slash.imath.detail'
  | 'slash.hr.label'
  | 'slash.hr.detail'
  | 'slash.img.label'
  | 'slash.img.detail';

/** The ONE table for slash/insert-bar strings. Exported for the untranslated-
 * leakage guard (`i18n/no-untranslated-leakage.test.ts`), which used to see
 * only `UI_STRINGS` — so the six block-name labels sat untranslated here while
 * the guard reported a clean sweep. */
export const SLASH_STRINGS: Record<AppLocale, Record<SlashKey, string>> = {
  en: {
    'slash.grid.label': 'Grid',
    'slash.grid.detail': 'Multi-column layout for text and images',
    'slash.grid.per_line_hint': 'How many cards fit along one line',
    'slash.grid.wide_hint': 'width token: body | wide | page | screen',
    'slash.grid.scroll_hint': 'Keep cards on one line; readers scroll sideways',
    'slash.grid.label_hint': 'Name for the scrolling row (screen readers)',
    'slash.hero.label': 'Hero',
    'slash.hero.detail': 'Full-width banner with image and text',
    'slash.hero.image_hint': 'path to image file',
    'slash.hero.wide_hint': 'width token: body | wide | page | screen',
    'slash.hero.alt_hint': 'Describes the picture for screen readers',
    'slash.hero.caption_hint': 'Caption shown below the picture, never over it',
    'slash.hero.header_hint': 'over: lay the site header over the picture',
    'slash.hero.mobile_hint': 'overlay: keep the words on the picture on phones',
    'slash.hero.align_hint': 'end: put the words at the other side',
    'slash.hero.panel_hint': 'auto | none | always: a panel behind the words',
    'slash.hero.fade_hint': 'top: fade the picture into the page at its top',
    'slash.hero.max_height_hint': 'height cap: px, rem, vw or clamp()',
    'slash.hero.dark_hint': 'picture shown in dark mode',
    'slash.hero.from_hint': 'folder listing whose first page fills the hero',
    'slash.gallery.label': 'Gallery',
    'slash.gallery.detail': 'Photo grid',
    'slash.gallery.per_line_hint': 'How many photos fit along one line',
    'slash.buttons.label': 'Buttons',
    'slash.buttons.detail': 'Call-to-action button group',
    'slash.subscribe.label': 'Subscribe',
    'slash.subscribe.detail': 'Email signup form',
    'slash.subscribe.button_hint': 'button label text',
    'slash.subscribe.placeholder_hint': 'input placeholder',
    'slash.recent.label': 'Recent',
    'slash.recent.detail': 'List of recent posts',
    'slash.recent.count_hint': 'maximum posts (default 10)',
    'slash.recent.since_hint': 'posts on/after date (YYYY-MM-DD)',
    'slash.recent.last_hint': 'relative window: week | month | Nd',
    'slash.form.label': 'Form',
    'slash.form.detail': 'Request form, one field per line',
    'slash.form.next_hint': 'page shown after sending',
    'slash.form.button_hint': 'button label text',
    'slash.h1.label': 'Heading 1',
    'slash.h1.detail': 'Large section heading',
    'slash.h2.label': 'Heading 2',
    'slash.h2.detail': 'Medium section heading',
    'slash.h3.label': 'Heading 3',
    'slash.h3.detail': 'Small section heading',
    'slash.ul.label': 'Bullet List',
    'slash.ul.detail': 'Unordered list',
    'slash.task.label': 'Task',
    'slash.task.detail': 'Checkbox item',
    'slash.ol.label': 'Numbered List',
    'slash.ol.detail': 'Ordered list',
    'slash.bq.label': 'Quote',
    'slash.bq.detail': 'Block quotation',
    'slash.callout.label': 'Callout',
    'slash.callout.detail': 'Highlighted note box',
    'slash.code.label': 'Code Block',
    'slash.code.detail': 'Fenced code block',
    'slash.math.label': 'Math Block',
    'slash.math.detail': 'Display math equation ($$…$$)',
    'slash.imath.label': 'Inline Math',
    'slash.imath.detail': 'Math within a line ($…$)',
    'slash.hr.label': 'Divider',
    'slash.hr.detail': 'Horizontal rule',
    'slash.img.label': 'Image',
    'slash.img.detail': 'Inline image',
  },
  de: {
    'slash.grid.label': 'Spalten',
    'slash.grid.detail': 'Mehrspaltiges Layout für Text und Bilder',
    'slash.grid.per_line_hint': 'Wie viele Karten in eine Zeile passen',
    'slash.grid.wide_hint': 'Breitenwert: body | wide | page | screen',
    'slash.grid.scroll_hint': 'Karten in einer Zeile halten; Lesende scrollen seitlich',
    'slash.grid.label_hint': 'Name der scrollenden Zeile (für Screenreader)',
    'slash.hero.label': 'Bildbanner',
    'slash.hero.detail': 'Banner in voller Breite mit Bild und Text',
    'slash.hero.image_hint': 'Pfad zur Bilddatei',
    'slash.hero.wide_hint': 'Breitenwert: body | wide | page | screen',
    'slash.hero.alt_hint': 'Beschreibt das Bild für Screenreader',
    'slash.hero.caption_hint': 'Bildunterschrift unter dem Bild, nie darüber',
    'slash.hero.header_hint': 'over: den Seitenkopf über das Bild legen',
    'slash.hero.mobile_hint': 'overlay: den Text auf dem Handy auf dem Bild lassen',
    'slash.hero.align_hint': 'end: den Text auf die andere Seite setzen',
    'slash.hero.panel_hint': 'auto | none | always: eine Fläche hinter dem Text',
    'slash.hero.fade_hint': 'top: das Bild oben in die Seite ausblenden',
    'slash.hero.max_height_hint': 'Höhengrenze: px, rem, vw oder clamp()',
    'slash.hero.dark_hint': 'Bild im dunklen Modus',
    'slash.hero.from_hint': 'Ordnerliste, deren erste Seite das Banner füllt',
    'slash.gallery.label': 'Galerie',
    'slash.gallery.detail': 'Galerie als Bilderraster',
    'slash.gallery.per_line_hint': 'Wie viele Fotos in eine Zeile passen',
    'slash.buttons.label': 'Buttons',
    'slash.buttons.detail': 'Gruppe von Handlungsaufforderungs-Buttons',
    'slash.subscribe.label': 'Abonnieren',
    'slash.subscribe.detail': 'Formular zur E-Mail-Anmeldung',
    'slash.subscribe.button_hint': 'Beschriftung des Buttons',
    'slash.subscribe.placeholder_hint': 'Platzhalter im Eingabefeld',
    'slash.recent.label': 'Neueste Beiträge',
    'slash.recent.detail': 'Liste der neuesten Beiträge',
    'slash.recent.count_hint': 'Höchstzahl der Beiträge (Standard 10)',
    'slash.recent.since_hint': 'Beiträge ab diesem Datum (JJJJ-MM-TT)',
    'slash.recent.last_hint': 'Relativer Zeitraum: week | month | Nd',
    'slash.form.label': 'Formular',
    'slash.form.detail': 'Anfrageformular, ein Feld pro Zeile',
    'slash.form.next_hint': 'Seite nach dem Absenden',
    'slash.form.button_hint': 'Beschriftung des Buttons',
    'slash.h1.label': 'Überschrift 1',
    'slash.h1.detail': 'Große Abschnittsüberschrift',
    'slash.h2.label': 'Überschrift 2',
    'slash.h2.detail': 'Mittlere Abschnittsüberschrift',
    'slash.h3.label': 'Überschrift 3',
    'slash.h3.detail': 'Kleine Abschnittsüberschrift',
    'slash.ul.label': 'Aufzählung',
    'slash.ul.detail': 'Ungeordnete Liste',
    'slash.task.label': 'Aufgabe',
    'slash.task.detail': 'Element mit Kontrollkästchen',
    'slash.ol.label': 'Nummerierte Liste',
    'slash.ol.detail': 'Geordnete Liste',
    'slash.bq.label': 'Zitat',
    'slash.bq.detail': 'Zitatblock',
    'slash.callout.label': 'Hinweisbox',
    'slash.callout.detail': 'Hervorgehobener Hinweis',
    'slash.code.label': 'Codeblock',
    'slash.code.detail': 'Umzäunter Codeblock',
    'slash.math.label': 'Formelblock',
    'slash.math.detail': 'Abgesetzte Formel ($$…$$)',
    'slash.imath.label': 'Formel im Text',
    'slash.imath.detail': 'Formel innerhalb einer Zeile ($…$)',
    'slash.hr.label': 'Trennlinie',
    'slash.hr.detail': 'Horizontale Linie',
    'slash.img.label': 'Bild',
    'slash.img.detail': 'Bild im Text',
  },
  'zh-hans': {
    'slash.grid.label': '分栏',
    'slash.grid.detail': '多列文字与图片布局',
    'slash.grid.per_line_hint': '一行放几张卡片',
    'slash.grid.wide_hint': '宽度模式：body | wide | page | screen',
    'slash.grid.scroll_hint': '让卡片排成一行，读者左右滑动查看',
    'slash.grid.label_hint': '这一行的名称（供屏幕阅读器使用）',
    'slash.hero.label': '首图横幅',
    'slash.hero.detail': '带图片和文字的全宽横幅',
    'slash.hero.image_hint': '图片文件路径',
    'slash.hero.wide_hint': '宽度模式：body | wide | page | screen',
    'slash.hero.alt_hint': '为读屏软件描述这张图',
    'slash.hero.caption_hint': '显示在图片下方的说明，不压在图上',
    'slash.hero.header_hint': 'over：网站页头叠在图片上',
    'slash.hero.mobile_hint': 'overlay：手机上文字仍叠在图片上',
    'slash.hero.align_hint': 'end：文字放到另一侧',
    'slash.hero.panel_hint': 'auto | none | always：文字背后的底板',
    'slash.hero.fade_hint': 'top：让图片顶部淡入页面',
    'slash.hero.max_height_hint': '高度上限：px、rem、vw 或 clamp()',
    'slash.hero.dark_hint': '深色模式下显示的图片',
    'slash.hero.from_hint': '以其第一页填充横幅的文件夹列表',
    'slash.gallery.label': '相册',
    'slash.gallery.detail': '图片网格相册',
    'slash.gallery.per_line_hint': '一行放几张照片',
    'slash.buttons.label': '按钮组',
    'slash.buttons.detail': '行动号召按钮组',
    'slash.subscribe.label': '订阅',
    'slash.subscribe.detail': '邮件订阅表单',
    'slash.subscribe.button_hint': '按钮文字',
    'slash.subscribe.placeholder_hint': '输入框占位文字',
    'slash.recent.label': '最新文章',
    'slash.recent.detail': '最新文章列表',
    'slash.recent.count_hint': '最多显示文章数（默认 10）',
    'slash.recent.since_hint': '显示此日期及之后的文章（YYYY-MM-DD）',
    'slash.recent.last_hint': '相对时间范围：week | month | Nd',
    'slash.form.label': '表单',
    'slash.form.detail': '申请表单，每行一个字段',
    'slash.form.next_hint': '提交后显示的页面',
    'slash.form.button_hint': '按钮文字',
    'slash.h1.label': '一级标题',
    'slash.h1.detail': '大号章节标题',
    'slash.h2.label': '二级标题',
    'slash.h2.detail': '中号章节标题',
    'slash.h3.label': '三级标题',
    'slash.h3.detail': '小号章节标题',
    'slash.ul.label': '项目符号列表',
    'slash.ul.detail': '无序列表',
    'slash.task.label': '任务',
    'slash.task.detail': '待办事项',
    'slash.ol.label': '编号列表',
    'slash.ol.detail': '有序列表',
    'slash.bq.label': '引用',
    'slash.bq.detail': '块引用',
    'slash.callout.label': '标注',
    'slash.callout.detail': '高亮提示框',
    'slash.code.label': '代码块',
    'slash.code.detail': '围栏代码块',
    'slash.math.label': '公式块',
    'slash.math.detail': '块级数学公式（$$…$$）',
    'slash.imath.label': '行内公式',
    'slash.imath.detail': '行内数学公式（$…$）',
    'slash.hr.label': '分隔线',
    'slash.hr.detail': '水平分割线',
    'slash.img.label': '图片',
    'slash.img.detail': '行内图片',
  },
  'zh-hant': {
    'slash.grid.label': '分欄',
    'slash.grid.detail': '多欄文字與圖片佈局',
    'slash.grid.per_line_hint': '一行放幾張卡片',
    'slash.grid.wide_hint': '寬度模式：body | wide | page | screen',
    'slash.grid.scroll_hint': '讓卡片排成一行，讀者左右滑動查看',
    'slash.grid.label_hint': '這一行的名稱（供螢幕閱讀器使用）',
    'slash.hero.label': '首圖橫幅',
    'slash.hero.detail': '帶圖片和文字的全寬橫幅',
    'slash.hero.image_hint': '圖片檔案路徑',
    'slash.hero.wide_hint': '寬度模式：body | wide | page | screen',
    'slash.hero.alt_hint': '為讀屏軟體描述這張圖',
    'slash.hero.caption_hint': '顯示在圖片下方的說明，不壓在圖上',
    'slash.hero.header_hint': 'over：網站頁首疊在圖片上',
    'slash.hero.mobile_hint': 'overlay：手機上文字仍疊在圖片上',
    'slash.hero.align_hint': 'end：文字放到另一側',
    'slash.hero.panel_hint': 'auto | none | always：文字背後的底板',
    'slash.hero.fade_hint': 'top：讓圖片頂部淡入頁面',
    'slash.hero.max_height_hint': '高度上限：px、rem、vw 或 clamp()',
    'slash.hero.dark_hint': '深色模式下顯示的圖片',
    'slash.hero.from_hint': '以其第一頁填充橫幅的資料夾列表',
    'slash.gallery.label': '相簿',
    'slash.gallery.detail': '圖片網格相簿',
    'slash.gallery.per_line_hint': '一行放幾張照片',
    'slash.buttons.label': '按鈕組',
    'slash.buttons.detail': '行動呼籲按鈕組',
    'slash.subscribe.label': '訂閱',
    'slash.subscribe.detail': '電子郵件訂閱表單',
    'slash.subscribe.button_hint': '按鈕文字',
    'slash.subscribe.placeholder_hint': '輸入框佔位文字',
    'slash.recent.label': '最新文章',
    'slash.recent.detail': '最新文章列表',
    'slash.recent.count_hint': '最多顯示文章數（預設 10）',
    'slash.recent.since_hint': '顯示此日期及之後的文章（YYYY-MM-DD）',
    'slash.recent.last_hint': '相對時間範圍：week | month | Nd',
    'slash.form.label': '表單',
    'slash.form.detail': '申請表單，每行一個欄位',
    'slash.form.next_hint': '送出後顯示的頁面',
    'slash.form.button_hint': '按鈕文字',
    'slash.h1.label': '一級標題',
    'slash.h1.detail': '大號章節標題',
    'slash.h2.label': '二級標題',
    'slash.h2.detail': '中號章節標題',
    'slash.h3.label': '三級標題',
    'slash.h3.detail': '小號章節標題',
    'slash.ul.label': '項目符號列表',
    'slash.ul.detail': '無序列表',
    'slash.task.label': '任務',
    'slash.task.detail': '待辦事項',
    'slash.ol.label': '編號列表',
    'slash.ol.detail': '有序列表',
    'slash.bq.label': '引用',
    'slash.bq.detail': '塊引用',
    'slash.callout.label': '標註',
    'slash.callout.detail': '高亮提示框',
    'slash.code.label': '代碼塊',
    'slash.code.detail': '圍欄代碼塊',
    'slash.math.label': '公式塊',
    'slash.math.detail': '塊級數學公式（$$…$$）',
    'slash.imath.label': '行內公式',
    'slash.imath.detail': '行內數學公式（$…$）',
    'slash.hr.label': '分隔線',
    'slash.hr.detail': '水平分割線',
    'slash.img.label': '圖片',
    'slash.img.detail': '行內圖片',
  },
};

/**
 * Resolve a slash-menu string for the current locale, with en fallback.
 * Exported so cm-slash-menu.ts can resolve label/detail at render time.
 */
export function ts(key: SlashKey): string {
  const locale = getLocale();
  return SLASH_STRINGS[locale]?.[key] ?? SLASH_STRINGS.en[key];
}

export interface ShortcodeSpec {
  kind: 'shortcode';
  name: string;
  labelKey: SlashKey;
  detailKey: SlashKey;
  /**
   * CM6 snippet template (@codemirror/autocomplete `snippet()` syntax):
   * `${n:placeholder}` fields are tab stops — the first is selected on
   * insert, Tab/Shift-Tab navigate, Escape ends the snippet. This is the
   * ONE insertion grammar for both the fence autocomplete and the slash
   * menu (applyShortcodeTemplate).
   */
  template: string;
  /**
   * `assetKinds` on an attr entry marks it as an asset-search attribute: when
   * the cursor is in that attr's VALUE (not its name), the completion source
   * dispatches an RPC asset search scoped to these `ExtKind`s instead of
   * offering no completions (the default for attrs without `assetKinds`,
   * e.g. `per-line=`/`count=`/`button=`).
   */
  attrs: { name: string; hintKey: SlashKey; assetKinds?: ExtKind[]; flag?: boolean }[];
  /**
   * When set, cursor positions inside this shortcode's BODY (between the
   * open fence and its close `:::`) that land on an asset-path span dispatch
   * an RPC asset search scoped to these kinds. Only `gallery` sets this today
   * — its body grammar is `path` / `![alt](path)`, optionally suffixed with
   * `|attrs` (see `parseGalleryBodyLine`).
   */
  bodyAssetKinds?: ExtKind[];
}

/**
 * Block command spec — plain dispatch insertion (no snippet tab stops).
 * apply() receives the absolute doc offsets of the "/" and the end of the
 * typed query so it can clean them up, then inserts the block syntax.
 */
export interface BlockCommandSpec {
  kind: 'block';
  name: string;
  labelKey: SlashKey;
  detailKey: SlashKey;
  apply(view: EditorView, triggerFrom: number, triggerTo: number): void;
}

export type SlashCommandSpec = ShortcodeSpec | BlockCommandSpec;

/**
 * Helper: replace [triggerFrom, triggerTo] with a line-prefix at line.from.
 * Used by heading / list / quote block commands.
 *
 * Strategy: replace [line.from, triggerTo] with prefix + text-before-slash.
 * This is a single, non-overlapping range change, so CM6 accepts it cleanly.
 */
function applyLinePrefix(
  view: EditorView,
  triggerFrom: number,
  triggerTo: number,
  prefix: string,
): void {
  const line = view.state.doc.lineAt(triggerFrom);
  const textBeforeSlash = line.text.slice(0, triggerFrom - line.from);
  const insert = prefix + textBeforeSlash;
  view.dispatch({
    changes: { from: line.from, to: triggerTo, insert },
    selection: { anchor: line.from + prefix.length },
  });
}

export const BLOCK_COMMAND_CATALOG: BlockCommandSpec[] = [
  {
    kind: 'block', name: 'h1', labelKey: 'slash.h1.label', detailKey: 'slash.h1.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '# '); },
  },
  {
    kind: 'block', name: 'h2', labelKey: 'slash.h2.label', detailKey: 'slash.h2.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '## '); },
  },
  {
    kind: 'block', name: 'h3', labelKey: 'slash.h3.label', detailKey: 'slash.h3.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '### '); },
  },
  {
    kind: 'block', name: 'ul', labelKey: 'slash.ul.label', detailKey: 'slash.ul.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '- '); },
  },
  {
    kind: 'block', name: 'task', labelKey: 'slash.task.label', detailKey: 'slash.task.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '- [ ] '); },
  },
  {
    kind: 'block', name: 'ol', labelKey: 'slash.ol.label', detailKey: 'slash.ol.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '1. '); },
  },
  {
    kind: 'block', name: 'bq', labelKey: 'slash.bq.label', detailKey: 'slash.bq.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '> '); },
  },
  {
    // `> [!type]` callouts (cm-callout.ts) rendered but were reachable only by
    // prior Obsidian knowledge — an inventory gap. The line-prefix insert lands
    // the cursor after "> [!note] ", ready for the callout text; the type word
    // is plain text the author can retype (tip/warning/…).
    kind: 'block', name: 'callout', labelKey: 'slash.callout.label', detailKey: 'slash.callout.detail',
    apply(view, triggerFrom, triggerTo) { applyLinePrefix(view, triggerFrom, triggerTo, '> [!note] '); },
  },
  {
    kind: 'block', name: 'code', labelKey: 'slash.code.label', detailKey: 'slash.code.detail',
    apply(view, triggerFrom, triggerTo) {
      const insert = '```\n\n```';
      view.dispatch({
        changes: { from: triggerFrom, to: triggerTo, insert },
        // Cursor lands on the blank middle line (offset +4 from triggerFrom)
        selection: { anchor: triggerFrom + 4 },
      });
    },
  },
  {
    kind: 'block', name: 'math', labelKey: 'slash.math.label', detailKey: 'slash.math.detail',
    apply(view, triggerFrom, triggerTo) {
      // Display math, mirroring the code-fence insert: `$$` / blank line / `$$`
      // with the cursor on the middle line. Highlight-only in the editor — the
      // preview typesets it.
      const insert = '$$\n\n$$';
      view.dispatch({
        changes: { from: triggerFrom, to: triggerTo, insert },
        // Cursor lands on the blank middle line (offset +3 from triggerFrom)
        selection: { anchor: triggerFrom + 3 },
      });
    },
  },
  {
    kind: 'block', name: 'imath', labelKey: 'slash.imath.label', detailKey: 'slash.imath.detail',
    apply(view, triggerFrom, triggerTo) {
      // Inline math: `$$` with the cursor between the dollars → typing yields
      // `$x$`. NOTE the empty pair alone parses as an (empty) DisplayMath until
      // a character lands between — harmless for a transient caret state.
      const insert = '$$';
      view.dispatch({
        changes: { from: triggerFrom, to: triggerTo, insert },
        selection: { anchor: triggerFrom + 1 },
      });
    },
  },
  {
    kind: 'block', name: 'hr', labelKey: 'slash.hr.label', detailKey: 'slash.hr.detail',
    apply(view, triggerFrom, _triggerTo) {
      const line = view.state.doc.lineAt(triggerFrom);
      // Replace the current line content with "---".
      // After the change, cursor goes to the next line (line.from + 3 + 1 if there
      // is a trailing newline, i.e. line.to < doc.length; else end of doc).
      const hasNextLine = line.to < view.state.doc.length;
      view.dispatch({
        changes: { from: line.from, to: line.to, insert: '---' },
        selection: { anchor: hasNextLine ? line.from + 4 : line.from + 3 },
      });
    },
  },
  {
    kind: 'block', name: 'img', labelKey: 'slash.img.label', detailKey: 'slash.img.detail',
    apply(view, triggerFrom, triggerTo) {
      // The standard form; the caret lands in the destination, where the
      // completion writes the path from the page.
      const insert = '![]()';
      view.dispatch({
        changes: { from: triggerFrom, to: triggerTo, insert },
        // Cursor between the parentheses (offset +4)
        selection: { anchor: triggerFrom + 4 },
      });
    },
  },
];

/**
 * App-side presentation overlay for the generated catalog: which authorable
 * shortcodes appear in the slash menu (in this order), their i18n keys, and
 * `bodyAssetKinds` (an editor-search concern, not yet in the artifact).
 * STRUCTURE — attr lists, asset kinds, templates — comes from
 * the generated catalog in `@symbiosis-lab/moss-syntax` (Rust SSOT); only presentation lives here.
 * `cm-completion.test.ts` asserts this overlay covers every
 * authorable catalog entry and names every generated attr.
 */
const SHORTCODE_PRESENTATION: Record<
  string,
  {
    labelKey: SlashKey;
    detailKey: SlashKey;
    attrHints: Record<string, SlashKey>;
    bodyAssetKinds?: ExtKind[];
  }
> = {
  grid: {
    labelKey: 'slash.grid.label', detailKey: 'slash.grid.detail',
    attrHints: {
      'per-line': 'slash.grid.per_line_hint',
      wide: 'slash.grid.wide_hint',
      page: 'slash.grid.wide_hint',
      screen: 'slash.grid.wide_hint',
      scroll: 'slash.grid.scroll_hint',
      label: 'slash.grid.label_hint',
    },
  },
  hero: {
    labelKey: 'slash.hero.label', detailKey: 'slash.hero.detail',
    attrHints: {
      image: 'slash.hero.image_hint',
      wide: 'slash.hero.wide_hint',
      page: 'slash.hero.wide_hint',
      screen: 'slash.hero.wide_hint',
      alt: 'slash.hero.alt_hint',
      caption: 'slash.hero.caption_hint',
      header: 'slash.hero.header_hint',
      mobile: 'slash.hero.mobile_hint',
      align: 'slash.hero.align_hint',
      panel: 'slash.hero.panel_hint',
      fade: 'slash.hero.fade_hint',
      'max-height': 'slash.hero.max_height_hint',
      dark: 'slash.hero.dark_hint',
      from: 'slash.hero.from_hint',
    },
  },
  gallery: {
    labelKey: 'slash.gallery.label', detailKey: 'slash.gallery.detail',
    attrHints: {
      'per-line': 'slash.gallery.per_line_hint',
      wide: 'slash.grid.wide_hint',
      page: 'slash.grid.wide_hint',
      screen: 'slash.grid.wide_hint',
    },
    bodyAssetKinds: ['Image'],
  },
  buttons: {
    labelKey: 'slash.buttons.label', detailKey: 'slash.buttons.detail',
    attrHints: {},
  },
  subscribe: {
    labelKey: 'slash.subscribe.label', detailKey: 'slash.subscribe.detail',
    attrHints: {
      button: 'slash.subscribe.button_hint',
      placeholder: 'slash.subscribe.placeholder_hint',
    },
  },
  recent: {
    labelKey: 'slash.recent.label', detailKey: 'slash.recent.detail',
    attrHints: {
      count: 'slash.recent.count_hint',
      since: 'slash.recent.since_hint',
      last: 'slash.recent.last_hint',
    },
  },
  form: {
    labelKey: 'slash.form.label', detailKey: 'slash.form.detail',
    attrHints: {
      next: 'slash.form.next_hint',
      button: 'slash.form.button_hint',
    },
  },
};

export const SHORTCODE_CATALOG: ShortcodeSpec[] = Object.entries(SHORTCODE_PRESENTATION).map(
  ([name, pres]) => {
    const info = SHORTCODES.find((s) => s.name === name && s.authorable);
    if (!info) {
      // Presentation for a shortcode Rust no longer offers — regenerate the
      // artifact and prune the overlay (the test fails before this can ship).
      throw new Error(`SHORTCODE_PRESENTATION names unknown/hidden shortcode "${name}"`);
    }
    return {
      kind: 'shortcode' as const,
      name,
      labelKey: pres.labelKey,
      detailKey: pres.detailKey,
      template: info.canonicalTemplate,
      attrs: info.attrs.map((a) => ({
        name: a.name,
        // A generated attr the overlay doesn't hint would render an empty
        // hint; the totality test catches it before it ships.
        hintKey: pres.attrHints[a.name],
        // Kinds are emitted from Rust's `ExtKind` (`{:?}` of the same enum
        // the local `ExtKind` union mirrors), so the cast is sound.
        ...(a.assetKinds ? { assetKinds: [...a.assetKinds] as ExtKind[] } : {}),
        ...(a.flag ? { flag: true as const } : {}),
      })),
      ...(pres.bodyAssetKinds ? { bodyAssetKinds: pres.bodyAssetKinds } : {}),
    };
  },
);

/** Combined catalog: block commands first, then shortcodes. */
export const SLASH_CATALOG: SlashCommandSpec[] = [
  ...BLOCK_COMMAND_CATALOG,
  ...SHORTCODE_CATALOG,
];

/**
 * THE one shortcode insertion path — both the fence autocomplete (here) and
 * the slash menu (cm-slash-menu.ts) route through this. Expands the spec's
 * tab-stop template via @codemirror/autocomplete `snippet()`: the first
 * placeholder is selected on insert, Tab/Shift-Tab navigate fields.
 *
 * `fencePrefix` prepends `:::` for call sites that replace a range NOT
 * already starting with the fence (the slash menu replaces `/query`).
 */
export function applyShortcodeTemplate(
  view: EditorView,
  spec: ShortcodeSpec,
  from: number,
  to: number,
  opts: { fencePrefix?: boolean } = {},
): void {
  const tpl = (opts.fencePrefix ? ':::' : '') + spec.template;
  // CM6 snippet only needs `label` for the annotation; use the raw name
  // (never translated) so snippet insertion is locale-independent.
  snippet(tpl)(view, { label: spec.name }, from, to);
}
