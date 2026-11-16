import { describe, it, expect, beforeEach } from 'vitest';
import { PreviewState, PreviewStatus, resolveUnshowableReason, type UnshowableInputs } from '../preview-state';
import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { host } from '../../host';
import { resetTestHost } from '../../__tests__/helpers/test-host';

// Helper to create a progress update with defaults
function createProgressUpdate(overrides: Partial<ProgressUpdate> = {}): ProgressUpdate {
  return {
    step: 'test',
    message: 'Test message',
    percentage: 0,
    completed: false,
    port: null,
    is_empty: null,
    ...overrides,
  };
}

describe('PreviewState', () => {
  let state: PreviewState;

  beforeEach(() => {
    state = new PreviewState();
  });

  describe('Initial State', () => {
    it('starts in initial state', () => {
      expect(state.getStatus()).toEqual({ type: 'initial' });
    });

    it('is not server ready initially', () => {
      expect(state.isServerReady()).toBe(false);
    });

    it('has null server URL initially', () => {
      expect(state.getServerUrl()).toBe(null);
    });
  });

  describe('Initial Build Flow', () => {
    it('transitions to loading on first progress update', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        step: 'scanning',
        message: 'Scanning folder...',
        percentage: 20,
        completed: false,
      }));

      expect(status).toEqual({
        type: 'loading',
        message: 'Scanning folder...',
        percentage: 20,
      });
      expect(state.isServerReady()).toBe(false);
    });

    it('updates loading progress on subsequent updates', () => {
      state.handleProgressUpdate(createProgressUpdate({
        message: 'Scanning...',
        percentage: 20,
      }));

      const status = state.handleProgressUpdate(createProgressUpdate({
        message: 'Generating site...',
        percentage: 50,
      }));

      expect(status).toEqual({
        type: 'loading',
        message: 'Generating site...',
        percentage: 50,
      });
    });

    it('transitions to ready on completion with port', () => {
      state.handleProgressUpdate(createProgressUpdate({ percentage: 50 }));

      const status = state.handleProgressUpdate(createProgressUpdate({
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.isServerReady()).toBe(true);
      expect(state.getServerUrl()).toBe('http://localhost:3000');
    });

    it('goes ready on empty completion (served empty-home is real content) and records empty-ness', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      }));

      // No more `empty` status — the preview loads the served homepage.
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.isServerReady()).toBe(true);
      // Empty-ness is surfaced separately for the app shell's onboarding decision.
      expect(state.lastCompletionWasEmpty()).toBe(true);
    });

    it('handles completion without is_empty flag (defaults to ready)', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: null,
      }));

      // null is falsy, so should be treated as not empty
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });
  });

  describe('Silent Rebuilds', () => {
    beforeEach(() => {
      // Set up initial ready state
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));
    });

    it('ignores progress updates after server is ready', () => {
      expect(state.getStatus()).toEqual({ type: 'ready', url: 'http://localhost:3000' });

      // Rebuild starts - should be silent
      const status = state.handleProgressUpdate(createProgressUpdate({
        message: 'Rebuilding...',
        percentage: 50,
        completed: false,
      }));

      // Status unchanged
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });

    it('stays ready on rebuild completion when site has content', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });

    it('returns SAME status object on rebuild completion (prevents unnecessary reload)', () => {
      // This test verifies the fix for the bug where rebuild completion
      // triggered a reload even when the current page wasn't affected.
      // The fix is to NOT create a new status object, so previousStatus !== newStatus
      // will be false in PreviewManager.handleProgressUpdate.

      const statusBeforeRebuild = state.getStatus();

      // Simulate rebuild completion
      const statusAfterRebuild = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      // Critical: The SAME object reference should be returned
      // This prevents PreviewManager from calling renderStatus
      expect(statusAfterRebuild).toBe(statusBeforeRebuild);
    });

    it('stays ready (not empty status) on rebuild completion when site becomes empty', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      }));

      // Preview stays ready; empty-ness recorded separately.
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.lastCompletionWasEmpty()).toBe(true);
    });
  });

  describe('Empty-ness tracking across rebuilds', () => {
    beforeEach(() => {
      // Set up initial empty completion — preview is ready, empty recorded.
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      }));
    });

    it('clears the empty flag when content is added', () => {
      expect(state.getStatus()).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.lastCompletionWasEmpty()).toBe(true);

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.lastCompletionWasEmpty()).toBe(false);
    });

    it('keeps status ready and empty flag unchanged during a silent rebuild', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        message: 'Rebuilding...',
        percentage: 50,
        completed: false,
      }));

      // Status unchanged - still ready; non-complete updates don't touch the flag.
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.lastCompletionWasEmpty()).toBe(true);
    });
  });

  describe('Error State', () => {
    it('can set error state', () => {
      state.setError('Something went wrong');
      expect(state.getStatus()).toEqual({
        type: 'error',
        message: 'Something went wrong',
        verdict: 'reported',
      });
    });

    it('can set error from ready state', () => {
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      state.setError('Server crashed');
      expect(state.getStatus()).toEqual({
        type: 'error',
        message: 'Server crashed',
        verdict: 'reported',
      });
    });

    // ─── Told vs inferred ────────────────────────────────────────
    // A completion carries no port whenever the pipeline that sent it did not
    // start the server — the watcher rebuild, the cloud supervisor's arrival
    // rebuild. That is not evidence the server is down, and on the incident
    // vault it arrived while :8080 had been serving for nineteen seconds. The
    // typed `MossEvent::PreviewServerFailed` is what carries a real failure,
    // so the state machine reads the verdict off that rather than off a
    // missing field.

    it('a portless completion nobody reported a failure for is UNVERIFIED', () => {
      state.handleProgressUpdate(createProgressUpdate({ percentage: 50 }));

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      }));

      expect(status.type).toBe('error');
      expect((status as { verdict: string }).verdict).toBe('unverified');
    });

    it('a portless completion AFTER PreviewServerFailed is a REPORTED verdict', () => {
      state.handleProgressUpdate(createProgressUpdate({ percentage: 50 }));
      // Rust starts the server before anything else and emits this on the Err
      // arm, so the report always precedes the completion it explains.
      state.noteServerFailed('Address already in use (os error 48)', false);

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      }));

      expect(status.type).toBe('error');
      expect((status as { verdict: string }).verdict).toBe('reported');
    });

    it('a reported failure carries the server\'s words and whether another app has the folder', () => {
      state.noteServerFailed('this folder is already served by another moss process', true);
      const held = state.handleProgressUpdate(createProgressUpdate({ percentage: 100, completed: true, port: null, is_empty: false }));
      expect(held).toEqual({
        type: 'error',
        message: 'this folder is already served by another moss process',
        cause: 'other-app',
        verdict: 'reported',
      });

      state.reset();
      state.noteServerFailed('Address already in use (os error 48)', false);
      const unheld = state.handleProgressUpdate(createProgressUpdate({ percentage: 100, completed: true, port: null, is_empty: false }));
      expect(unheld).toMatchObject({ message: 'Address already in use (os error 48)', cause: 'server' });
    });

    it('a portless completion from another pipeline never unseats a live server', () => {
      // The bug's own shape: one pipeline started the server and said so, a
      // second (start_server=false) finished later carrying no port. The
      // second must not paint the first one's live server as dead.
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 8080,
        is_empty: false,
      }));

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      }));

      expect(status).toEqual({ type: 'ready', url: 'http://localhost:8080' });
    });

    it('a port retires the failure record it contradicts, mid-build and all', () => {
      // The record says a preview server did not start. A port is proof one
      // did, so it stops being evidence the moment one arrives — and NOT only
      // on `reset()`, which runs on a folder switch: a server that has since
      // started in the SAME folder falsifies the record just as much.
      //
      // The update used here is the reachable shape. `build.rs`'s build-start
      // event carries `port: Some(p)` with `is_empty: None`, and the
      // early-preview branch below requires `is_empty === false` — so this
      // update proves :8080 is up WITHOUT setting `serverUrl`, which is
      // exactly the state the record is still readable in. Left standing, it
      // would upgrade the next portless completion to `reported`, and
      // `reported` is the verdict allowed to tear down a live download
      // screen.
      state.noteServerFailed('Address already in use (os error 48)', false);
      state.handleProgressUpdate(createProgressUpdate({
        step: 'build',
        percentage: 10,
        completed: false,
        port: 8080,
        is_empty: null,
      }));

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      }));

      expect((status as { verdict: string }).verdict).toBe('unverified');
    });

    it('a reported failure is forgotten on reset, so the next folder starts clean', () => {
      state.noteServerFailed('Address already in use (os error 48)', false);
      state.reset();

      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      }));

      expect((status as { verdict: string }).verdict).toBe('unverified');
    });
  });

  // ─── The one-unshowable-surface ladder ───
  describe('resolveUnshowableReason', () => {
    const BASE: UnshowableInputs = {
      unreachable: false,
      gateUp: false,
      previewBlank: false,
      errorVerdict: null,
      pagePending: false,
      blocking: 0,
      stalled: false,
      buildStageLabel: null,
    };
    const inputs = (overrides: Partial<UnshowableInputs>): UnshowableInputs => ({ ...BASE, ...overrides });

    // One row per rank, in ladder order (rank 1 = highest).
    it.each([
      ['unreachable', inputs({ unreachable: true }), 'unreachable'],
      ['gate', inputs({ gateUp: true }), 'gate'],
      ['error-hard', inputs({ previewBlank: true, errorVerdict: 'reported' }), 'error-hard'],
      ['page-pending', inputs({ pagePending: true }), 'page-pending'],
      ['stalled', inputs({ previewBlank: true, blocking: 1, stalled: true }), 'stalled'],
      ['downloading', inputs({ previewBlank: true, blocking: 1, stalled: false }), 'downloading'],
      ['error-soft', inputs({ previewBlank: true, errorVerdict: 'unverified' }), 'error-soft'],
      ['building', inputs({ previewBlank: true, buildStageLabel: 'Compiling…' }), 'building'],
      ['opening', inputs({ previewBlank: true }), 'opening'],
      ['served (null)', inputs({}), null],
    ])('rank %s', (_name, input, expected) => {
      expect(resolveUnshowableReason(input)).toBe(expected);
    });

    // Ordering pairs over the ladder's ranks.
    it('unreachable outranks everything', () => {
      expect(resolveUnshowableReason(inputs({
        unreachable: true, gateUp: true, previewBlank: true, errorVerdict: 'reported', blocking: 1,
      }))).toBe('unreachable');
    });

    it('gate outranks a reported (hard) error', () => {
      expect(resolveUnshowableReason(inputs({
        gateUp: true, previewBlank: true, errorVerdict: 'reported',
      }))).toBe('gate');
    });

    it('gate outranks an unverified (soft) error', () => {
      expect(resolveUnshowableReason(inputs({
        gateUp: true, previewBlank: true, errorVerdict: 'unverified',
      }))).toBe('gate');
    });

    it('a hard error outranks stalled downloads', () => {
      expect(resolveUnshowableReason(inputs({
        previewBlank: true, errorVerdict: 'reported', blocking: 1, stalled: true,
      }))).toBe('error-hard');
    });

    it('a hard error outranks an ordinary download', () => {
      expect(resolveUnshowableReason(inputs({
        previewBlank: true, errorVerdict: 'reported', blocking: 1, stalled: false,
      }))).toBe('error-hard');
    });

    it('a hard error wins even with a site already served (previewBlank false)', () => {
      // Unlike ranks 4-8, rank 3 is not gated behind `previewBlank`: a
      // REPORTED failure (`verdict !== 'unverified' || !screenUp`)
      // wins unconditionally once past ranks 1-2, because a server that just
      // failed means the cached site underneath can no longer be trusted.
      expect(resolveUnshowableReason(inputs({
        previewBlank: false, errorVerdict: 'reported',
      }))).toBe('error-hard');
    });

    it('a download in progress outranks a soft error', () => {
      // The inverse bug this preserves: a soft (unverified) error must not
      // steal the window from a progress bar that is still moving.
      expect(resolveUnshowableReason(inputs({
        previewBlank: true, errorVerdict: 'unverified', blocking: 1,
      }))).toBe('downloading');
    });

    it('a new page waits even though a site is served (previewBlank false)', () => {
      // Rank 4 is checked before the previewBlank gate for the same reason
      // rank 2 is: what is served underneath is the synthetic 404 of the page
      // the author just created.
      expect(resolveUnshowableReason(inputs({
        previewBlank: false, pagePending: true,
      }))).toBe('page-pending');
    });

    it('a reported error outranks a new page waiting on its build', () => {
      // The build that would have produced the page just failed out loud;
      // saying "preparing this page" over that would be a promise moss has
      // already broken.
      expect(resolveUnshowableReason(inputs({
        pagePending: true, errorVerdict: 'reported',
      }))).toBe('error-hard');
    });

    it('the home gate outranks a new page waiting on its build', () => {
      expect(resolveUnshowableReason(inputs({
        pagePending: true, gateUp: true,
      }))).toBe('gate');
    });

    it('a soft error outranks the bare building stage', () => {
      // The inverse bug: an error that can never be displayed leaves the user
      // on a progress bar that has stopped moving.
      expect(resolveUnshowableReason(inputs({
        previewBlank: true, errorVerdict: 'unverified', buildStageLabel: 'Compiling…',
      }))).toBe('error-soft');
    });
  });

  describe('Reset', () => {
    it('resets to initial state', () => {
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      expect(state.isServerReady()).toBe(true);

      state.reset();

      expect(state.getStatus()).toEqual({ type: 'initial' });
      expect(state.isServerReady()).toBe(false);
      expect(state.getServerUrl()).toBe(null);
    });
  });

  describe('Early Port (Instant Preview)', () => {
    it('transitions to ready early when port and content available before completion', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        step: 'server-ready',
        message: 'Loading preview...',
        percentage: 5,
        completed: false,
        port: 3000,
        is_empty: false,
      }));
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
      expect(state.isServerReady()).toBe(true);
    });

    it('treats subsequent progress as silent rebuild after early port', () => {
      // Early port arrives — preview shows immediately
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 5,
        completed: false,
        port: 3000,
        is_empty: false,
      }));

      // Later progress events should be silent (rebuild behavior)
      const status = state.handleProgressUpdate(createProgressUpdate({
        message: 'Generating site...',
        percentage: 50,
        completed: false,
      }));
      expect(status).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });

    it('handles completion after early port without changing status', () => {
      // Early port — instant preview
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 5,
        completed: false,
        port: 3000,
        is_empty: false,
      }));
      const readyStatus = state.getStatus();

      // Build completes — should return SAME object (no unnecessary reload)
      const completionStatus = state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));
      expect(completionStatus).toBe(readyStatus);
    });

    it('does NOT transition early when is_empty is null (unknown)', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 5,
        completed: false,
        port: 3000,
        is_empty: null,
      }));
      expect(status.type).toBe('loading');
      expect(state.isServerReady()).toBe(false);
    });

    it('does NOT transition early when port is null', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 5,
        completed: false,
        port: null,
        is_empty: false,
      }));
      expect(status.type).toBe('loading');
      expect(state.isServerReady()).toBe(false);
    });

    it('does NOT transition early when is_empty is true', () => {
      const status = state.handleProgressUpdate(createProgressUpdate({
        percentage: 5,
        completed: false,
        port: 3000,
        is_empty: true,
      }));
      expect(status.type).toBe('loading');
      expect(state.isServerReady()).toBe(false);
    });
  });

  describe('Edge Cases', () => {
    it('handles rapid progress updates', () => {
      // Simulate rapid updates during initial build
      for (let i = 0; i <= 100; i += 10) {
        state.handleProgressUpdate(createProgressUpdate({
          percentage: i,
          completed: i === 100,
          port: i === 100 ? 3000 : null,
          is_empty: i === 100 ? false : null,
        }));
      }

      expect(state.getStatus()).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });

    it('handles multiple rapid rebuilds', () => {
      // Initial build complete
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      // Multiple rebuilds - all should be silent except completion
      for (let rebuild = 0; rebuild < 3; rebuild++) {
        state.handleProgressUpdate(createProgressUpdate({ percentage: 20 }));
        state.handleProgressUpdate(createProgressUpdate({ percentage: 50 }));
        state.handleProgressUpdate(createProgressUpdate({
          percentage: 100,
          completed: true,
          port: 3000,
          is_empty: false,
        }));
      }

      expect(state.getStatus()).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });

    it('preserves server URL across rebuilds', () => {
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      }));

      // Rebuild with different port in update (shouldn't change stored URL)
      state.handleProgressUpdate(createProgressUpdate({
        percentage: 100,
        completed: true,
        port: 4000, // Different port
        is_empty: false,
      }));

      // Original URL is preserved
      expect(state.getServerUrl()).toBe('http://localhost:3000');
      expect(state.getStatus()).toEqual({ type: 'ready', url: 'http://localhost:3000' });
    });
  });
});

