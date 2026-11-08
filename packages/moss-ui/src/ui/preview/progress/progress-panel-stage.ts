import type { PreviewStage } from '../preview-state';
import type { Action, Severity, Scope } from '@symbiosis-lab/moss-ui';
import { t } from "../../i18n/index";
import { hasSizeReadout } from './upload-format';
import type { FileParts } from './upload-file';

// ─── The panel's shared type contract ───────────────────────────────────────
//
// These five types are consumed by the panel, its renderer, the status block
// and the formatters below. They used to live on `progress-panel.ts`, which
// meant every one of those modules imported the 2,600-line panel in order to
// name a task row — a cycle, and one the dependency cruiser flagged the moment
// a sixth module joined it. They sit here because this module is already the
// pure, dependency-free half of the panel.

export interface ProgressTask {
  id: string;
  label: string;
  progress: number | null; // 0-100 or null for indeterminate
  detail?: string;
  cancellable?: boolean;
  source: "moss" | "plugin";
  pluginName?: string;
  /**
   * Monotonic timestamp (Date.now()) set on every
   * upsertTask call. Used by buildViewModel's tiebreak rule: among running
   * `job:*`/`plugin:*` tasks at the same tier, the most-recently-updated wins
   * the status line (deterministic, no flap — the others stay tracked but
   * off-screen). Not shown in the UI; internal only.
   */
  lastUpdatedAt?: number;
  /**
   * Routine background sub-jobs (`bg:videos`, `bg:images`, `bg:rss`,
   * save/template-apply) set `ambient: true`. An ambient task is recorded in
   * the store but does NOT pop the panel open on its own — it lives and dies at
   * the hairline, keeping successful routine work quiet. It un-hides the panel
   * ONLY when the build is already consequential (`shouldSurface()` true).
   *
   * Consequential tasks (`preview-server-error`, `dns:check`, deploy/syndicate/
   * enhance hooks) leave this unset and keep un-hiding unconditionally —
   * silencing one of THOSE would bury a real error.
   */
  ambient?: boolean;
  /**
   * This row's label is a SENTENCE naming the stage a long operation is in
   * ("Pulling images (43 MB / 210 MB)…"), not a short noun. The renderer marks
   * it `data-narrate` and the cascade lets it wrap within a budget, because the
   * default single-line ellipsis cuts a stage name in half — and half a stage
   * name is the same as none. Set by the machine-scoped stack install, whose
   * whole purpose on this surface is to SAY what the wait is.
   *
   * Orthogonal to `ambient`, and in practice its opposite: a narrating row is
   * consequential, so it surfaces the panel like any other non-ambient task.
   */
  narrate?: boolean;
  /**
   * A media sub-job's parent Build Job row id (`job:<parentId>`).
   * When set, the renderer nests this row under its parent (indented, with a
   * `data-parent` marker). Media children (images/videos) and the fine-grained
   * `bg:videos` detail nest under the Build parent; a content-only rebuild mints
   * no parent → no nested rows (invariant #6 — the heartbeat owns it).
   */
  parentId?: string;
  /**
   * The recovery affordance for a Job that needs the user (the
   * `Awaiting` interrupt's typed `Action` — e.g. the domain email-verify 403's
   * `InApp{SignIn}` "Open Settings" button). When set, the renderer draws the
   * action below the row. `"None"` / unset renders no affordance.
   */
  action?: Action;
}

/**
 * A soft advisory surfaced in the panel's advisory zone. `reason` carries the
 * Rust `Advisory.what` text supplies the human-readable message shown on hover
 * for a per-file advisory and inline for a build-wide one. `action` carries the
 * Rust `Advisory.action` (the recovery affordance, action-as-data); only the
 * build-wide tier (`item == null`) renders it, as a copyable `Command` field.
 */
