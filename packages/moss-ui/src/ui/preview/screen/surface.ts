// The one full-window screen the preview window puts up when it cannot show
// the site — every reason, in one vocabulary.
//
// Rendered into #moss-preview-overlay, the one surface for every build error,
// `showMessage()` and wait.
//
// ## Every state is one shape, filled in differently
//
// A state declares a `ScreenSpec` — title, meter, sentence, hint, tip,
// buttons — and ONE renderer draws it. There is no per-state template to
// forget the meter, and the shape key is derived from the spec rather than
// restated per kind, so a new state cannot forget to key on its own contents
// either.
//
// Two rules decide the two things a spec can turn off:
//
// - **A meter is drawn for every wait**, determinate or not: the lit part of
//   the screen's axis, a real grid line. A *verdict* — an unreachable folder,
//   a build failure — draws none: nothing is arriving to measure.
// - **The crossing glow means something is moving.** A stalled download, an
//   unreachable folder and a failure suppress it; every live wait shows it.

import { cloudWaitSpec, type WaitingProvider } from "./cloud-wait";
import { markStartup } from "../../log";
import { t } from "../../i18n/index";
import {
  actionButton,
  actionsRow,
  escapeHtml,
  applyScreenValues,
  chooseFolderButton,
  detailsFold,
  hintLine,
  locationLineOf,
  META_ANNOUNCE_MS,
  meter,
  metaLineOf,
  revealFolderButton,
  screenShell,
  tipLine,
  type MeterMode,
  type ScreenFraction,
} from "./shell";
import { formatVaultLocation, type VaultLocation } from "./vault-location";
import { renderScreen } from "./overlay-slot";

/** Enough silence that an escape is a mercy rather than an interruption. The
 *  button ships hidden in the markup and is revealed by a timer, so the reveal
 *  costs no re-render and cannot move focus. */
export const CHECKING_ESCAPE_AFTER_MS = 10_000;

/**
 * Where the folder actually is, carried by every ScreenContent kind whose
 * title names it (`specFor`'s "Opening {name}" / "Building {name}") and
 * whose screen offers the location line (`locationLineOf`).
 *
 * `folderName` is `VaultRoot::name()`, threaded from Rust through whatever
 * channel already carries `folder_path` (see the host location resolver's
 * `describe_vault_location`) — this module never re-derives it. Absent (the
 * name hasn't arrived yet, or the kind doesn't carry one) falls back to the
 * old generic title and no location line, rather than a half-labelled one.
 *
 * Deliberately NOT on `stalled`/`unreachable`: both already render
 * {@link revealFolderButton} in their `buttons` row, and the location line
 * renders the same button — two would collide on one id.
 */
interface VaultLocationFields {
  folderName?: string;
  /** Home-abbreviated parent path, already computed by the backend
   *  (`redact_home_dir`) — see `formatVaultLocation`. */
  parentDisplay?: string;
  cloudProviderLabel?: string | null;
}

/**
 * What the surface should render, once the caller has already decided that
 * something should.
 *
 * `unreachable` and `failed` are the two verdicts; the rest are waits.
 */
export type ScreenContent =
  /** The first picture of every launch, before moss knows anything to say
   *  about the folder. index.html carries the same screen as static markup,
   *  so it is on screen before any script runs. */
  | { kind: "starting" }
  | { kind: "unreachable" }
  | ({ kind: "downloading"; downloaded: number; total: number; provider?: WaitingProvider } & VaultLocationFields)
  | { kind: "stalled"; downloaded: number; total: number; provider?: WaitingProvider }
  | ({ kind: "building"; stageText: string } & VaultLocationFields)
  | { kind: "page-pending"; stageText: string; stalled?: boolean }
  | ({ kind: "opening" } & VaultLocationFields)
  /** The nested-site walk, before any question can be asked. A live count with
   *  no denominator — text progress, never a spinner alone — and an escape
   *  that joins it once the wait stops being brief. */
  | ({ kind: "checking"; visited: number } & VaultLocationFields)
  /**
   * moss could not open the site: its build failed, or the open did not
   * finish. `detail` is the error as the program said it, shown folded.
   */
  | { kind: "failed"; folderName: string; detail: string };

