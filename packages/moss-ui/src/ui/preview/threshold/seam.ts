// The threshold screen's data seam — the ONE wire-shaped contract between the
// frontend and the host nested-vault guard.
//
// Shapes mirror the host guard's wire data. Each host adapter must verify
// serialization and type compatibility with its own implementation.
// The commands reach the UI as the host's `nestedSiteGuard`.

import type { AcknowledgeThresholdPrompt, GuardDecision, SubmitThresholdDecision } from "../../host";
import { onMossEvent } from "../../utils/moss-events";

/** What kind of folder the opened root is — string-shape classification,
 *  computed backend-side with zero I/O. The screen renders it; it never
 *  re-derives it. */
export type RootClass =
  | { kind: 'normal' }
  | { kind: 'cloud_provider_root'; provider: string }
  | { kind: 'home_dir' }
  | { kind: 'system_special'; which: string }
  | { kind: 'filesystem_root' };

/** One nested site the bounded walk found under the opened root. */
export interface NestedRootInfo {
  /** '/'-separated, root-relative (build convention). */
  rel_path: string;
  abs_path: string;
  /** Folder basename = the future site name (VaultRoot::name semantics). */
  name: string;
  /** true = published (site_id present) · false = preview-only ·
   *  null = state unreadable (evicted). */
  published: boolean | null;
  site_id: string | null;
}

/** The detector's report for one opened root. */
export interface NestedRootsReport {
  root: string;
  root_class: RootClass;
  /** ≤ 5, shallowest-first. */
  nested: NestedRootInfo[];
  /** A depth/dir/report cap was hit — the walk may have missed sites. */
  truncated: boolean;
  /** Telemetry, and the live count the checking narration shows. */
  dirs_visited: number;
}

/** Amendment 2's guess: which side the decision layer believes is the real
 *  site. Computed backend-side from the report's own signals (root class,
 *  published flags, pristine-ness) — the frontend renders it and NEVER
 *  re-derives it. */
export type SuggestedSite =
  | { kind: 'outer' }
  | { kind: 'nested'; abs_path: string };

/**
 * What the screen asks, which depends on whether the opened root already owns
 * a `.moss/` — a fact the guard states explicitly on the wire:
 *
 *  - 'unowned': cases (a)/(b) — no `.moss` exists yet; the question is which
 *    site to open, and nothing is written until the user commits.
 *  - 'owned': case (d) — ONE question: which folder is
 *    the correct site. The chosen one opens; the guard un-vaults the loser in
 *    the same action.
 */
export interface ThresholdPrompt {
  /** The guard pass's generation token. The screen's ack and its one answer
   *  must quote it, so a click on an older prompt can never land in a newer
   *  prompt's slot (and a predecessor's late teardown never wipes its
   *  successor's live question). */
  token: number;
  kind: 'unowned' | 'owned';
  report: NestedRootsReport;
  /** When present, the case-(d) screen leads with "This looks like your
   *  site: X" and the suggested card is the focused primary. A guess is a
   *  default, never a silent decision. null = neutral peer layout. */
  suggestion: SuggestedSite | null;
}

/**
 * Decision-table row 4's third option: "make this folder a site anyway" is
 * offered ONLY for exactly one nested site that is preview-only
 * (`published !== true`; null = unreadable counts as not-known-preview-only,
 * so no offer) under a Normal root class. Rows 5 (N > 1) and 6 (suspicious
 * root class) never offer it, and neither does case (d).
 *
 * This is the single frontend statement of that policy — the screen calls it,
 * nothing else restates it. The backend re-checks the same rule on receipt
 * (`map_unowned_decision`), so a screen bug cannot widen the table.
 */
export function offersMakeRoot(prompt: ThresholdPrompt): boolean {
  return (
    prompt.kind === 'unowned' &&
    prompt.report.root_class.kind === 'normal' &&
    prompt.report.nested.length === 1 &&
    prompt.report.nested[0].published === false
  );
}