export interface AdvisoryEntry {
  task: string;
  item: string | null;
  reason: string;
  action?: Action;
  /**
   * The Rust `Advisory.severity` (`ShippedDegraded | NeedsAction | Blocking`).
   * Drives the surface tier: only a `Blocking` advisory satisfies
   * `shouldSurface()` and interrupts by showing the panel. A
   * `ShippedDegraded` / non-blocking `NeedsAction` advisory stays a quiet
   * hairline dot, never pushed. Optional so older events (or hand-built test
   * fixtures) that omit it don't crash — a missing severity is treated as
   * non-surfacing.
   */
  severity?: Severity;
  /**
   * The Rust `Advisory.scope` (`File | Config | Environment |
   * Remote | Account`). Drives `noticeTier()`: NeedsAction+File→broken, everything
   * else→degraded. Optional to keep existing fixtures without scope valid.
   */
  scope?: Scope;
  /**
   * Survives the panel's build-start blanket clears (`clearBuildAdvisories`);
   * removed only by CLEAR_NOTICE or panel destroy. For advisories scoped to
   * something other than a build — the `publish-verify` advisory is scoped to
   * a publish generation, and a mid-check rebuild must not wipe the verdict
   * Same shaped carve-out the
   * panel already grants `preview-server-error` and the cloud keys, made
   * declarative.
   */
  sticky?: boolean;
}

/**
 * The view-model passed from ProgressPanel to the
 * renderer. Built by `buildViewModel()`.
 */
export interface NoticeVM {
  /**
   * Stable identity across renders — `${task}::${reason-head}`, the same key
   * `groupNotices` groups by.
   *
   * The notices rail is rebuilt wholesale on every render, so any state living
   * in a notice's DOM (a `<details open>`, a revealed `+N` list) dies with it.
   * A cloud-sync stall re-renders every 2 seconds, which is how "expand the
   * detail" became impossible to do. The renderer keys the open
   * state off this instead of off the element.
   */
  key: string;
  severity: "broken" | "degraded";
  reason: string;
  files: string[];
  hiddenFiles: string[];
  command?: { run: string };
  action?: Action;
  detail?: string;
  cloudFiles?: {
    count: number | null;
    status: "idle" | "loading" | "loaded" | "error";
    files: string[];
  };
}

export interface PanelViewModel {
  /**
   * Exactly what `formatStatus` returns — the same type, not a copy of its
   * shape. The copy that used to live here drifted the moment the status line
   * grew a second register, and the renderer typechecked against the stale one.
   */
  status: StatusResult;
  notices: NoticeVM[];
  reveal: "minimized" | "peek" | "pinned" | "surfaced";
  topSeverity: "broken" | "degraded" | "none";
}

export type ProgressPlacement = "bottom";

export type StageLabelInput =
  | { stage: 'initial' }
  | { stage: 'opening' }
  | { stage: 'scanning'; found?: number }
  | { stage: 'materializing'; provider: string; downloaded: number; total: number }
  | { stage: 'compiling'; phase: PagePhase; current: number; total: number }
  | { stage: 'ready' };

/**
 * Which half of the page work a `compiling` count belongs to.
 *
 * A build renders every markdown file into a document, then writes every
 * document out as a page — two passes over almost, but not exactly, the same
 * set. They are separate `BackgroundProgress` tasks with separate denominators
 * (`html_total` counts synthetic documents markdown never saw, which is the
 * "total goes up by one at the end" the counter used to show), and each is
 * monotonic only within itself.
 *
 * So the phase rides with the count and the label names it. A denominator that
 * moves while the sentence stays put reads as a broken counter; a denominator
 * that moves WITH the verb is the build entering its second pass. Same rule
 * `screen/cloud-wait.ts` applies at the download→build crossing, where it refuses
 * to hold "553 of 553" up while the build blocks.
 */
export type PagePhase = 'markdown' | 'pages';

