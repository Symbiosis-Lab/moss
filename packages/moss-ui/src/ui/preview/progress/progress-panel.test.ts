/**
 * ProgressPanel — publish/deploy FAILURE advisory surface.
 *
 * Regression guard for the "bare Publish failed" bug: when a deploy fails, the
 * panel must restore the STANDARD advisory surface —
 *   1. a severity dot (amber/degraded for a retryable deploy failure),
 *   2. a collapsible notice carrying the REAL error detail (not a generic
 *      sentinel), and
 *   3. a persistent re-expand handle (the seam/hit-region survives a collapse).
 *
 * Root cause it locks down: the deploy-progress `failed` listener used to DROP
 * `p.message` and `advancePublishStage`'s failed branch set only a generic
 * `lastFailure` with NO advisory + NO surface, so `topSeverity` was "none"
 * (no dot), `hasContent()` was false (no seam/handle), and once collapsed the
 * failure could never be re-opened.
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { emitEngineEvent, engineSubscribers, resetTestHost } from "../../__tests__/helpers/test-host";

import { ProgressPanel } from './progress-panel';

/** Fire an engine event into every subscriber init() registered on `channel`. */
function emitOn(channel: string, payload: unknown): void {
  if (engineSubscribers(channel) === 0) throw new Error(`no '${channel}' listener registered by init()`);
  emitEngineEvent(channel, payload);
}

/** Fire a `deploy-progress` event into the panel's registered listener. */
function emitDeploy(payload: Record<string, unknown>): void {
  emitOn('deploy-progress', payload);
}

/** Fire a MossEvent (tagged {kind, payload}) into the shared channel. */
function emitMossEvent(kind: string, payload: unknown): void {
  emitOn('moss-event', { kind, payload });
}

/** Reach the private members the surface behaviour is defined in terms of. */
type Internals = {
  hasContent(): boolean;
  advisories: Map<string, unknown>;
};
const internals = (p: ProgressPanel): Internals => p as unknown as Internals;

describe('ProgressPanel — publish/deploy failure advisory', () => {
  let panel: ProgressPanel;

  beforeEach(async () => {
    resetTestHost();
    panel = new ProgressPanel(document.createElement('div'));
    await panel.init();
  });

  it('sets an advisory carrying the real error detail from p.message', () => {
    emitDeploy({
      stage: 'failed',
      current: 0,
      total: 0,
      message: 'Connection refused (bad endpoint)',
    });

    const advisories = panel.getAdvisories();
    expect(advisories.length).toBeGreaterThan(0);
    const reasons = advisories.map((a) => a.reason).join(' | ');
    expect(reasons).toContain('Connection refused (bad endpoint)');
  });

  it('renders the amber/degraded severity dot (not "none", not red "broken")', () => {
    emitDeploy({ stage: 'failed', current: 0, total: 0, message: 'DNS lookup failed' });

    const vm = panel.buildViewModel();
    expect(vm.topSeverity).toBe('degraded');

    // The error text is the notice DETAIL; the head is the generic title. This
    // proves the real detail — not just the sentinel — reaches the surface.
    const notice = vm.notices.find((n) => n.detail?.includes('DNS lookup failed'));
    expect(notice).toBeDefined();
    expect(notice!.severity).toBe('degraded');
    expect(notice!.detail).toContain('DNS lookup failed');
    expect(notice!.reason).not.toContain('DNS lookup failed'); // head = title only
  });

  it('makes the seam show — hasContent() is true after a failure', () => {
    emitDeploy({ stage: 'failed', current: 0, total: 0, message: 'boom' });
    expect(internals(panel).hasContent()).toBe(true);
  });

  it('keeps the re-expand handle alive after the body is collapsed', () => {
    emitDeploy({ stage: 'failed', current: 0, total: 0, message: 'boom' });
    // User collapses the advisory body.
    panel.hide();
    // The seam + reveal hit-region must survive so it can be re-opened.
    expect(internals(panel).hasContent()).toBe(true);
  });

  it('hasContent() stays true from lastFailure alone (belt-and-suspenders gate)', () => {
    emitDeploy({ stage: 'failed', current: 0, total: 0, message: 'boom' });
    // Defence in depth: even if the advisory is cleared out from under us, a
    // red publish status must keep the seam + reveal handle alive.
    internals(panel).advisories.clear();
    expect(internals(panel).hasContent()).toBe(true);
  });

  it('carries the detail (message), never the generic sentinel, into the notice', () => {
    emitDeploy({ stage: 'failed', current: 0, total: 0, message: 'certificate expired' });

    const vm = panel.buildViewModel();
    const details = vm.notices.map((n) => n.detail).filter(Boolean);
    expect(details.some((d) => d!.includes('certificate expired'))).toBe(true);
    // The generic "Publish failed" is the status/head — it must never be the
    // detail that supplants the real error.
    expect(details).not.toContain('Publish failed');
  });
});

describe('ProgressPanel — overdue-build advisory folder scope', () => {
  let panel: ProgressPanel;

  const degraded = (folder: string, degraded: boolean) =>
    emitMossEvent('FolderHealthChanged', { folder, unavailable: false, degraded });

  const overdueShown = () =>
    panel.getAdvisories().some((a) => a.task === 'build-overdue');

  beforeEach(async () => {
    resetTestHost();
    panel = new ProgressPanel(document.createElement('div'));
    await panel.init();
    panel.setFolder('/vault/a');
  });

  it('shows on degraded=true and clears on the all-clear transition', () => {
    degraded('/vault/a', true);
    expect(overdueShown()).toBe(true);
    degraded('/vault/a', false);
    expect(overdueShown()).toBe(false);
  });

  it("ignores another folder's health — a stale sweep tick is not news", () => {
    degraded('/vault/b', true);
    expect(overdueShown()).toBe(false);
  });

  it('drops a stale advisory on folder switch — the old all-clear never arrives', () => {
    // FolderHealthChanged is transitions-only and folder A's sweep is
    // cancelled at close BEFORE it can emit degraded=false, so without the
    // setFolder clear this advisory would outlive the switch indefinitely.
    degraded('/vault/a', true);
    expect(overdueShown()).toBe(true);
    panel.setFolder('/vault/b');
    expect(overdueShown()).toBe(false);
    // And folder A's late, stale tick cannot resurrect it.
    degraded('/vault/a', true);
    expect(overdueShown()).toBe(false);
  });
});
