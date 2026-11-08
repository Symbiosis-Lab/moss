import { describe, it, expect } from 'vitest';
import {
  computeStageLabel,
  nextStage,
  computePublishLabel,
  nextPublishStage,
  formatStatus,
  noticeTier,
  groupNotices,
} from './progress-panel-stage';
import type { AdvisoryEntry } from './progress-panel';

describe('progress-panel stage state machine', () => {
  it('opening -> indeterminate "Opening folder…"', () => {
    const label = computeStageLabel({ stage: 'opening' });
    expect(label).toEqual({ text: 'Opening folder…', percent: null });
  });

  it('scanning shows running found count, indeterminate bar', () => {
    const label = computeStageLabel({ stage: 'scanning', found: 42 });
    expect(label.text).toContain('Scanning files');
    expect(label.text).toContain('42');
    expect(label.percent).toBeNull();
  });

  it('materializing shows provider and determinate progress', () => {
    const label = computeStageLabel({
      stage: 'materializing',
      provider: 'iCloud',
      downloaded: 12,
      total: 340,
    });
    expect(label.text).toContain('iCloud');
    expect(label.text).toContain('12');
    expect(label.text).toContain('340');
    expect(label.percent).toBeCloseTo((12 / 340) * 100, 0);
  });

  it('compiling shows determinate page progress', () => {
    const label = computeStageLabel({
      stage: 'compiling',
      phase: 'pages',
      current: 5,
      total: 20,
    });
    expect(label.percent).toBe(25);
  });

  it('nextStage: scan-complete with no evicted skips materializing', () => {
    expect(nextStage('scanning', { kind: 'scan-complete', evictedCount: 0 }))
      .toBe('compiling');
  });

  it('nextStage: scan-complete with evicted goes to materializing', () => {
    expect(nextStage('scanning', { kind: 'scan-complete', evictedCount: 340 }))
      .toBe('materializing');
  });

  it('nextStage: first build progress advances from materializing', () => {
    expect(nextStage('materializing', { kind: 'build-progress' }))
      .toBe('compiling');
  });

  it('nextStage: first scan progress advances from opening', () => {
    expect(nextStage('opening', { kind: 'scan-progress' })).toBe('scanning');
  });

  it('nextStage is monotonic: build-progress after ready does not regress', () => {
    // Regression guard for the "Generating pages… (N / N)" stuck-panel bug.
    // After the early server-ready ProgressUpdate flips status to 'ready'
    // (because site/ already had content from a prior build), background
    // image/asset tasks keep emitting BackgroundProgress events. Without
    // the monotonic guard, the stage rewinds 'ready' → 'compiling' and
    // never returns, because the silent-rebuild branch in preview-state.ts
    // swallows the final ProgressUpdate.
    expect(nextStage('ready', { kind: 'build-progress' })).toBe('ready');
  });

  it('nextStage is monotonic: late scan-complete after compiling stays compiling', () => {
    // The icloud-sync handler used to direct-mutate to side-step this.
    // Now the state machine itself prevents the regression.
    expect(nextStage('compiling', { kind: 'scan-complete', evictedCount: 5 }))
      .toBe('compiling');
    expect(nextStage('ready', { kind: 'scan-complete', evictedCount: 5 }))
      .toBe('ready');
  });

  it('nextStage is monotonic: server-ready then a stray scan-progress stays ready', () => {
    expect(nextStage('ready', { kind: 'scan-progress' })).toBe('ready');
  });

  it('compiling with no counts yet says so plainly, not "(0 / 0)"', () => {
    // The window between the first build-progress tick (which advances the
    // stage to 'compiling') and the first `html_pages` BackgroundProgress is
    // the whole markdown render, most of a large site's build.
    // `stageContext.pages` is unset for all of it and the call site
    // defaults the counts to 0, so the interpolated form rendered
    // "Formatting your writing… (0 / 0)" on the full-window waiting screen: a
    // counter pinned at zero reads as a build that has stopped.
    const label = computeStageLabel({
      stage: 'compiling',
      phase: 'markdown',
      current: 0,
      total: 0,
    });
    expect(label.text).toBe('Formatting your writing…');
    expect(label.percent).toBeNull();
  });

  it('compiling names the phase, so a moving denominator moves with the verb', () => {
    // The two page passes have different denominators (`html_total` counts
    // synthetic documents `markdown_total` never saw), which is the "total
    // goes up by one at the end" the old shared counter showed. Naming the
    // pass is what makes that change read as progress instead of a glitch.
    const first = computeStageLabel({
      stage: 'compiling',
      phase: 'markdown',
      current: 553,
      total: 553,
    });
    const second = computeStageLabel({
      stage: 'compiling',
      phase: 'pages',
      current: 40,
      total: 554,
    });
    expect(first.text).toBe('Formatting your writing… (553 / 553)');
    expect(second.text).toBe('Generating pages… (40 / 554)');
  });

  it('materializing with total=0 returns null percent', () => {
    const label = computeStageLabel({
      stage: 'materializing',
      provider: 'iCloud',
      downloaded: 0,
      total: 0,
    });
    expect(label.percent).toBeNull();
  });
});