/**
 * The only two `BackgroundProgress` tasks that count pages.
 *
 * Every other task a build emits — `images`, `videos`, `assets`, `notebooks`,
 * `sealing` — measures its own unrelated set, and `sealing` reports a flat
 * 0/1. Feeding them all into one pair of scalars is what made the counter walk
 * backwards; this table is the whitelist that stops it.
 */
export const PAGE_PHASE_BY_TASK: Readonly<Record<string, PagePhase>> = {
  markdown: 'markdown',
  html_pages: 'pages',
};

/** Phase ordering, so a straggler from the first pass cannot rewind the
 *  second. Same monotonicity discipline as [`STAGE_RANK`], one level down. */
export const PAGE_PHASE_RANK: Readonly<Record<PagePhase, number>> = {
  markdown: 0,
  pages: 1,
};

/** A page counter in flight: how far, out of how many, in which pass. */
export interface PageProgress {
  phase: PagePhase;
  current: number;
  total: number;
}

/**
 * Fold one `BackgroundProgress` into the page counter, or refuse it.
 *
 * Three refusals, and each is a bug the counter used to show. A task that does
 * not count pages is dropped outright — `images`, `videos`, `assets`,
 * `notebooks` and `sealing` all wrote the shared pair, so the numerator fell
 * from 553 to 3 mid-build. A straggler from the earlier pass cannot rewind the
 * later one. And within a pass the numerator only rises, because the terminal
 * `html_pages` emit reports the pages actually achieved, which trails the last
 * stride tick whenever documents were skipped.
 */
export function nextPageProgress(
  live: PageProgress | undefined,
  task: string,
  current: number,
  total: number,
): PageProgress | undefined {
  const phase = PAGE_PHASE_BY_TASK[task];
  if (!phase) return live;
  if (live && PAGE_PHASE_RANK[phase] < PAGE_PHASE_RANK[live.phase]) return live;
  if (live && phase === live.phase) {
    return { phase, current: Math.max(live.current, current), total };
  }
  return { phase, current, total };
}

/** The compiling readout. No page task has reported yet → the verb with no
 *  parenthetical, never "(0 / 0)"; `computeStageLabel` drops it on total 0. */
export function compilingLabelInput(pages: PageProgress | undefined): StageLabelInput {
  return {
    stage: 'compiling',
    phase: pages?.phase ?? 'markdown',
    current: pages?.current ?? 0,
    total: pages?.total ?? 0,
  };
}

/**
 * Decide the `bg:<task>` row (if any) for a `BackgroundProgress` tick the
 * primary bar does not already count — `null` when `PAGE_PHASE_BY_TASK`
 * covers it (markdown/html_pages) or the tick is advisory-only. A listed
 * task gets a plain-language phrase; an unlisted one falls back to the
 * wire's own formatted `message` (Rust `format_progress_message`), so a
 * future emitter needs no frontend change to get *a* row.
 *
 * `skipArrival` marks `videos`: it reports twice — this quiet tick (queued,
 * before FFmpeg starts — video.rs emits it ahead of the materialize wait and
 * the encode-permit queue, which can run tens of seconds) and the richer
 * VideoConversionProgress tick once FFmpeg's callback fires. Both upsert the
 * SAME `bg:videos` row so the fill is hoverable from the moment it moves
 * Only FFmpeg's tick may claim
 * `upsertTask`'s first-arrival re-surface check — the quiet one spending it
 * would leave the real, user-meaningful arrival never asked.
 */
export function bgTaskRowDecision(p: {
  task: string;
  current: number;
  total: number;
  message: string;
  isAdvisoryOnly: boolean;
  parentId: string | undefined;
}): { task: ProgressTask; skipArrival: boolean } | null {
  if (p.isAdvisoryOnly || PAGE_PHASE_BY_TASK[p.task]) return null;
  const base = { source: 'moss' as const, ambient: true, parentId: p.parentId };
  if (p.task === 'videos') {
    return {
      task: {
        ...base,
        id: 'bg:videos',
        label: t(p.total === 1 ? 'progress.bg_videos_waiting_one' : 'progress.bg_videos_waiting_other', { total: p.total }),
        progress: null, // queued, not fractional yet — FFmpeg's own tick supplies the real fraction
      },
      skipArrival: true,
    };
  }
  const progress = p.total > 0 ? Math.round((p.current / p.total) * 100) : null;
  const label = p.task === 'images' ? t('progress.bg_images', { current: p.current, total: p.total }) : p.message || p.task;
  return { task: { ...base, id: `bg:${p.task}`, label, progress }, skipArrival: false };
}

