// packages/moss-ui/src/ui/editor/session/__tests__/stale-source-refresh.test.ts
import { describe, it, expect, vi } from 'vitest';
import { folderHomeTakeover } from '../node-class';
import { StaleSourceRefresh } from '../stale-source-refresh';
import type { EditSourceResolution, ResolvedNode } from '../node-class';

type Answer = EditSourceResolution;

/** A resolve stub that hands out a scripted sequence, repeating the last answer. */
function scripted(...answers: Answer[]) {
  let i = 0;
  const fn = vi.fn(async (_urlPath: string) => {
    const data = answers[Math.min(i, answers.length - 1)];
    i += 1;
    return { status: 'ok' as const, data };
  });
  return fn;
}

const ARRIVING: Answer = { source_path: null, takeover: null, source_pending: true };
const LANDED: Answer = { source_path: '/vault/index.md', takeover: null, source_pending: false };
const GENERATED: Answer = { source_path: null, takeover: null, source_pending: false };

function harness(
  resolvePageSource: ReturnType<typeof scripted>,
  over: Partial<{ liveUrlPath: () => string; editorIsActive: () => boolean }> = {},
) {
  const applied: ResolvedNode[] = [];
  const refresher = new StaleSourceRefresh({
    resolvePageSource,
    liveUrlPath: over.liveUrlPath ?? (() => '/'),
    editorIsActive: over.editorIsActive ?? (() => true),
    applyNode: (node) => { applied.push(node); },
    warn: () => {},
  });
  return { refresher, applied };
}

describe('StaleSourceRefresh — the arrival retry', () => {
  it('turns the pending pane into the real editor when the bytes land, with no user action', async () => {
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();

    expect(applied).toEqual([
      { sourcePath: '/vault/index.md', isRoot: true, takeover: null },
    ]);
  });

  it('does nothing at all until something actually arrives — no poll, no timer', async () => {
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/');
    await vi.waitFor(() => expect(true).toBe(true));

    expect(resolve).not.toHaveBeenCalled();
    expect(applied).toEqual([]);
  });

  it('stays armed while the answer is still "arriving", and pushes nothing over the pane', async () => {
    const resolve = scripted(ARRIVING, ARRIVING, LANDED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    expect(applied).toEqual([]);

    await refresher.onFilesArrived();
    expect(applied).toEqual([
      { sourcePath: '/vault/index.md', isRoot: true, takeover: null },
    ]);
  });

  it('disarms once healed, so a later arrival does not re-resolve for a pane that is gone', async () => {
    const resolve = scripted(LANDED);
    const { refresher } = harness(resolve);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    expect(resolve).toHaveBeenCalledTimes(1);

    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    expect(resolve).toHaveBeenCalledTimes(1);
  });

  it('an arrival with nothing armed is ignored — the retry is not a standing subscription', async () => {
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve);

    await refresher.onFilesArrived();

    expect(resolve).not.toHaveBeenCalled();
    expect(applied).toEqual([]);
  });

  it('a genuinely generated page replaces the pending pane with the GENERATED one, not another wait', async () => {
    const resolve = scripted(GENERATED);
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/tags/poetry/' });

    refresher.armForArrival('/tags/poetry/');
    await refresher.onFilesArrived();

    // (null, null, non-root) is exactly what decideEditorSurface reads as
    // `generated` — the opposite case, and it must still be reachable.
    expect(applied).toEqual([
      { sourcePath: null, isRoot: false, takeover: null },
    ]);
    // …and the retry stops: nothing about this page is going to change.
    await refresher.onFilesArrived();
    expect(resolve).toHaveBeenCalledTimes(1);
  });

  it('never clobbers a page the user has navigated to since', async () => {
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/somewhere-else/' });

    refresher.armForArrival('/');
    await refresher.onFilesArrived();

    expect(applied).toEqual([]);
  });

  it('pushes nothing into an editor the user has closed', async () => {
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve, { editorIsActive: () => false });

    refresher.armForArrival('/');
    await refresher.onFilesArrived();

    expect(applied).toEqual([]);
  });

  it('coalesces a burst of arrivals into one resolve instead of stacking them', async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => { release = r; });
    const resolve = vi.fn(async () => {
      await gate;
      return { status: 'ok' as const, data: LANDED };
    });
    const { refresher, applied } = harness(resolve as never);

    refresher.armForArrival('/');
    const first = refresher.onFilesArrived();
    await refresher.onFilesArrived();
    await refresher.onFilesArrived();
    expect(resolve).toHaveBeenCalledTimes(1);

    release();
    await first;
    expect(applied).toHaveLength(1);
  });

  it('a resolve that throws leaves the pane armed rather than declaring the file absent', async () => {
    const resolve = vi.fn()
      .mockRejectedValueOnce(new Error('backend busy'))
      .mockResolvedValueOnce({ status: 'ok' as const, data: LANDED });
    const { refresher, applied } = harness(resolve as never);

    refresher.armForArrival('/');
    await refresher.onFilesArrived();
    expect(applied).toEqual([]);

    await refresher.onFilesArrived();
    expect(applied).toEqual([
      { sourcePath: '/vault/index.md', isRoot: true, takeover: null },
    ]);
  });
});

