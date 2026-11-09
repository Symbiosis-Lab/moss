// The cloud download half of the full-window screen: what its meter reads,
// the numbers under it, and the tip that ends the wait for good.
//
// The screen itself is screen/surface.ts, its markup is screen/shell.ts and
// its grid alignment is screen/overlay-slot.ts — a cloud wait looks exactly
// like every other wait, so everything that is not specific to a sync provider
// lives there. What is left here is the two-phase rule below, the two vendor
// tables, and `cloudWaitSpec` handing the surface four values.
//
// ## The wait has two phases
//
// The first phase is the download: files are still arriving, and the meter measures
// them. The second phase is what is left once `remaining` hits zero and there is still
// no site to show — the build itself. The screen says so ("Building your
// site") rather than holding a 100%-downloaded count up while it blocks,
// which would be its own kind of dishonesty.
//
// ## The gate holds
//
// There is no "show the site anyway" link — showing the site before
// `home_ready` would be moss offering a 404 where the home page belongs. The
// gate holds until the requested page is usable or genuinely missing
// (`home_ready`, emitted from the build or its selected sealed output). The
// way out is the tip: pin the folder so
// the provider stops evicting it. The way past is "Choose a different
// folder", which is a real destination rather than a broken version of this
// one.
//
// There is likewise no timed-out variant. A deadline would be a guess — a
// download can resume at minute four.
//
// `stalled` is an observation, not a verdict: the supervisor has seen nothing
// arrive across the whole pending set for 90 seconds. It keeps trying, and an
// arrival takes the screen straight back to `downloading`.
//
// ## Why the meter looks like that
//
// The progress meter is a lit run of one *real* horizontal grid line: the
// screen's axis is the multiple of `GRID_SPACING` nearest the middle, which is
// exactly where `blueprint-grid` strokes a horizontal line, and the meter's
// ends sit on two vertical ones (style.css, `.moss-waiting-screen`).
// The ambient line therefore continues past the meter to both window edges,
// the crossing glow travels that whole width on the grid's own terms, and the
// fill's leading edge fades over 24px because the grid never ends a line
// abruptly — it fades along it.
//
// Every wait therefore draws a meter, determinate or not: a build screen
// without one shows no grid line at all and reads as a different design.

import { getLocale, t } from "../../i18n/index";
import type { AppLocale } from "../../i18n/ui-strings";
import type { MeterMode, ScreenFraction } from "./shell";

// ── Other apps' words, not moss's ──────────────────────────────────────────
//
// The tip below tells the user how to skip the wait next time: pin the folder
// so the provider stops evicting it. That is only useful if it quotes the menu
// item they will actually see, so every string in these two tables is the
// vendor's own — taken verbatim from that vendor's localized documentation,
// never translated from the English.
//
// **Do not "improve" these.** A better-sounding Chinese phrase that no menu
// contains is worse than no tip at all: the user searches for it, doesn't find
// it, and concludes moss is confused. If a vendor renames the item, replace the
// string; don't paraphrase it.
//
// The two languages agree in practice because both sides read the OS: Finder
// and the provider's Finder extension render in the system locale, and moss's
// default locale comes from `sys_locale::get_locale()`. A user
// who has overridden moss's language in App Settings can see a tip in one
// language quoting a menu that renders in another — narrow, and still better
// than omitting the tip.
//
// Keys are `CloudProvider::id()` from crates/moss-build/src/build/cloud_provider.rs,
// carried on the `icloud-sync` event's `provider` field.

/**
 * The menu item that pins a folder so the provider keeps its bytes on disk.
 *
 * All five reach it the same way — right-click the folder in Finder — but each
 * names it differently, and the Chinese names are not translations of the
 * English ones. Sources: Apple support HT210401, Dropbox "smart sync", Google
 * Drive for desktop, and Microsoft "Files On-Demand" help pages, each read in
 * the corresponding locale.
 *
 * Box is the exception: no localized Box Drive documentation exists, so every
 * non-English cell holds the English string.
 */
const PIN_ACTION: Record<string, Record<AppLocale, string>> = {
  icloud: { en: "Keep Downloaded", de: "Geladene Objekte behalten", "zh-hans": "保留下载", "zh-hant": "保留已下載項目" },
  dropbox: { en: "Make available offline", de: "Offline verfügbar machen", "zh-hans": "设为可离线访问", "zh-hant": "設為離線存取" },
  google_drive: { en: "Available offline", de: "Offline verfügbar", "zh-hans": "可离线使用", "zh-hant": "可離線存取" },
  onedrive: {
    en: "Always Keep on This Device",
    de: "Immer auf diesem Gerät behalten",
    "zh-hans": "始终保留在此设备上",
    "zh-hant": "永遠保留在此裝置上",
  },
  box: {
    en: "Always keep on device",
    de: "Always keep on device",
    "zh-hans": "Always keep on device",
    "zh-hant": "Always keep on device",
  },
};

/**
 * What the provider calls itself in the user's language.
 *
 * Not the backend's `provider_label`, which is an English identifier for logs
 * and generic status text ("iCloud"). The tip names the *product the menu lives
 * in*, so it uses the product name a Chinese-language Mac actually shows.
 */
const PROVIDER_NAME: Record<string, Record<AppLocale, string>> = {
  icloud: { en: "iCloud Drive", de: "iCloud Drive", "zh-hans": "iCloud 云盘", "zh-hant": "iCloud 雲碟" },
  dropbox: { en: "Dropbox", de: "Dropbox", "zh-hans": "Dropbox", "zh-hant": "Dropbox" },
  google_drive: { en: "Google Drive", de: "Google Drive", "zh-hans": "Google 云端硬盘", "zh-hant": "Google 雲端硬碟" },
  onedrive: { en: "OneDrive", de: "OneDrive", "zh-hans": "OneDrive", "zh-hant": "OneDrive" },
  box: { en: "Box", de: "Box", "zh-hans": "Box", "zh-hant": "Box" },
};