export interface StageLabel {
  text: string;
  /** null = indeterminate bar; 0–100 = determinate. */
  percent: number | null;
}

export function computeStageLabel(input: StageLabelInput): StageLabel {
  switch (input.stage) {
    case 'initial':
      return { text: '', percent: null };
    case 'opening':
      return { text: t('pstage.opening'), percent: null };
    case 'scanning':
      return {
        text:
          input.found != null
            ? t('pstage.scanning_found', { n: input.found })
            : t('pstage.scanning'),
        percent: null,
      };
    case 'materializing': {
      const pct = input.total > 0 ? (input.downloaded / input.total) * 100 : null;
      return {
        text: t('pstage.downloading', {
          provider: input.provider,
          n: input.downloaded,
          total: input.total,
        }),
        percent: pct,
      };
    }
    case 'compiling': {
      const pct = input.total > 0 ? (input.current / input.total) * 100 : null;
      // No denominator yet — say the verb, not "(0 / 0)".
      //
      // Both callers (`buildStageLabelInput`, `buildStageKnownLabel`) fall back
      // to a zero count, and it is unset for the whole stretch between the
      // first build-progress tick — which advances the stage to `compiling` —
      // and the first page-task `BackgroundProgress`. Interpolating zeros there
      // pinned a counter at 0 / 0 in front of the user, which reads as a build
      // that has stopped — the opposite of what rank 7 exists to say.
      //
      // Same discipline, same shape as `pstage.scanning` / `scanning_found`
      // and `pstage.uploading_plain` / `uploading`: the count is an
      // enrichment, and its absence drops the parenthetical rather than
      // rendering it empty.
      const verb = input.phase === 'markdown' ? 'formatting' : 'generating';
      return {
        text:
          input.total > 0
            ? t(`pstage.${verb}`, { n: input.current, total: input.total })
            : t(`pstage.${verb}_plain`),
        percent: pct,
      };
    }
    case 'ready':
      return { text: '', percent: null };
  }
}

export type StageEvent =
  | { kind: 'scan-progress' }
  | { kind: 'scan-complete'; evictedCount: number }
  | { kind: 'build-progress' }
  | { kind: 'server-ready' };

/**
 * Rank ordering for `PreviewStage` — used to enforce monotonicity in
 * `nextStage`. Late or out-of-order events (a stray `build-progress` after
 * the early-port `server-ready` flipped the panel to `'ready'`, a delayed
 * `scan-complete` arriving after compile already started) must not rewind
 * the bar to an earlier phase.
 */
export const STAGE_RANK: Record<PreviewStage, number> = {
  initial: 0,
  opening: 1,
  // Pre-build threshold states (nested-site guard): between opening and the
  // build proper. Same monotonicity — a late detection tick must not rewind
  // a build that has started.
  checking: 2,
  threshold: 3,
  scanning: 4,
  materializing: 5,
  compiling: 6,
  ready: 7,
};

/**
 * Pure stage-transition function. `current` is the current stage;
 * returns the new stage after observing `event`.
 *
 * Invariant: never regresses. Each event proposes a target stage; we
 * advance only if the target outranks `current`.
 *
 * The regression guard matters when site/ already has content from a
 * prior build: `start_preview_server` succeeds and the backend emits an
 * early `ProgressUpdate { port: Some, is_empty: false }` before scan or
 * compile have run. The frontend flips status to 'ready' and calls
 * `setStage('ready')` — but background image/asset tasks keep emitting
 * `BackgroundProgress` events. Without this guard, each one rewound the
 * stage to 'compiling'. The terminal `ProgressUpdate` then lands in
 * preview-state.ts's silent-rebuild branch and returns the same status
 * object, so `setStage('ready')` is never called again and the bar sits
 * at "Generating pages… (N / N)" forever.
 */