/**
 * Everything a state puts on the surface.
 *
 * `meta` and `fraction` are the two values that can move without new markup;
 * everything else is the shape. Keeping them in one record is what lets the
 * renderer and the in-place updater read from a single computation instead of
 * deriving the same sentence twice and drifting.
 */
interface ScreenSpec {
  title: string;
  /** The quiet parent-path line under the title, plus its reveal button —
   *  absent wherever the title itself falls back (no name yet) or where
   *  `buttons` already carries {@link revealFolderButton} (stalled,
   *  unreachable) and a second copy of it would collide on the id. */
  location?: VaultLocation;
  /** null = a verdict rather than a wait, so no meter at all. */
  meter: MeterMode | null;
  fraction?: ScreenFraction;
  /** Something is arriving or progressing. */
  crossing: boolean;
  meta: string;
  hint: string;
  tip: string;
  /** The error in a program's words, folded under "Details". */
  details: string;
  buttons: string[];
  /** Marker attribute on the root, for the states CSS styles differently. */
  attr: string;
  /** The walk's escape ships hidden and is revealed by a timer. */
  escapeHidden: boolean;
  /**
   * Whether this screen covers the whole window, taking the chrome down with
   * it (`home-gate-active`; see `show`). A spec field rather than a parameter
   * because it is a render input like any other, and the shape key has to see
   * it — two states that differ only here must not share markup.
   */
  ownsWindow: boolean;
}

const BASE: ScreenSpec = {
  title: "",
  // Indeterminate by default, so a wait added later that says nothing gets an
  // honest meter rather than a live-looking bar pinned at zero. Every state
  // that has a real fraction says so.
  meter: "unknown",
  crossing: true,
  meta: "",
  hint: "",
  tip: "",
  details: "",
  buttons: [],
  attr: "",
  escapeHidden: false,
  ownsWindow: true,
};

/**
 * The corner-panel notice for `degraded`: either a rebuild has outlived its
 * 300s watchdog and is still running, or the watcher itself is in suppressed
 * backoff (db01dc0) and file-change detection is briefly unreliable. The
 * backend ORs the two causes into one flag, so this copy deliberately does
 * not claim which one is true — see `folder_degraded` in `sweep.rs`.
 * Diagnostics-level by design — "timeout = diagnostics + degradation, never
 * an escape hatch" — so it is an advisory, never a window grab. Lives beside
 * the screen it is NOT, so the two verdicts of one sweep read as one thing.
 * The `" — "` is the delimiter `groupNotices` splits on (head → title, detail
 * after); it is never rendered.
 */
export const buildOverdueAdvisoryReason = (): string =>
  `${t("preview.build_overdue")} — ${t("preview.build_overdue_hint")}`;

/**
 * The wait screen's title and quiet location line, for every kind that
 * carries {@link VaultLocationFields}. Falls back to the old generic title
 * with no location line when the name hasn't arrived yet (e.g. the very
 * first synchronous paint, before `describe_vault_location`'s value has
 * round-tripped) — never a half-labelled "Opening" with nothing after it.
 */
function nameTitle(
  key: "preview.opening_title" | "preview.building_title",
  content: VaultLocationFields,
): { title: string; location?: VaultLocation } {
  if (!content.folderName) return { title: t("preview.waiting_title") };
  return {
    title: t(key, { name: content.folderName }),
    location: formatVaultLocation({
      folderName: content.folderName,
      parentDisplay: content.parentDisplay ?? "",
      cloudProviderLabel: content.cloudProviderLabel,
    }),
  };
}