/** What the backend told us about the sync provider, if anything. */
export interface WaitingProvider {
  /** `CloudProvider::id()` — "icloud", "dropbox", … or "unknown". */
  id?: string;
  /** `CloudProvider::display_name()` — "iCloud", "Dropbox", … or "cloud". */
  label?: string;
}

/**
 * What to call the provider on screen.
 *
 * Prefers the localized product name keyed off the id, so a Chinese UI says
 * "iCloud 云盘" rather than leaving the backend's English label untranslated.
 * Falls back to that label, and to nothing at all when the backend could not
 * identify a provider — "cloud" as a name reads as a placeholder, which is
 * worse than no name.
 */
function providerName(provider?: WaitingProvider): string | undefined {
  const localized = provider?.id ? PROVIDER_NAME[provider.id]?.[getLocale()] : undefined;
  if (localized) return localized;
  const label = provider?.label;
  return label && label !== "cloud" ? label : undefined;
}

/**
 * Nothing left to download, and still no site to show.
 *
 * The wait has two phases under a monotonic gate, and a reproduction
 * walked straight through the second — downloads drained to zero
 * while builds still took 9.31s each. A screen that keeps saying
 * "553 of 553 downloaded" while it blocks would be dishonest, so the screen counts the
 * build instead. `downloaded >= total` is `remaining === 0`
 * restated in the two numbers this module already receives.
 *
 * `total === 0` is NOT this: it means the eviction counts are unavailable
 * (non-macOS), which is an unknown download state, not a finished one.
 */
function isBuilding(downloaded: number, total: number): boolean {
  return total > 0 && downloaded >= total;
}

/** Is there a fraction worth drawing? False while building and when counts are absent. */
function countsKnown(downloaded: number, total: number): boolean {
  return total > 0 && !isBuilding(downloaded, total);
}

/**
 * One line under the meter carrying both facts: who is sending, and how far in.
 *
 * Three shapes, because either fact can be missing. Merged into one line
 * rather than stacked as two, so the screen has a single centre of gravity.
 */
function metaText(downloaded: number, total: number, provider?: WaitingProvider): string {
  // The second phase says what is happening instead of who is sending: there is nothing
  // left to download, and naming the provider again would describe finished work.
  if (isBuilding(downloaded, total)) return t("preview.waiting_building");
  const name = providerName(provider);
  if (name && total > 0) {
    return t("preview.waiting_meta", {
      provider: name,
      downloaded: String(downloaded),
      total: String(total),
    });
  }
  if (name) return t("preview.waiting_from_provider", { provider: name });
  if (total > 0) {
    return t("preview.waiting_progress", { downloaded: String(downloaded), total: String(total) });
  }
  return "";
}

/**
 * The one thing the user can do to never see this screen again.
 *
 * Pinning has no public API — an undocumented `com.apple.fileprovider.pinned`
 * xattr, unverified to write ourselves — so this is guidance, never
 * automation. Omitted when the provider is unidentified: the whole value of
 * the tip is naming a menu item that exists, and we can't name one for a
 * provider we can't name.
 */
function pinTip(provider?: WaitingProvider): string {
  const locale = getLocale();
  const action = provider?.id ? PIN_ACTION[provider.id]?.[locale] : undefined;
  const name = provider?.id ? PROVIDER_NAME[provider.id]?.[locale] : undefined;
  if (!action || !name) return "";
  return t("preview.waiting_pin_tip", { action, provider: name });
}

/**
 * What a cloud wait puts on the shared surface: how the meter reads, the
 * fraction to draw it with, the sentence under it, and the tip.
 *
 * The rest of the screen — title, crossing glow, escapes — is the surface's,
 * because it is identical to every other wait's. This module owns only what is
 * genuinely about the cloud, which is these four values and the two vendor
 * tables above.
 */
export function cloudWaitSpec(
  downloaded: number,
  total: number,
  stalled: boolean,
  provider?: WaitingProvider,
): { meter: MeterMode; fraction?: ScreenFraction; meta: string; tip: string } {
  const known = countsKnown(downloaded, total);
  return {
    meter: !known ? "unknown" : stalled ? "stalled" : "live",
    fraction: known ? { value: downloaded, max: total } : undefined,
    meta: metaText(downloaded, total, provider),
    tip: pinTip(provider),
  };
}

/**
 * The corner panel's version of the stalled notice, for when the site is
 * already up and the full-window screen is not in play.
 *
 * Lives here so both surfaces read from one wording. The `" — "` is the
 * delimiter `groupNotices` splits an advisory reason on to get title + detail;
 * it is consumed by that split and never rendered.
 */
/**
 * Concrete read failures: the count covers every failure, while names are a
 * bounded sample. A later supervisor snapshot clears this notice on recovery.
 */
export const unavailableAdvisoryReason = (count: number | null, files: string[]): string => {
  if (count === 1 && files.length > 0) {
    return `${t("preview.cloud_unavailable_one", { file: files[0] })} — ${t(
      "preview.cloud_unavailable_hint_one",
    )}`;
  }
  const examples = files.length > 0 ? ` ${t("preview.cloud_unavailable_examples", { files: files.join(", ") })}` : "";
  const lead = count === null ? t("preview.cloud_unavailable_some") :
    t("preview.cloud_unavailable_many", { count: String(count) });
  return `${lead} — ${t("preview.cloud_unavailable_hint_many")}${examples}`;
};