export function nextStage(current: PreviewStage, event: StageEvent): PreviewStage {
  const target = stageTarget(event);
  return STAGE_RANK[target] > STAGE_RANK[current] ? target : current;
}

function stageTarget(event: StageEvent): PreviewStage {
  switch (event.kind) {
    case 'scan-progress':
      return 'scanning';
    case 'scan-complete':
      return event.evictedCount > 0 ? 'materializing' : 'compiling';
    case 'build-progress':
      return 'compiling';
    case 'server-ready':
      return 'ready';
  }
}

// ─── Publish stage state machine ────────────────────────────────────────────
//
// The publish pipeline runs after build (or concurrently, when waiting on a
// late background task). It drives the same single primary-progress bar that
// the build stage drives — the panel renders one or the other based on which
// half is active.

export type PublishStage =
  | 'idle'
  | 'waiting'
  | 'preparing'
  | 'rebuilding'
  | 'syncing'
  | 'uploading'
  | 'committing'
  | 'finalizing'
  | 'complete'
  | 'failed';

export type PublishLabelInput =
  | { stage: 'waiting' }
  | { stage: 'preparing' }
  | { stage: 'rebuilding'; message?: string }
  | { stage: 'syncing' }
  | {
      stage: 'uploading';
      current: number;
      total: number;
      /**
       * Byte counts from `deploy-progress`. Present
       * only on the bytes path — absent on the prebuilt CLI path and on a
       * commit-only deploy, where the file-count form is the fallback.
       */
      bytesDone?: number;
      bytesTotal?: number;
    }
  | { stage: 'committing' }
  | { stage: 'finalizing' }
  | { stage: 'complete' }
  | { stage: 'failed' };

export function computePublishLabel(input: PublishLabelInput): StageLabel {
  switch (input.stage) {
    case 'waiting':
      return { text: t('pstage.waiting'), percent: null };
    case 'preparing':
      return { text: t('pstage.preparing'), percent: null };
    case 'rebuilding':
      return { text: input.message?.trim() || t('pstage.rebuilding'), percent: null };
    case 'syncing':
      return { text: t('pstage.syncing'), percent: null };
    case 'uploading': {
      const pct = input.total > 0 ? (input.current / input.total) * 100 : null;
      // On the bytes path the verb drops the file count: the numbers move to
      // the count slot as `12.4/58.0 MB · 2.1 MB/s`.
      // File-count progress is the least informative of the four available
      // numbers — it reaches ~97% before the videos start.
      //
      // `percent` is deliberately left on the file-count basis. A grep of the
      // publish path found no consumer of StageLabel.percent; switching it to
      // a byte basis would be dead code plus a test asserting a value nobody
      // reads. If a future primary bar consumes it, that is a two-line edit
      // with a real consumer to test against.
      // A tiny total is NOT the bytes path: `0/0 kB` says less than the file
      // count, so the verb keeps its `(n / total)` — see `hasSizeReadout`.
      const haveBytes = hasSizeReadout(input.bytesTotal);
      return {
        text: haveBytes
          ? t('pstage.uploading_plain')
          : t('pstage.uploading', { n: input.current, total: input.total }),
        percent: pct,
      };
    }
    case 'committing':
      return { text: t('pstage.committing'), percent: null };
    case 'finalizing':
      return { text: t('pstage.finalizing'), percent: null };
    case 'complete':
    case 'failed':
      return { text: '', percent: null };
  }
}