describe('publish stage labels', () => {
  it('waiting -> "Waiting for build…", indeterminate', () => {
    expect(computePublishLabel({ stage: 'waiting' })).toEqual({
      text: 'Waiting for build…',
      percent: null,
    });
  });

  it('preparing -> "Preparing to publish…", indeterminate', () => {
    expect(computePublishLabel({ stage: 'preparing' })).toEqual({
      text: 'Preparing to publish…',
      percent: null,
    });
  });

  it('rebuilding shows backend message when present', () => {
    const label = computePublishLabel({ stage: 'rebuilding', message: 'No rebuild needed' });
    expect(label.text).toBe('No rebuild needed');
    expect(label.percent).toBeNull();
  });

  it('rebuilding falls back to default when message empty', () => {
    expect(computePublishLabel({ stage: 'rebuilding' }).text).toBe('Rebuilding…');
  });

  it('syncing -> "Comparing with server…"', () => {
    expect(computePublishLabel({ stage: 'syncing' }).text).toBe('Comparing with server…');
  });

  it('uploading shows determinate fraction', () => {
    const label = computePublishLabel({ stage: 'uploading', current: 3, total: 12 });
    expect(label.text).toContain('Uploading');
    expect(label.text).toContain('3');
    expect(label.text).toContain('12');
    expect(label.percent).toBe(25);
  });

  it('uploading with total=0 returns null percent', () => {
    expect(computePublishLabel({ stage: 'uploading', current: 0, total: 0 }).percent).toBeNull();
  });

  it('uploading WITH bytes drops the file count from the verb', () => {
    // On the bytes path the numbers move to the count slot as
    // `12.4/58.0 MB · 2.1 MB/s`; four numbers on an 11px muted line is a
    // dashboard, and the file count is the least informative of the four.
    // `percent` remains unchanged because no stage consumer reads it.
    const label = computePublishLabel({
      stage: 'uploading',
      current: 7,
      total: 214,
      bytesDone: 12.4e6,
      bytesTotal: 58e6,
    });
    expect(label.text).toBe('Uploading…');
    expect(label.text).not.toContain('214');
  });

  it('uploading WITHOUT bytes keeps the file-count form (degradation branch)', () => {
    // bytes_total is absent on the prebuilt CLI path and 0 on a commit-only
    // deploy — both fall back to today's exact behaviour.
    expect(
      computePublishLabel({ stage: 'uploading', current: 7, total: 214 }).text,
    ).toBe('Uploading… (7 / 214)');
    expect(
      computePublishLabel({
        stage: 'uploading',
        current: 7,
        total: 214,
        bytesTotal: 0,
      }).text,
    ).toBe('Uploading… (7 / 214)');
  });

  it('a sub-10 kB total keeps the file count — `0/0 kB` says less', () => {
    // The kB rung has no decimals, so a one-post deploy would spend its whole
    // life reading `0/0 kB`: strictly worse than the count it replaced.
    expect(
      computePublishLabel({
        stage: 'uploading',
        current: 1,
        total: 1,
        bytesDone: 400,
        bytesTotal: 900,
      }).text,
    ).toBe('Uploading… (1 / 1)');
    // ...and the first total with a moving column does take the bytes path.
    expect(
      computePublishLabel({
        stage: 'uploading',
        current: 1,
        total: 1,
        bytesDone: 400,
        bytesTotal: 10_000,
      }).text,
    ).toBe('Uploading…');
  });

  it('committing -> "Making changes live…"', () => {
    expect(computePublishLabel({ stage: 'committing' }).text).toBe('Making changes live…');
  });

  it('finalizing -> "Finalizing…"', () => {
    expect(computePublishLabel({ stage: 'finalizing' }).text).toBe('Finalizing…');
  });

  it('complete and failed return empty text (panel auto-closes)', () => {
    expect(computePublishLabel({ stage: 'complete' }).text).toBe('');
    expect(computePublishLabel({ stage: 'failed' }).text).toBe('');
  });
});