function specFor(content: ScreenContent): ScreenSpec {
  switch (content.kind) {
    /**
     * The folder moss cannot reach. The backend half is the sweep
     * (crates/moss-build/src/ops/watch/sweep.rs): the vault root has been
     * unreadable for several consecutive passes — moved, renamed, deleted,
     * drive gone, provider down. moss runs no rebuilds until it comes back, so
     * the preview under this screen may be arbitrarily stale, which is why it
     * outranks every other reason rather than being a corner notice.
     *
     * A verdict, so no meter and no glow: there is nothing arriving to
     * animate, and the screen's whole message is that moss cannot see the
     * folder at all. No dismiss either, for the same reason the cloud gate has
     * none — what a dismissal would reveal is a preview moss can no longer
     * vouch for.
     */
    case "unreachable":
      return {
        ...BASE,
        title: t("preview.unavailable_title"),
        meter: null,
        crossing: false,
        hint: t("preview.unavailable_hint"),
        buttons: [revealFolderButton(), chooseFolderButton()],
        attr: "data-folder-unavailable",
      };

    /**
     * The ordinary cloud wait: files still arriving, then the build that has to run once
     * they have arrived. The
     * title covers both — it names the promise the screen is keeping, not the
     * mechanism, which is what changes between the two.
     */
    case "downloading":
      return {
        ...BASE,
        ...nameTitle("preview.opening_title", content),
        buttons: [chooseFolderButton()],
        ...cloudWaitSpec(content.downloaded, content.total, false, content.provider),
      };

    /**
     * Nothing has arrived for a while. Says so, rather than animating a
     * download that isn't happening.
     *
     * No "Retry" button: the supervisor is already retrying on a backoff, so a
     * Retry that did nothing but re-render would be a button that lies.
     */
    case "stalled":
      return {
        ...BASE,
        title: t("preview.waiting_stalled"),
        crossing: false,
        hint: t("preview.waiting_stalled_hint"),
        buttons: [revealFolderButton(), chooseFolderButton()],
        ...cloudWaitSpec(content.downloaded, content.total, true, content.provider),
      };

    /**
     * Rank 8: a build stage is known but nothing has been served yet, and no
     * cloud download is blocking it. `stageText` is the SAME sentence the
     * corner panel would show (`computeStageLabel`) — it has to reach this
     * screen because the panel renders nothing while the screen is up
     * A screen that says nothing explains nothing. With no stage known yet it falls back
     * to the sentence the cloud wait's second phase uses, so the two are one screen saying one thing.
     *
     * The meter is indeterminate: a build has no honest fraction, and the
     * counter that would supply one would interleave tasks, so the meter shows none.
     */
    case "building":
      return {
        ...BASE,
        ...nameTitle("preview.building_title", content),
        meter: "unknown",
        meta: content.stageText || t("preview.waiting_building"),
      };

    /**
     * Rank 4: the author just made a page, the preview has already jumped to
     * its build-independent URL, and the build that produces it has not landed
     * — so the URL serves the "page not found" screen underneath this one.
     *
     * The title names THIS page rather than the site, because the site is fine
     * and already on screen behind us; what the author is waiting for is the
     * one thing they just made.
     */
    case "page-pending":
      return {
        ...BASE,
        title: t("preview.page_pending_title"),
        meta: content.stalled ? t("preview.page_pending_stalled") : content.stageText,
        hint: content.stalled ? t("preview.page_pending_stalled_hint") : t("preview.page_pending_hint"),
        crossing: !content.stalled,
        buttons: content.stalled ? [actionButton("moss-waiting-retry-page", t("preview.retry"))] : [],
        ownsWindow: false,
      };

    case "starting":
      return { ...BASE, title: t("preview.starting_title") };

    /** Nothing served and nothing known yet — the same wait with no sentence
     *  under it. */
    case "opening":
      return { ...BASE, ...nameTitle("preview.opening_title", content), meter: "unknown" };

    /** Detection narration for the nested-site walk: a live count on the
     *  indeterminate meter, and an escape that joins it after ten seconds. */
    case "checking":
      return {
        ...BASE,
        ...nameTitle("preview.opening_title", content),
        meter: "unknown",
        meta: t("threshold.checking", { count: String(content.visited) }),
        buttons: [chooseFolderButton()],
        escapeHidden: true,
      };

    /** The open stopped. Same window, its reason, and the three ways on. */
    case "failed":
      return {
        ...BASE,
        title: t("preview.open_failed_title", { name: content.folderName }),
        meter: null,
        crossing: false,
        hint: t("preview.open_failed_reason"),
        details: content.detail,
        buttons: [
          actionButton("moss-waiting-retry-open", t("preview.retry")),
          actionButton("moss-waiting-send-logs", t("preview.send_logs")),
          chooseFolderButton(),
        ],
        attr: "data-open-failed",
      };
  }
}