export type PublishEvent =
  | { kind: 'waiting' }
  | {
      kind: 'progress';
      stage:
        | 'preparing'
        | 'rebuilding'
        | 'syncing'
        | 'uploading'
        | 'committing'
        | 'finalizing';
    }
  | { kind: 'complete' }
  | { kind: 'failed' };

const PUBLISH_RANK: Record<PublishStage, number> = {
  idle: 0,
  waiting: 1,
  preparing: 2,
  rebuilding: 3,
  syncing: 4,
  uploading: 5,
  committing: 6,
  finalizing: 7,
  complete: 8,
  failed: 8,
};

/**
 * Pure transition for publish stages. Never regresses — a late "waiting"
 * event after publish has already started progressing is ignored. `complete`
 * and `failed` are terminal.
 */
export function nextPublishStage(
  current: PublishStage,
  event: PublishEvent,
): PublishStage {
  switch (event.kind) {
    case 'waiting':
      return current === 'idle' ? 'waiting' : current;
    case 'progress':
      return PUBLISH_RANK[event.stage] > PUBLISH_RANK[current]
        ? event.stage
        : current;
    case 'complete':
      return 'complete';
    case 'failed':
      return 'failed';
  }
}

// ─── Status line ──────────────────────────────────────────────────
//
// `formatStatus` produces the single-line status text for the status-line zone.
// The STATUS priority (failed → publish stage → media → build stage → receipt →
// none) is resolved by the caller (buildViewModel / renderPanel); this function
// only formats one input into the output shape.

export type StatusInput =
  | {
      kind: 'running';
      verb: string;
      subject?: string;
      current?: number;
      total?: number;
      /**
       * Pre-formatted count text that WINS over the `current`/`total` pair.
       * The deploy upload readout (`12.4/58.0 MB · 2.1 MB/s`) is not an
       * "n of m" fraction, so it arrives already composed.
       */
      countText?: string;
      /**
       * Milestone text for the a11y live region (25/50/75% of an upload).
       * Sticky between milestones — the renderer only writes it when it
       * CHANGES, so a re-render does not re-announce.
       */
      announce?: string;
      /**
       * The SECOND register: what is moving and how
       * fast. Evidence for the headline rather than a peer of it, so it gets
       * its own quieter row instead of competing for the one nowrap line that
       * used to hold four kinds of thing and truncate three of them.
       */
      detail?: StatusDetail;
      /**
       * A standing condition for the whole operation — "Rebuild paused until
       * upload finishes". Not progress at all, which is why it does not ride
       * the `subject` slot: there it was the only shrinkable child of a full
       * row, so it ellipsized to nothing every time.
       */
      condition?: string;
    }
  | { kind: 'receipt'; text: string }
  | { kind: 'failed'; text: string }
  | { kind: 'none' };

/**
 * The detail row: a file on the left, a rate on the right. Either half may be
 * absent — a commit-only deploy names no file, and the rate is unknown until
 * the trailing window has two samples.
 */
export interface StatusDetail {
  /** Pre-split so the directory can recede without a second parse in the DOM. */
  file?: FileParts;
  rate?: string;
}

export interface StatusResult {
  text: string;
  subject?: string;
  count?: string;
  detail?: StatusDetail;
  condition?: string;
  kind: 'running' | 'receipt' | 'failed' | 'none';
  /** Live-region milestone text; only meaningful when kind === "running". */
  announce?: string;
  /**
   * A green receipt requires zero unresolved notices.
   * When notices are present the receipt is neutral (no accent colour).
   * Only meaningful when kind === "receipt"; undefined otherwise.
   */
  clear?: boolean;
}

/**
 * Pure status-line formatter. Converts a structured StatusInput into the
 * StatusResult shape consumed by the renderer.
 *
 *   • running  — verb text + optional subject (filename) + optional count ("3 / 12")
 *   • receipt  — pre-formatted "Built · N pages · X.Xs" (no transformation)
 *   • failed   — pre-formatted failure text; rendered in danger color
 *   • none     — empty; status line is hidden
 */