describe('nextPublishStage', () => {
  it('idle -> waiting on waiting event', () => {
    expect(nextPublishStage('idle', { kind: 'waiting' })).toBe('waiting');
  });

  it('waiting -> preparing on first progress event', () => {
    expect(nextPublishStage('waiting', { kind: 'progress', stage: 'preparing' }))
      .toBe('preparing');
  });

  it('idle -> preparing if first event is progress (no waiting fired)', () => {
    expect(nextPublishStage('idle', { kind: 'progress', stage: 'preparing' }))
      .toBe('preparing');
  });

  it('progress events drive through linear stages', () => {
    expect(nextPublishStage('preparing', { kind: 'progress', stage: 'syncing' })).toBe('syncing');
    expect(nextPublishStage('syncing', { kind: 'progress', stage: 'uploading' })).toBe('uploading');
    expect(nextPublishStage('uploading', { kind: 'progress', stage: 'committing' })).toBe('committing');
    expect(nextPublishStage('committing', { kind: 'progress', stage: 'finalizing' })).toBe('finalizing');
  });

  it('complete event -> complete', () => {
    expect(nextPublishStage('finalizing', { kind: 'complete' })).toBe('complete');
  });

  it('failed event -> failed from any stage', () => {
    expect(nextPublishStage('uploading', { kind: 'failed' })).toBe('failed');
    expect(nextPublishStage('preparing', { kind: 'failed' })).toBe('failed');
  });

  it('late waiting event after preparing does not regress (stays preparing)', () => {
    expect(nextPublishStage('preparing', { kind: 'waiting' })).toBe('preparing');
    expect(nextPublishStage('uploading', { kind: 'waiting' })).toBe('uploading');
  });
});