function templateFor(spec: ScreenSpec): string {
  return screenShell({
    title: spec.title,
    crossing: spec.crossing,
    attr: spec.attr,
    above: locationLineOf(spec.location),
    meter: spec.meter ? meter(spec.meter, spec.fraction) : "",
    below: `${metaLineOf(spec.meta)}
      ${hintLine(spec.hint)}
      ${tipLine(spec.tip)}
      ${actionsRow(spec.buttons, {
        attr: spec.escapeHidden ? "data-checking-escape" : "",
        hidden: spec.escapeHidden,
      })}
      ${detailsFold(t("preview.open_failed_details"), spec.details)}`,
  });
}

/**
 * The identity of a spec's *markup*, as opposed to its values.
 *
 * It IS the markup: the template rendered with the two movable things
 * neutralised — `meta`'s text and `fraction`'s numbers, which
 * `applyScreenValues` can change in place. Their PRESENCE survives (a blank
 * sentence still emits the line, a zero fraction still emits the `role`),
 * because a sentence appearing or a meter turning determinate is new markup.
 *
 * Derived rather than restated as a field list, so adding anything to
 * `ScreenSpec` keys on it automatically — a list would have to be remembered.
 * The locale rides along inside the localized strings, so a screen sitting for
 * minutes with nothing ticking still repaints on an in-session language change.
 */
function shapeKeyOf(spec: ScreenSpec): string {
  return `${spec.ownsWindow}|${templateFor({
    ...spec,
    meta: spec.meta ? " " : "",
    fraction: spec.fraction && { value: 0, max: 1 },
  })}`;
}

const valueKeyOf = (spec: ScreenSpec): string =>
  `${spec.meta}/${spec.fraction?.value ?? ""}/${spec.fraction?.max ?? ""}`;

/**
 * The one full-window "nothing to look at" surface: what is currently on the
 * overlay, and what has to be torn down when it leaves.
 *
 * Lives here rather than in `PreviewManager` because everything it coordinates
 * — the spec, the in-place update, the grid alignment, the buttons — is this
 * module's. The manager keeps only the decision to show it, and the blueprint
 * grid, which belongs to the overlay rather than to any screen on it.
 */
export class FullWindowScreen {
  /**
   * Two keys: `shape` needs new markup, `key` adds the values
   * [`applyScreenValues`] can move in place. Re-rendering on the supervisor's
   * 2s tick blurs whatever the user had focused, on top of the four costs
   * listed on that function.
   */
  private key: string | null = null;
  private shape: string | null = null;

  constructor(
    private readonly deps: {
      revealFolder?: () => void;
      chooseFolder?: () => void;
      /**
       * The escape on the `checking` screen. Not `chooseFolder`: the walk is a
       * question the backend is awaiting an answer to, so abandoning it has to
       * submit a cancel, not just open a picker and leave the guard hanging.
       * Absent when no guard pass is wired — then the button is the ordinary
       * folder chooser.
       */
      cancelChecking?: () => void;
      retryPage?: () => void;
      /** "Try again" on a failed open. */
      retryOpen?: () => void;
      sendLogs?: () => void;
    },
  ) {}

  /** The `checking` escape's reveal timer, and the clock the meta line's live
   *  region throttles announcements against. */
  private escapeTimer: ReturnType<typeof setTimeout> | null = null;
  private lastAnnounce = 0;

