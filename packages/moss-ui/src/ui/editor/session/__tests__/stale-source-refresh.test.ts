// packages/moss-ui/src/ui/editor/session/__tests__/stale-source-refresh.test.ts
import { describe, it, expect, vi } from 'vitest';
import { StaleSourceRefresh } from '../stale-source-refresh';
import type { EditorNodeWire, PendingReason } from '../node-class';

type Answer = { pending: PendingReason | null } | null;

/** A page re-ask that hands out a scripted sequence, repeating the last answer. */
function scripted(...answers: Answer[]) {
  let i = 0;
  return vi.fn(async (_urlPath: string): Promise<Answer> => answers[Math.min(i++, answers.length - 1)]);
}

const ARRIVING: Answer = { pending: 'download' };
const ANSWERED: Answer = { pending: null };

function harness(
  refreshPage: ReturnType<typeof scripted>,
  over: Partial<{ liveUrlPath: () => string; editorIsActive: () => boolean }> = {},
) {
  const applied: EditorNodeWire[] = [];
  const refresher = new StaleSourceRefresh({
    refreshPage,
    liveUrlPath: over.liveUrlPath ?? (() => '/'),
    editorIsActive: over.editorIsActive ?? (() => true),
    applyNode: (node) => { applied.push(node); },
  });
  return { refresher, applied };
}

// The page's own lookup delivers its answer to the editor; this module only
// decides when to ask again and when to stop.
describe('StaleSourceRefresh — the arrival retry for a page', () => {
  it('asks the page again when bytes land, and pushes nothing itself', async () => {
    const ask = scripted(ANSWERED);
    const { refresher, applied } = harness(ask);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();

    expect(ask).toHaveBeenCalledWith('/');
    expect(applied).toEqual([]);
  });

  it('does nothing at all until something actually arrives — no poll, no timer', async () => {
    const ask = scripted(ANSWERED);
    const { refresher } = harness(ask);

    refresher.armForArrival('/');
    await vi.waitFor(() => expect(true).toBe(true));

    expect(ask).not.toHaveBeenCalled();
  });

  it('stays armed while the answer is still "arriving", then stops once answered', async () => {
    const ask = scripted(ARRIVING, ARRIVING, ANSWERED);
    const { refresher } = harness(ask);

    refresher.armForArrival('/');
    for (let i = 0; i < 5; i++) await refresher.onFilesArrived();

    expect(ask).toHaveBeenCalledTimes(3);
  });

  it('an arrival with nothing armed is ignored — the retry is not a standing subscription', async () => {
    const ask = scripted(ANSWERED);
    const { refresher } = harness(ask);

    await refresher.onFilesArrived();

    expect(ask).not.toHaveBeenCalled();
  });

  it('never asks about a page the user has navigated away from, and stops', async () => {
    const ask = scripted(ANSWERED);
    let live = '/somewhere-else/';
    const { refresher } = harness(ask, { liveUrlPath: () => live });

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    live = '/';
    await refresher.onFilesArrived();

    expect(ask).not.toHaveBeenCalled();
  });

  it('asks nothing for an editor the user has closed', async () => {
    const ask = scripted(ANSWERED);
    const { refresher } = harness(ask, { editorIsActive: () => false });

    refresher.armForArrival('/');
    await refresher.onFilesArrived();

    expect(ask).not.toHaveBeenCalled();
  });

  it('stops when the page is no longer the one being looked up', async () => {
    const ask = scripted(null, ANSWERED);
    const { refresher } = harness(ask);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();

    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('coalesces a burst of arrivals into one ask instead of stacking them', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => { release = r; });
    const ask = vi.fn(async (): Promise<Answer> => { await gate; return ANSWERED; });
    const { refresher } = harness(ask as never);

    refresher.armForArrival('/');
    const first = refresher.onFilesArrived();
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    expect(ask).toHaveBeenCalledTimes(1);

    release();
    await first;
  });

  it('a failed lookup leaves the pane armed rather than declaring the file absent', async () => {
    const ask = scripted({ pending: 'lookup' }, ANSWERED);
    const { refresher } = harness(ask);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();

    expect(ask).toHaveBeenCalledTimes(2);
  });
});

describe('StaleSourceRefresh — a pane the editor opened by PATH', () => {
  // The preview URL is the right identity for a pane that CAME from resolving
  // that URL, and the wrong one for a pane the user reached by clicking a row
  // in the file tree: the two name different documents, and the tree click is
  // the commonest way to meet a still-downloading file.
  const DRAFT = '/vault/notes/draft.md';

  it('re-offers the file it is actually waiting for, without asking about the preview page', async () => {
    const resolve = scripted(ANSWERED);
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/home/' });

    refresher.armForArrival('/home/', DRAFT);
    await refresher.onFilesArrived();

    expect(applied).toEqual([
      { sourcePath: DRAFT, isRoot: false, takeover: null },
    ]);
    // The preview's page has no bearing on this pane, so it is never consulted
    // — which is also what stops a torn article-map from answering "generated"
    // for a file that is merely downloading.
    expect(resolve).not.toHaveBeenCalled();
  });

  it('offers again once the editor has answered the last one, and not before', async () => {
    // The answer IS the next announcement: a re-open that lands back on the
    // pending pane re-announces (re-arming), one that finds the bytes settles.
    // Until one of those happens the offer is still in flight, and an offer is
    // a full open — a 15s materialize wait against a 2s arrival tick, so an
    // ungated burst stacks seven blocking reads on one file.
    const resolve = scripted(ANSWERED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/', DRAFT);
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    expect(applied).toHaveLength(1);

    refresher.armForArrival('/', DRAFT);   // the pane re-announced: still waiting
    await refresher.onFilesArrived();
    expect(applied).toHaveLength(2);

    refresher.disarm();
    await refresher.onFilesArrived();
    expect(applied).toHaveLength(2);
  });

  it('stops offering a file whose open answers nothing at all', async () => {
    // A file deleted or renamed while it was downloading fails its open on a
    // path that mounts no surface, so the editor announces neither pending nor
    // settled. Without a terminating condition every arrival edge would draw
    // another "could not open" toast for the rest of the sync episode.
    const resolve = scripted(ANSWERED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/', DRAFT);
    for (let i = 0; i < 5; i++) await refresher.onFilesArrived();

    expect(applied).toHaveLength(1);
  });

  it('pushes nothing into an editor the user has closed, and gives up on it', async () => {
    const resolve = scripted(ANSWERED);
    let active = false;
    const { refresher, applied } = harness(resolve, { editorIsActive: () => active });

    refresher.armForArrival('/', DRAFT);
    await refresher.onFilesArrived();
    expect(applied).toEqual([]);

    // The pane is gone; re-opening the panel is not this module's cue to resume.
    active = true;
    await refresher.onFilesArrived();
    expect(applied).toEqual([]);
  });
});