// formatStatus — pure status-line formatter
describe('formatStatus', () => {
  it('running with subject and count — "Optimizing" / "intro.mov" / "3 / 12"', () => {
    const result = formatStatus({
      kind: 'running',
      verb: 'Optimizing',
      subject: 'intro.mov',
      current: 3,
      total: 12,
    });
    expect(result.text).toBe('Optimizing');
    expect(result.subject).toBe('intro.mov');
    expect(result.count).toBe('3 / 12');
    expect(result.kind).toBe('running');
  });

  // The running variant's optional slots, one table instead of four
  // near-duplicate blocks. `countText` is the deploy-upload readout: it is not
  // an "n of m" fraction, so it arrives pre-composed and WINS over the pair.
  it.each([
    {
      name: 'verb only',
      input: { kind: 'running', verb: 'Scanning files…' } as const,
      subject: undefined,
      count: undefined,
    },
    {
      name: 'verb + subject, no fraction',
      input: { kind: 'running', verb: 'Building', subject: 'intro.md' } as const,
      subject: 'intro.md',
      count: undefined,
    },
    {
      name: 'countText, no pair',
      input: {
        kind: 'running',
        verb: 'Uploading…',
        countText: '12.4/58.0 MB · 2.1 MB/s',
      } as const,
      subject: undefined,
      count: '12.4/58.0 MB · 2.1 MB/s',
    },
    {
      name: 'countText WINS over current/total',
      input: {
        kind: 'running',
        verb: 'Uploading…',
        current: 7,
        total: 214,
        countText: '12.4/58.0 MB · 2.1 MB/s',
      } as const,
      subject: undefined,
      count: '12.4/58.0 MB · 2.1 MB/s',
    },
  ])('running — $name', ({ input, subject, count }) => {
    const result = formatStatus(input);
    expect(result.kind).toBe('running');
    expect(result.text).toBe(input.verb);
    expect(result.subject).toBe(subject);
    expect(result.count).toBe(count);
  });

  it('receipt — "Built · 47 pages · 3.2s"', () => {
    const result = formatStatus({
      kind: 'receipt',
      text: 'Built · 47 pages · 3.2s',
    });
    expect(result.text).toBe('Built · 47 pages · 3.2s');
    expect(result.subject).toBeUndefined();
    expect(result.count).toBeUndefined();
    expect(result.kind).toBe('receipt');
  });

  it('failed — "Publish failed"', () => {
    const result = formatStatus({ kind: 'failed', text: 'Publish failed' });
    expect(result.text).toBe('Publish failed');
    expect(result.kind).toBe('failed');
  });

  it('none — empty text, kind none', () => {
    const result = formatStatus({ kind: 'none' });
    expect(result.text).toBe('');
    expect(result.kind).toBe('none');
  });
});

// noticeTier + groupNotices
describe('noticeTier', () => {
  it('Blocking → broken regardless of scope', () => {
    expect(noticeTier('Blocking', 'File')).toBe('broken');
    expect(noticeTier('Blocking', 'Environment')).toBe('broken');
    expect(noticeTier('Blocking', undefined)).toBe('broken');
  });

  it('NeedsAction + File → broken', () => {
    expect(noticeTier('NeedsAction', 'File')).toBe('broken');
  });

  it('NeedsAction + Environment → degraded', () => {
    expect(noticeTier('NeedsAction', 'Environment')).toBe('degraded');
  });

  it('NeedsAction + Remote → degraded', () => {
    expect(noticeTier('NeedsAction', 'Remote')).toBe('degraded');
  });

  it('NeedsAction + Account → degraded', () => {
    expect(noticeTier('NeedsAction', 'Account')).toBe('degraded');
  });

  it('NeedsAction + Config → degraded', () => {
    expect(noticeTier('NeedsAction', 'Config')).toBe('degraded');
  });

  it('NeedsAction with no scope → degraded', () => {
    expect(noticeTier('NeedsAction', undefined)).toBe('degraded');
  });

  it('ShippedDegraded → degraded regardless of scope', () => {
    expect(noticeTier('ShippedDegraded', 'File')).toBe('degraded');
    expect(noticeTier('ShippedDegraded', 'Environment')).toBe('degraded');
    expect(noticeTier('ShippedDegraded', undefined)).toBe('degraded');
  });

  it('undefined severity → degraded', () => {
    expect(noticeTier(undefined, undefined)).toBe('degraded');
  });
});