export function formatStatus(input: StatusInput): StatusResult {
  switch (input.kind) {
    case 'running': {
      const count =
        input.countText ??
        (input.current != null && input.total != null
          ? `${input.current} / ${input.total}`
          : undefined);
      // An empty detail object would make the renderer mount a row with
      // nothing in it, so drop it here rather than testing two fields there.
      const detail =
        input.detail?.file || input.detail?.rate ? input.detail : undefined;
      return {
        text: input.verb,
        subject: input.subject,
        count,
        detail,
        condition: input.condition,
        announce: input.announce,
        kind: 'running',
      };
    }
    case 'receipt':
      return { text: input.text, kind: 'receipt' };
    case 'failed':
      return { text: input.text, kind: 'failed' };
    case 'none':
      return { text: '', kind: 'none' };
  }
}

// ─── Notice tier + groupNotices ────────────────────────────────────

/**
 * Number of files shown inline per NoticeVM. Overflow goes to hiddenFiles.
 */
export const FILES_BUDGET = 6;

/**
 * Shared predicate: exclude awaiting-narration synthetic advisories from
 * groupNotices. They are surfaced via the awaiting bridge,
 * never doubled as a notice.
 */
export function isAwaitingNarration(e: AdvisoryEntry): boolean {
  return e.task === 'awaiting-narration';
}

/**
 * Pure severity → tier mapping (by NATURE, not by parsing `what`).
 *   Blocking → broken
 *   NeedsAction + File → broken (a specific file is missing/broken)
 *   Everything else → degraded (NeedsAction+Env/Remote/Account/Config, ShippedDegraded, unknown)
 */
export function noticeTier(
  severity: Severity | undefined,
  scope: Scope | undefined,
): 'broken' | 'degraded' {
  if (severity === 'Blocking') return 'broken';
  if (severity === 'NeedsAction' && scope === 'File') return 'broken';
  return 'degraded';
}

/**
 * Group a flat AdvisoryEntry list into NoticeVM[]:
 *   1. Exclude awaiting-narration entries (bridge owns them).
 *   2. Split `reason` at first " — " → head (reason) + tail (detail).
 *   3. Group by (task, reason-head).
 *   4. Set severity = noticeTier(severity, scope) for each group.
 *   5. Sort broken-first, then first-seen.
 *   6. Cap files at FILES_BUDGET → hiddenFiles.
 *   7. Carry command (Action::Command → {run}) + action (InApp/Link).
 */
/**
 * The key `groupNotices` groups by, and the only identity a rendered notice
 * row has: `${task}::${reason-head}`, where the head is everything before the
 * first " — ".
 *
 * Exported because the dismiss button on a notice can only name this key, so
 * whoever handles the dismissal has to be able to recompute it per advisory.
 * One owner, or the two derivations drift and a dismissal silently clears
 * nothing.
 */
export function noticeGroupKey(e: Pick<AdvisoryEntry, 'task' | 'reason'>): string {
  const dashIdx = e.reason.indexOf(' — ');
  return `${e.task}::${dashIdx >= 0 ? e.reason.slice(0, dashIdx) : e.reason}`;
}

/**
 * Does this advisory report an ACT moss took rather than a condition of the
 * build? Those are the ones that must survive a rebuild and leave only by
 * being acknowledged — see `Action::Acknowledge` in `advisory.rs`.
 */
export function isAcknowledge(action: Action | null | undefined): boolean {
  return !!action && action !== 'None' && typeof action === 'object' && 'Acknowledge' in action;
}