/** The opened folder's display name — basename of the report's root path. */
export function rootFolderName(report: NestedRootsReport): string {
  const trimmed = report.root.replace(/[/\\]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx >= 0 ? trimmed.slice(idx + 1) || trimmed : trimmed;
}

/** What the guard's events drive on the screen side. ThresholdPass is the
 *  one implementor; the interface exists so the transport is testable with a
 *  plain object. */
export interface ThresholdUi {
  /** Detection narration: the guard pass named by `token` is scanning. */
  showChecking(dirsVisited: number, token: number): void;
  /** The guard's one question — cases (a)/(b)/(d). `prompt.token` names the
   *  pass; the transport has already acked it. */
  showPrompt(prompt: ThresholdPrompt): void;
  /** The guard pass named by `token` ended. `cancelled: true` → restore what
   *  the overlay showed before; `false` → leave the screen for the build's
   *  own overlay to paint over (no interstitial flash). A resolve for a pass
   *  the screen is not showing must be ignored. */
  close(cancelled: boolean, token: number): void;
}

/**
 * Is this `decide` failure the backend saying "that question is dead"
 * (already answered, superseded, or nobody awaiting)? Then the screen must
 * come down. Any other failure is transient — the question is still parked
 * backend-side, so the screen re-arms instead of leaving an invisible
 * pending question. Texts mirror wire.rs's ERR_NOT_AWAITING / ERR_STALE.
 */
export function isThresholdRefusal(message: string): boolean {
  return (
    message.includes('no threshold question is awaiting a decision') ||
    message.includes('stale threshold decision')
  );
}

/** How the screen hands its one decision back to the guard, and how the
 *  guard's events reach the screen. */
export interface ThresholdTransport {
  decide(decision: GuardDecision, token: number): Promise<void>;
  /** Subscribe to the guard's events; returns the combined unlisten. */
  start(): Promise<() => void>;
}

// Deliberately deferred. Follow-ups for the narration and the card meta, summarized here:
// - The stalled/offline variant preserves the distinction between unknown and none: when the
//   walk stalls on an unmaterialized cloud folder, the checking screen should
//   name the provider and its pin action — reuse the waiting screen's
//   PIN_ACTION/PROVIDER_NAME tables rather than duplicating them. Requires
//   the backend walk to report a stall, which it cannot yet (synchronous,
//   no progress callback) — the same gap that keeps the checking narration a
//   single pre-scan tick instead of a live count.
// - Card meta lines and title-from-config: the report carries no dates and no
//   site title yet, so cards show folder name + rel path. When the backend
//   adds them, extend NestedRootInfo (Rust wire.rs) and the card template
//   together.
export function createThresholdTransport(
  ui: ThresholdUi,
  submit: SubmitThresholdDecision,
  acknowledge: AcknowledgeThresholdPrompt,
): ThresholdTransport {
  return {
    async decide(decision: GuardDecision, token: number): Promise<void> {
      const result = await submit(decision, token);
      if (result.status === 'error') {
        throw new Error(result.error);
      }
    },
    async start(): Promise<() => void> {
      const unlisteners = await Promise.all([
        onMossEvent('ThresholdChecking', (msg) =>
          ui.showChecking(msg.payload.dirs_visited, msg.payload.token),
        ),
        onMossEvent('ThresholdPrompt', (msg) => {
          // Ack first: the guard holds a bounded wait for this before it
          // trusts the screen with the question (webview-exists is not
          // listener-mounted). Fire-and-forget — a refused/failed ack means
          // the guard falls back to the dialog on its own clock.
          acknowledge(msg.payload.token).catch((err) => {
            console.warn('[threshold] prompt ack failed:', err);
          });
          ui.showPrompt(msg.payload);
        }),
        onMossEvent('ThresholdResolved', (msg) =>
          ui.close(msg.payload.cancelled, msg.payload.token),
        ),
      ]);
      return () => unlisteners.forEach((un) => un());
    },
  };
}