describe('groupNotices', () => {
  function adv(overrides: Partial<AdvisoryEntry>): AdvisoryEntry {
    return {
      task: 'videos',
      item: null,
      reason: 'test reason',
      ...overrides,
    };
  }

  it('excludes awaiting-narration entries via isAwaitingNarration predicate', () => {
    const advisories: AdvisoryEntry[] = [
      adv({ task: 'awaiting-narration', item: null, reason: 'Waiting for you' }),
      adv({ task: 'videos', item: 'a.mov', reason: 'FFmpeg not found', severity: 'Blocking', scope: 'Environment' }),
    ];
    const notices = groupNotices(advisories);
    expect(notices.length).toBe(1);
    expect(notices[0].reason).not.toContain('Waiting');
  });

  it('groups by (task, reason-head) — collapses same-cause files', () => {
    const reason = 'still downloading from iCloud';
    const advisories: AdvisoryEntry[] = [
      adv({ task: 'videos', item: 'a.mov', reason, severity: 'ShippedDegraded', scope: 'File' }),
      adv({ task: 'videos', item: 'b.mov', reason, severity: 'ShippedDegraded', scope: 'File' }),
      adv({ task: 'videos', item: 'c.mov', reason, severity: 'ShippedDegraded', scope: 'File' }),
    ];
    const notices = groupNotices(advisories);
    expect(notices.length).toBe(1);
    expect(notices[0].files.length + notices[0].hiddenFiles.length).toBe(3);
  });

  it('splits what at first " — " into reason(head) + detail(tail)', () => {
    const advisories: AdvisoryEntry[] = [
      adv({
        task: 'videos',
        item: null,
        reason: 'FFmpeg not found — install via Homebrew: brew install ffmpeg',
        severity: 'NeedsAction',
        scope: 'Environment',
      }),
    ];
    const notices = groupNotices(advisories);
    expect(notices.length).toBe(1);
    expect(notices[0].reason).toBe('FFmpeg not found');
    expect(notices[0].detail).toBe('install via Homebrew: brew install ffmpeg');
  });

  it('reason with no em-dash has no detail', () => {
    const advisories: AdvisoryEntry[] = [
      adv({ task: 'videos', item: 'a.mov', reason: 'simple reason', severity: 'ShippedDegraded', scope: 'File' }),
    ];
    const notices = groupNotices(advisories);
    expect(notices[0].detail).toBeUndefined();
  });

  it('broken notices come before degraded notices', () => {
    const advisories: AdvisoryEntry[] = [
      adv({ task: 'images', item: 'a.png', reason: 'iCloud', severity: 'ShippedDegraded', scope: 'File' }),
      adv({ task: 'videos', item: null, reason: 'FFmpeg missing', severity: 'Blocking', scope: 'Environment' }),
    ];
    const notices = groupNotices(advisories);
    expect(notices[0].severity).toBe('broken');
    expect(notices[1].severity).toBe('degraded');
  });

  it('caps files at FILES_BUDGET=6, puts overflow in hiddenFiles', () => {
    const advisories: AdvisoryEntry[] = Array.from({ length: 9 }, (_, i) =>
      adv({ task: 'videos', item: `v${i}.mov`, reason: 'same cause', severity: 'ShippedDegraded', scope: 'File' }),
    );
    const notices = groupNotices(advisories);
    expect(notices.length).toBe(1);
    expect(notices[0].files.length).toBe(6);
    expect(notices[0].hiddenFiles.length).toBe(3);
  });

  it('carries command from Action::Command — uses wire key {run}', () => {
    const advisories: AdvisoryEntry[] = [
      adv({
        task: 'videos',
        item: null,
        reason: 'FFmpeg not installed',
        severity: 'NeedsAction',
        scope: 'Environment',
        action: { Command: { run: 'brew install ffmpeg', label: 'Install' } },
      }),
    ];
    const notices = groupNotices(advisories);
    expect(notices[0].command).toEqual({ run: 'brew install ffmpeg' });
  });

  it('carries action for InApp/Link advisory', () => {
    const action = { InApp: { op: 'SignIn', args: null, label: 'Open Settings' } } as const;
    const advisories: AdvisoryEntry[] = [
      adv({
        task: 'auth',
        item: null,
        reason: 'Sign in required',
        severity: 'NeedsAction',
        scope: 'Account',
        action,
      }),
    ];
    const notices = groupNotices(advisories);
    expect(notices[0].action).toEqual(action);
  });

  it('derives tier word from noticeTier — does NOT parse what', () => {
    const advisories: AdvisoryEntry[] = [
      adv({ task: 'videos', item: null, reason: 'broken thing', severity: 'Blocking', scope: 'Environment' }),
    ];
    const notices = groupNotices(advisories);
    // Severity should be "broken" as derived from noticeTier
    expect(notices[0].severity).toBe('broken');
  });
});