/**
 * Which of `advisories`' keys belong to `task` and are stale after that
 * job's own `completed:true` tick — present under that job,
 * not sticky, and not in `refreshed` (the keys THIS run of the task
 * re-raised via its `BackgroundProgress` payloads, tracked by the caller).
 *
 * Deliberately per-task, not a blanket "clear everything, this build
 * started": a rebuild's task set is not the whole site, and `nextStage` never
 * regresses so a stage-transition clear could only ever fire on a session's
 * first build. An unchanged markdown page replays its parse-cache entry
 * without re-deriving its advisories, but the only advisory ever carried on
 * the "markdown" task (duplicate-uid) is derived from the FULL document set
 * every build and is sticky besides, so that replay is harmless.
 *
 * Every task's `completed:true` tick speaks for its whole item set: the
 * engine carries a skip-carried image/video's advisory forward from its
 * fingerprint cache and re-emits it in that terminal tick (moss-build
 * `ImageLedger`/`VideoConversionState`), so no per-task carve-out is needed.
 * Callers invoke this ONLY from a task's own completion, never from a
 * build-start or stage-transition signal.
 */
export function staleTaskAdvisoryKeys(
  advisories: ReadonlyMap<string, AdvisoryEntry>,
  task: string,
  refreshed: ReadonlySet<string> | undefined,
): string[] {
  const stale: string[] = [];
  for (const [key, entry] of advisories) {
    if (entry.task !== task || entry.sticky) continue;
    if (refreshed?.has(key)) continue;
    stale.push(key);
  }
  return stale;
}

export function groupNotices(advisories: AdvisoryEntry[]): NoticeVM[] {
  // Exclude awaiting-narration
  const filtered = advisories.filter((e) => !isAwaitingNarration(e));

  // Group by (task, reason-head)
  const groups = new Map<string, {
    key: string;
    tier: 'broken' | 'degraded';
    reason: string;
    detail?: string;
    files: string[];
    command?: { run: string };
    action?: AdvisoryEntry['action'];
    firstSeen: number;
  }>();

  let order = 0;
  for (const e of filtered) {
    // Split at first " — " (en-dash surrounded by spaces)
    const dashIdx = e.reason.indexOf(' — ');
    const head = dashIdx >= 0 ? e.reason.slice(0, dashIdx) : e.reason;
    const tail = dashIdx >= 0 ? e.reason.slice(dashIdx + 3) : undefined;

    const groupKey = noticeGroupKey(e);
    const tier = noticeTier(e.severity, e.scope);

    if (!groups.has(groupKey)) {
      groups.set(groupKey, {
        key: groupKey,
        tier,
        reason: head,
        detail: tail,
        files: [],
        command: undefined,
        action: undefined,
        firstSeen: order++,
      });
    }

    const g = groups.get(groupKey)!;
    // Per-file advisory: collect the filename
    if (e.item != null) {
      g.files.push(e.item);
    }
    // Carry command from Action::Command (wire key: `run`)
    if (e.action && typeof e.action === 'object' && 'Command' in e.action) {
      g.command = { run: (e.action as { Command: { run: string } }).Command.run };
    }
    // Carry InApp/Link action
    if (e.action && e.action !== 'None' && typeof e.action === 'object' && !('Command' in e.action)) {
      g.action = e.action;
    }
    // If this entry elevates to broken, promote the group
    if (tier === 'broken') {
      g.tier = 'broken';
    }
  }

  // Sort broken-first then first-seen
  const sorted = Array.from(groups.values()).sort((a, b) => {
    if (a.tier !== b.tier) {
      return a.tier === 'broken' ? -1 : 1;
    }
    return a.firstSeen - b.firstSeen;
  });

  // Cap files, build NoticeVM[]
  return sorted.map((g) => {
    const files = g.files.slice(0, FILES_BUDGET);
    const hiddenFiles = g.files.slice(FILES_BUDGET);
    const vm: NoticeVM = {
      key: g.key,
      severity: g.tier,
      reason: g.reason,
      files,
      hiddenFiles,
    };
    if (g.detail !== undefined) vm.detail = g.detail;
    if (g.command !== undefined) vm.command = g.command;
    if (g.action !== undefined) vm.action = g.action;
    return vm;
  });
}