describe('PreviewState stage field', () => {
  it('starts with stage = "initial"', () => {
    const s = new PreviewState();
    expect(s.getStage()).toBe('initial');
  });

  it('setStage("opening") transitions from initial', () => {
    const s = new PreviewState();
    s.setStage('opening');
    expect(s.getStage()).toBe('opening');
  });

  it('setStage advances through the pipeline', () => {
    const s = new PreviewState();
    s.setStage('opening');
    s.setStage('scanning');
    s.setStage('materializing');
    s.setStage('compiling');
    expect(s.getStage()).toBe('compiling');
  });

  it('reset() clears stage back to initial', () => {
    const s = new PreviewState();
    s.setStage('compiling');
    s.reset();
    expect(s.getStage()).toBe('initial');
  });
});

describe('a preview reached at another address', () => {
  it('loads from where the host says, both on completion and while a rebuild of a served site runs', () => {
    host.previewUrl = () => 'https://edit.example.test:444';
    try {
      const done = new PreviewState();
      expect(done.handleProgressUpdate(createProgressUpdate({ completed: true, percentage: 100, port: 444, is_empty: false })))
        .toEqual({ type: 'ready', url: 'https://edit.example.test:444' });
      const early = new PreviewState();
      expect(early.handleProgressUpdate(createProgressUpdate({ port: 444, is_empty: false })))
        .toEqual({ type: 'ready', url: 'https://edit.example.test:444' });
    } finally {
      resetTestHost();
    }
  });
});
