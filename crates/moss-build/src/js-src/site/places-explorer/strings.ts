/**
 * strings.ts — the places explorer's own runtime copy.
 *
 * Bucketed the same way every other site runtime localises itself
 * (`subscribe/i18n.ts`'s `langBucket`/`Lang` — the blessed TS twin of Rust's
 * `Language::from_bcp47_lenient`): three buckets, read off `<html lang>`.
 * No per-site override exists for this landing, the same posture
 * `subscribe/i18n.ts` itself takes.
 */
import { langBucket, type Lang } from "../subscribe/i18n";

export interface PlacesStrings {
  /** `aria-label` on the pannable map region. */
  map: string;
  /** `aria-label` on the controls group (the zoom capsule plus the reset circle). */
  mapControls: string;
  zoomIn: string;
  zoomOut: string;
  /** `aria-label` on the reset-to-cover control. */
  reset: string;
  /** Untitled work fallback, same role as a card's own title fallback elsewhere. */
  untitled: string;
  /** `{n}` placeholder, replaced with a formatted count — a cluster marker's own label. */
  worksHere: string;
  /** Live-region announcement once a ring opens, scoping the row. */
  ringOpened: string;
  /** Live-region announcement once the scope returns to every work. */
  scopeCleared: string;
  /** `aria-label` on the works row. */
  worksInView: string;
  /** Link text to a work's own article. */
  readArticle: string;
  /** The scope chip's root crumb — every other crumb widens toward this. */
  chipAll: string;
  /** The scope chip's "this article" segment (the article scope's own crumb). */
  chipThisArticle: string;
  /** `aria-label` on the "This article | All articles" switch's group. */
  chipScopeGroup: string;
  /** Live-region text once the switch shows only the current article. */
  scopeThisArticle: string;
  /** Live-region text once the switch shows every article. */
  scopeAllArticles: string;
  /** `aria-label` on the phone-width "…" button that reveals the trail's collapsed middle crumbs in place. */
  chipShowHidden: string;
  /** `{name}` placeholder, filled with the embed's own place or article display name (`data-embed-name`, Rust-emitted, never translated — it is a proper noun) — the `title` on a `style:map`/locator embed's lazily-hydrated iframe (`embed.ts`'s `buildIframe`). */
  mapEmbedTitle: string;
  /** Visible loading status while the interactive embed boots. */
  mapEmbedLoading: string;
  /** Status before a lazy embed enters the viewport. */
  mapEmbedWaiting: string;
  /** Accessible opt-in button on Save-Data / slow connections. */
  mapEmbedLoad: string;
  /** Between the names of several authors on a card; the author-to-date separator is a fixed middle dot instead (`cards.ts`). */
  listSeparator: string;
}

const STRINGS: Record<Lang, PlacesStrings> = {
  en: {
    map: "Map. Use arrow keys to pan, plus and minus to zoom.",
    mapControls: "Map controls",
    zoomIn: "Zoom in",
    zoomOut: "Zoom out",
    reset: "Fit all places",
    untitled: "Untitled",
    worksHere: "{n} works here",
    ringOpened: "Showing works for this place.",
    scopeCleared: "Showing all works again.",
    worksInView: "Works in view",
    readArticle: "Read article",
    chipAll: "All articles",
    chipThisArticle: "This article",
    chipScopeGroup: "Which articles to show",
    scopeThisArticle: "Showing only this article.",
    scopeAllArticles: "Showing all articles.",
    chipShowHidden: "Show hidden places",
    mapEmbedTitle: "Map: {name}",
    mapEmbedLoading: "Loading interactive map…",
    mapEmbedWaiting: "The map loads when it is near the screen.",
    mapEmbedLoad: "Load interactive map",
    listSeparator: ", ",
  },
  "zh-hans": {
    map: "地图。使用方向键平移，加号及减号缩放。",
    mapControls: "地图控制",
    zoomIn: "放大地图",
    zoomOut: "缩小地图",
    reset: "显示全部地点",
    untitled: "未命名作品",
    worksHere: "此处 {n} 篇",
    ringOpened: "已显示此地点的作品。",
    scopeCleared: "已显示全部作品。",
    worksInView: "视野中的作品",
    readArticle: "阅读原文",
    chipAll: "全部文章",
    chipThisArticle: "本文",
    chipScopeGroup: "显示哪些文章",
    scopeThisArticle: "只显示本文。",
    scopeAllArticles: "已显示全部文章。",
    chipShowHidden: "显示隐藏地点",
    mapEmbedTitle: "地图：{name}",
    mapEmbedLoading: "正在加载互动地图…",
    mapEmbedWaiting: "地图进入视野后会自动加载。",
    mapEmbedLoad: "加载互动地图",
    listSeparator: "、",
  },
  "zh-hant": {
    map: "地圖。使用方向鍵平移，加號及減號縮放。",
    mapControls: "地圖控制",
    zoomIn: "放大地圖",
    zoomOut: "縮小地圖",
    reset: "顯示全部地點",
    untitled: "未命名作品",
    worksHere: "此處 {n} 篇",
    ringOpened: "已顯示此地點的作品。",
    scopeCleared: "已顯示全部作品。",
    worksInView: "視野中的作品",
    readArticle: "閱讀原文",
    chipAll: "全部文章",
    chipThisArticle: "本文",
    chipScopeGroup: "顯示哪些文章",
    scopeThisArticle: "只顯示本文。",
    scopeAllArticles: "已顯示全部文章。",
    chipShowHidden: "顯示隱藏地點",
    mapEmbedTitle: "地圖：{name}",
    mapEmbedLoading: "正在載入互動地圖…",
    mapEmbedWaiting: "地圖進入視野後會自動載入。",
    mapEmbedLoad: "載入互動地圖",
    listSeparator: "、",
  },
};

/** This document's own copy bucket, read once per boot — `<html lang>` never changes mid-session. */
export function copyFor(lang: string | null | undefined): PlacesStrings {
  return STRINGS[langBucket(lang)];
}

/** `worksHere`'s `{n}` filled with a locale-formatted count. */
export function worksHereLabel(strings: PlacesStrings, lang: string | null | undefined, count: number): string {
  const formatted = new Intl.NumberFormat(lang ?? undefined).format(count);
  return strings.worksHere.replace("{n}", formatted);
}