describe('StaleSourceRefresh — the stale-cache self-heal it already performed', () => {
  it('corrects an ambiguous bake to the folder identity the fresh resolve found', async () => {
    const resolve = scripted({ source_path: null, takeover: folderHomeTakeover('/vault/notes', 'notes') });
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/notes/' });

    await refresher.refreshStale('/notes/');

    expect(applied).toEqual([
      { sourcePath: null, isRoot: false, takeover: folderHomeTakeover('/vault/notes', 'notes') },
    ]);
  });

  it('leaves a confirmed-generated page alone — it must not downgrade a live takeover', async () => {
    const resolve = scripted(GENERATED);
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/tags/poetry/' });

    await refresher.refreshStale('/tags/poetry/');

    expect(applied).toEqual([]);
  });
});

describe('StaleSourceRefresh — a pane the editor opened by PATH', () => {
  // The preview URL is the right identity for a pane that CAME from resolving
  // that URL, and the wrong one for a pane the user reached by clicking a row
  // in the file tree: the two name different documents, and the tree click is
  // the commonest way to meet a still-downloading file.
  const DRAFT = '/vault/notes/draft.md';

  it('re-offers the file it is actually waiting for, without asking about the preview page', async () => {
    const resolve = scripted(LANDED);
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

  it('never lets the preview page\'s verdict land on the file the pane names', async () => {
    // The opposite case, aimed at the wrong document: '/home/' really is
    // generated, and pushing that answer would swap the pending draft for the
    // buttonless "nothing to edit here" pane.
    const resolve = scripted(GENERATED);
    const { refresher, applied } = harness(resolve, { liveUrlPath: () => '/home/' });

    refresher.armForArrival('/home/', DRAFT);
    await refresher.onFilesArrived();

    expect(applied).toEqual([
      { sourcePath: DRAFT, isRoot: false, takeover: null },
    ]);
  });

  it('offers again once the editor has answered the last one, and not before', async () => {
    // The answer IS the next announcement: a re-open that lands back on the
    // pending pane re-announces (re-arming), one that finds the bytes settles.
    // Until one of those happens the offer is still in flight, and an offer is
    // a full open — a 15s materialize wait against a 2s arrival tick, so an
    // ungated burst stacks seven blocking reads on one file.
    const resolve = scripted(LANDED);
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
    const resolve = scripted(LANDED);
    const { refresher, applied } = harness(resolve);

    refresher.armForArrival('/', DRAFT);
    for (let i = 0; i < 5; i++) await refresher.onFilesArrived();

    expect(applied).toHaveLength(1);
  });

  it('pushes nothing into an editor the user has closed, and gives up on it', async () => {
    const resolve = scripted(LANDED);
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

it('keeps the folder arrival retry armed until directory emptiness is known', async () => {
  const folder = folderHomeTakeover('/vault', 'vault', true);
  const resolve = scripted(
    { source_path: null, takeover: { ...folder, folder_empty: null } },
    { source_path: null, takeover: { ...folder, folder_empty: true } },
  );
  const { refresher, applied } = harness(resolve);
  refresher.armForArrival('/');
  await refresher.onFilesArrived();
  expect(applied).toEqual([]);
  await refresher.onFilesArrived();
  expect(applied).toEqual([{ sourcePath: null, isRoot: true, takeover: { ...folder, folder_empty: true } }]);
});