  /**
   * Render (or cheaply update) the screen in `container`.
   *
   * `spec.ownsWindow` is the `home-gate-active` marker, which this surface
   * sets in two places: on the CONTAINER it makes the screen span the whole
   * window, and on `<body>` it empties the titlebar above it and removes the
   * right control column's pills.
   *
   * One reason opts out of both: `page-pending`. It is up for a couple of
   * seconds while a single page builds, so taking the chrome away and giving
   * it straight back would be a flash traded for a flash — and unlike a
   * folder that will not open, none of those controls is meaningless
   * meanwhile. The two markers have to move TOGETHER, because they are one
   * claim ("this screen owns the whole window") wearing two class names:
   * setting only the container's blanked the nav pill, back/forward and the
   * Edit button while leaving the publish pill live — the exact inverse of
   * what opting out was for.
   */
  show(container: HTMLElement, content: ScreenContent): void {
    markStartup("main", "waiting");
    const spec = specFor(content);
    const shape = shapeKeyOf(spec);
    const key = `${shape}/${valueKeyOf(spec)}`;
    // The DOM check catches an overlay some other path has replaced since.
    const live = !!container.querySelector(".moss-waiting-screen");
    if (key === this.key && live) return;
    if (shape === this.shape && live) {
      const now = Date.now();
      const announce = now - this.lastAnnounce >= META_ANNOUNCE_MS;
      if (applyScreenValues(container, spec, announce)) {
        if (announce) this.lastAnnounce = now;
        this.key = key;
        return;
      }
    }
    this.drop();
    this.key = key;
    this.shape = shape;
    this.lastAnnounce = Date.now();
    renderScreen(container, templateFor(spec));
    // Removed, not merely left off, when this screen does not own the window:
    // rank 4 yields its pixels to a download and takes them back again, so
    // this method is re-entered with the other answer, and a stale marker
    // would leave the titlebar covered by a screen that opted out of it.
    container.classList.add("visible");
    container.classList.toggle("home-gate-active", spec.ownsWindow);
    // Mirrored onto <body>, where style.css keeps the titlebar above the
    // screen with only the window controls in it, and removes the right
    // column's pills (layout-zones I1c); `drop()` takes it back down.
    if (spec.ownsWindow) document.body.classList.add("home-gate-active");
    // Each selector finds nothing on a screen that offers no such button. A
    // button whose action this host lacks is taken away, not left to do nothing.
    const choose = content.kind === "checking"
      ? (this.deps.cancelChecking ?? this.deps.chooseFolder)
      : this.deps.chooseFolder;
    for (const [id, act] of [
      ["#moss-waiting-reveal-folder", this.deps.revealFolder],
      ["#moss-waiting-choose-folder", choose],
      ["#moss-waiting-retry-page", this.deps.retryPage],
      ["#moss-waiting-retry-open", this.deps.retryOpen],
      ["#moss-waiting-send-logs", this.deps.sendLogs],
    ] as const) {
      const button = container.querySelector(id);
      if (act) button?.addEventListener("click", () => act());
      else button?.remove();
    }
    if (spec.escapeHidden) {
      this.escapeTimer = setTimeout(() => {
        container.querySelector<HTMLElement>("[data-checking-escape]")?.removeAttribute("hidden");
      }, CHECKING_ESCAPE_AFTER_MS);
    }
  }

  /**
   * Forget the screen, because its markup is about to be written over. Leaving
   * the keys set behind a screen that is gone makes the next tick with the same
   * numbers a no-op, so it never comes back.
   */
  drop(): void {
    this.key = this.shape = null;
    // The re-render path (`show` → `drop` → re-add) stays in one synchronous
    // frame, so the chrome never paints in between; on a real teardown this
    // is what gives the right-column pills back.
    document.body.classList.remove("home-gate-active");
    if (this.escapeTimer !== null) {
      clearTimeout(this.escapeTimer);
      this.escapeTimer = null;
    }
  }
}
