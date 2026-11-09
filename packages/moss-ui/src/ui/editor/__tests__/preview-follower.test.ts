import { describe, test, expect, vi, beforeEach } from 'vitest';
import { EntryRegistry } from '../entry-registry';
import { PreviewFollower } from '../preview-follower';

const PROJECT_ROOT = '/tmp/proj';

let mockResolveUrlForFile: ReturnType<typeof vi.fn>;
let mockEmit: ReturnType<typeof vi.fn>;
let mockOnMossEvent: ReturnType<typeof vi.fn>;
let fileChangedCallbacks: Array<(msg: { kind: 'FileChanged' }) => void>;

beforeEach(() => {
  fileChangedCallbacks = [];
  mockResolveUrlForFile = vi.fn();
  mockEmit = vi.fn().mockResolvedValue(undefined);
  mockOnMossEvent = vi.fn().mockImplementation(
    async (_kind: string, cb: (msg: { kind: 'FileChanged' }) => void) => {
      fileChangedCallbacks.push(cb);
      return () => {
        const idx = fileChangedCallbacks.indexOf(cb);
        if (idx >= 0) fileChangedCallbacks.splice(idx, 1);
      };
    },
  );
});

function fireFileChanged(): void {
  for (const cb of [...fileChangedCallbacks]) cb({ kind: 'FileChanged' });
}

function mkFollower(
  registry: EntryRegistry,
  describeSourceRole?: ReturnType<typeof vi.fn>,
): PreviewFollower {
  return new PreviewFollower({
    registry,
    projectRoot: PROJECT_ROOT,
    resolveUrlForFile: mockResolveUrlForFile,
    emit: mockEmit,
    onMossEvent: mockOnMossEvent,
    describeSourceRole,
  });
}

describe('PreviewFollower', () => {
  test('a new page uses its canonical route when already built, skipping the expected route', async () => {
    const follower = mkFollower(new EntryRegistry());
    const expected = vi.fn().mockResolvedValue({ status: 'ok', data: '/hellow-internet/' });
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/' });
    await follower.navigate('Hellow Internet.md', expected);
    expect(expected).not.toHaveBeenCalled();
    expect(mockEmit).toHaveBeenCalledExactlyOnceWith('editor:navigate-preview', { urlPath: '/' });
  });

  test('canonical build resolution wins over a slower expected route', async () => {
    const follower = mkFollower(new EntryRegistry());
    let finishExpected!: (answer: { status: 'ok'; data: string }) => void;
    const expected = vi.fn().mockReturnValue(new Promise((resolve) => { finishExpected = resolve; }));
    mockResolveUrlForFile.mockResolvedValueOnce({ status: 'ok', data: null })
      .mockResolvedValue({ status: 'ok', data: '/' });
    const navigation = follower.navigate('Hellow Internet.md', expected);
    await vi.waitFor(() => expect(expected).toHaveBeenCalled());
    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledTimes(1));
    finishExpected({ status: 'ok', data: '/hellow-internet/' });
    await navigation;
    expect(mockEmit).toHaveBeenCalledExactlyOnceWith('editor:navigate-preview', { urlPath: '/' });
  });

  test('a new page emits a covered expected route and then the canonical build route', async () => {
    const follower = mkFollower(new EntryRegistry());
    const expected = vi.fn().mockResolvedValue({ status: 'ok', data: '/hellow-internet/' });
    mockResolveUrlForFile.mockResolvedValueOnce({ status: 'ok', data: null })
      .mockResolvedValue({ status: 'ok', data: '/' });
    await follower.navigate('Hellow Internet.md', expected);
    expect(mockEmit).toHaveBeenNthCalledWith(1, 'editor:navigate-preview', {
      urlPath: '/hellow-internet/', awaitingBuild: true,
    });
    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledTimes(2));
    expect(mockEmit).toHaveBeenLastCalledWith('editor:navigate-preview', { urlPath: '/' });
  });

  test('a superseded expected route cannot navigate away from the newer active document', async () => {
    const follower = mkFollower(new EntryRegistry());
    let finishExpected!: (answer: { status: 'ok'; data: string }) => void;
    const expected = vi.fn().mockReturnValue(new Promise((resolve) => { finishExpected = resolve; }));
    mockResolveUrlForFile.mockResolvedValueOnce({ status: 'ok', data: null })
      .mockResolvedValue({ status: 'ok', data: '/second/' });
    const first = follower.navigate('first.md', expected);
    await vi.waitFor(() => expect(expected).toHaveBeenCalled());
    await follower.navigate('second.md');
    finishExpected({ status: 'ok', data: '/first/' });
    await first;
    expect(mockEmit).toHaveBeenCalledExactlyOnceWith('editor:navigate-preview', { urlPath: '/second/' });
  });

  test('emits editor:navigate-preview when active path changes and URL resolves', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/hello/' });

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(id);
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
    expect(mockResolveUrlForFile).toHaveBeenCalledWith('/tmp/proj/posts/hello.md');
  });

  test('start() emits for an already-active entry (initial sync)', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    registry.setActive(id);  // setActive BEFORE start()
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/hello/' });

    const follower = mkFollower(registry);
    follower.start();

    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
  });

  test('does NOT fallback to "/" when URL resolves to null; waits on FileChanged', async () => {
    // The wait is on FileChanged, never BuildComplete: BuildComplete
    // (crates/moss-build/src/build/media/video.rs) only fires when the video pipeline
    // runs, so a plain markdown rebuild would never wake the retry and the
    // preview stayed stuck on the old 404 URL forever.
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(id);
    await vi.waitFor(() => expect(mockOnMossEvent).toHaveBeenCalled());
    expect(mockEmit).not.toHaveBeenCalled();
    expect(mockOnMossEvent).toHaveBeenCalledWith('FileChanged', expect.any(Function));
    expect(mockOnMossEvent.mock.calls.map((c) => c[0])).not.toContain('BuildComplete');
  });

  test('logs the file and a reason when resolution comes back empty', async () => {
    // The only signal an author (or a later debugger) has that navigation is
    // stalled — no toast, no error, the preview just doesn't move. Must name
    // which file failed to resolve and why, not just that something did.
    const debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {});
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    const follower = mkFollower(registry);
    follower.start();
    registry.setActive(id);
    await vi.waitFor(() => expect(debugSpy).toHaveBeenCalled());

    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('posts/hello.md'));
    expect(debugSpy).toHaveBeenCalledWith(expect.stringContaining('reason'));
    debugSpy.mockRestore();
  });

  test('keeps retrying on each FileChanged until URL resolves', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    mockResolveUrlForFile
      .mockResolvedValueOnce({ status: 'ok', data: null })   // initial
      .mockResolvedValueOnce({ status: 'ok', data: null })   // 1st build (unrelated, e.g. video)
      .mockResolvedValueOnce({ status: 'ok', data: '/posts/hello/' }); // 2nd build

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(id);
    await vi.waitFor(() => expect(mockOnMossEvent).toHaveBeenCalled());

    fireFileChanged();
    await vi.waitFor(() => expect(mockResolveUrlForFile).toHaveBeenCalledTimes(2));
    expect(mockEmit).not.toHaveBeenCalled();

    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
    expect(mockEmit).toHaveBeenCalledTimes(1);
  });

  test('rename of active entry triggers re-resolution', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('old.md', false);
    registry.setActive(id);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/old/' });

    const follower = mkFollower(registry);
    follower.start();
    mockEmit.mockClear();
    mockResolveUrlForFile.mockClear();

    mockResolveUrlForFile
      .mockResolvedValueOnce({ status: 'ok', data: null })       // initial
      .mockResolvedValueOnce({ status: 'ok', data: '/new/' });   // after build

    registry.rename('old.md', 'new.md');
    await vi.waitFor(() => expect(mockOnMossEvent).toHaveBeenCalled());
    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());

    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/new/' });
  });

  test('does not emit on transition to null active', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    registry.setActive(id);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/a/' });

    const follower = mkFollower(registry);
    follower.start();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    mockEmit.mockClear();

    registry.setActive(null);
    await Promise.resolve();
    await Promise.resolve();
    expect(mockEmit).not.toHaveBeenCalled();
  });

  test('superseding path change unsubscribes the pending FileChanged listener', async () => {
    const registry = new EntryRegistry();
    const idA = registry.upsert('a.md', false);
    const idB = registry.upsert('b.md', false);
    mockResolveUrlForFile
      .mockResolvedValueOnce({ status: 'ok', data: null })       // A: needs build
      .mockResolvedValueOnce({ status: 'ok', data: '/b/' });     // B: resolves immediately

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(idA);
    await vi.waitFor(() => expect(mockOnMossEvent).toHaveBeenCalled());
    expect(fileChangedCallbacks).toHaveLength(1);

    registry.setActive(idB);
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());

    // A's FileChanged listener was unsubbed when B superseded.
    expect(fileChangedCallbacks).toHaveLength(0);
  });

  // What the generation token guards against: A's own resolveUrlForFile
  // call — not a FileChanged retry — is still in flight when B supersedes it. A's
  // late completion must never emit, even though it resolves successfully. Uses
  // asset paths (permanent misses) so no FileChanged subscription is in play —
  // the generation check is the ONLY guard standing between this stale result
  // and an emit.
  test('a slow initial resolve for a superseded asset path never emits, even once it returns', async () => {
    const registry = new EntryRegistry();
    const idA = registry.upsert('a.png', false);
    const idB = registry.upsert('b.png', false);

    let releaseA!: (v: { status: 'ok'; data: string | null }) => void;
    const slowA = new Promise<{ status: 'ok'; data: string | null }>((resolve) => { releaseA = resolve; });
    mockResolveUrlForFile.mockImplementation((path: string) =>
      path.endsWith('a.png') ? slowA : Promise.resolve({ status: 'ok', data: '/b.png' }),
    );

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(idA);
    await vi.waitFor(() =>
      expect(mockResolveUrlForFile).toHaveBeenCalledWith(expect.stringContaining('a.png')),
    );

    registry.setActive(idB);
    await vi.waitFor(() =>
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/b.png' }),
    );
    mockEmit.mockClear();

    // A's resolve finally comes back, long after B took over.
    releaseA({ status: 'ok', data: '/a.png' });
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();

    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/a.png' });
  });

  test('public navigate(path) resolves and emits without going through registry', async () => {
    const registry = new EntryRegistry();
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/hello/' });

    const follower = mkFollower(registry);
    follower.start();

    await follower.navigate('posts/hello.md');
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
  });

  test('navigate() with unresolvable URL does NOT emit "/" fallback', async () => {
    const registry = new EntryRegistry();
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    const follower = mkFollower(registry);
    follower.start();

    await follower.navigate('assets/cover.jpg');
    await Promise.resolve();
    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/' });
  });

  test('a navigation that blows up is contained, never left as an unhandled rejection', async () => {
    // Every entry point here is started and walked away from — the registry
    // subscriber and editor-main both call these with `void`. So there is no
    // caller left to catch anything: an unexpected throw becomes an unhandled
    // rejection, which is invisible in the app and, in tests, an error charged
    // to whichever unrelated test happened to be running when it settled. That
    // is exactly how it surfaced — as two rejections attributed to two tests
    // that never touch the preview. Following the preview is best-effort, so a
    // step that cannot finish must end the same way the expected dead ends do:
    // a warning, and the preview left where it is.
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/hello.md', false);
    // An answer that breaks the Result contract stands in for "anything at all
    // went wrong in there" — the containment must not depend on the shape.
    mockResolveUrlForFile.mockResolvedValue(undefined);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const follower = mkFollower(registry);
    follower.start();

    // The fire-and-forget path: nothing here can observe a rejection.
    registry.setActive(id);
    await vi.waitFor(() => expect(warn).toHaveBeenCalled());

    // The awaited path resolves rather than rejecting.
    await expect(follower.navigate('posts/hello.md')).resolves.toBeUndefined();
    expect(mockEmit).not.toHaveBeenCalled();

    warn.mockRestore();
  });

  test('a FileChanged that fires while the INITIAL resolve is still in flight is not lost (new-file creation race)', async () => {
    // Regression for the "preview doesn't navigate to a brand-new page" bug:
    // create_files writes the file, then the file-tree dispatches
    // editor:file-selected → loadFile (several awaited RPCs) → navigate(),
    // whose FIRST resolveUrlForFile call is what used to gate subscribing to
    // FileChanged. The file watcher's rebuild (triggered by the file's
    // creation, independent of the frontend) can complete and emit
    // FileChanged WHILE that first resolve call is still in flight — before
    // a register-after-check subscription exists to hear it. If that one
    // FileChanged is the only rebuild the user's action produces (they
    // haven't typed anything yet), the preview is stuck on the old page
    // forever: nothing else will ever fire FileChanged again.
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/new-page.md', false);

    let resolveFirst!: (v: { status: 'ok'; data: string | null }) => void;
    const firstCall = new Promise<{ status: 'ok'; data: string | null }>((res) => {
      resolveFirst = res;
    });
    mockResolveUrlForFile
      .mockReturnValueOnce(firstCall) // initial attempt — stays pending until we settle it
      .mockResolvedValue({ status: 'ok', data: '/posts/new-page/' }); // any retry succeeds

    const follower = mkFollower(registry);
    follower.start();
    registry.setActive(id);

    // Let the initial resolveUrlForFile call actually get issued.
    await vi.waitFor(() => expect(mockResolveUrlForFile).toHaveBeenCalledTimes(1));

    // The ONE rebuild this file's creation triggers completes now, while the
    // initial resolve call above is still unsettled.
    fireFileChanged();

    // Only now does the initial attempt learn the file wasn't built YET (at
    // the moment the resolve call was issued) — a completely normal ordering
    // since the resolve was issued before the rebuild finished.
    resolveFirst({ status: 'ok', data: null });

    // No further FileChanged ever fires (the user hasn't edited anything
    // else). The preview must still have navigated from the one FileChanged
    // that arrived during the initial resolve's flight.
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/new-page/' });
  });

  test('does not double-emit when both the concurrent retry AND the original in-flight resolve succeed', async () => {
    // Subscribing before the resolve attempt (the fix above) closes the
    // lost-event gap, but opens a DIFFERENT one if left unguarded: the
    // FileChanged-triggered retry can resolve+emit BEFORE the original,
    // still-in-flight resolveUrlForFile call comes back — and that original
    // call may itself later resolve with the (by-then-built) URL too. Both
    // must not emit; only the winner does.
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/new-page.md', false);

    let resolveFirst!: (v: { status: 'ok'; data: string | null }) => void;
    const firstCall = new Promise<{ status: 'ok'; data: string | null }>((res) => {
      resolveFirst = res;
    });
    mockResolveUrlForFile
      .mockReturnValueOnce(firstCall) // initial attempt — resolves LATE
      .mockResolvedValue({ status: 'ok', data: '/posts/new-page/' }); // retry succeeds immediately

    const follower = mkFollower(registry);
    follower.start();
    registry.setActive(id);

    await vi.waitFor(() => expect(mockResolveUrlForFile).toHaveBeenCalledTimes(1));

    // Rebuild completes and its FileChanged fires while the initial call is
    // still pending. The retry it triggers resolves and emits FIRST.
    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledTimes(1));

    // The ORIGINAL call — issued before the rebuild finished — now also
    // comes back with a URL (plausible: it was simply slower to be
    // serviced). It must NOT emit a second time for the same navigation.
    resolveFirst({ status: 'ok', data: '/posts/new-page/' });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockEmit).toHaveBeenCalledTimes(1);
  });

  test('a CJK/full-width-punctuation draft path is passed through resolveUrlForFile unmangled and still retries+navigates', async () => {
    // Bug report: opening a `draft: true` file at 文字/信息网络/DWeb
    // Camp，下次见.md never moved the preview. The frontend emit path (this
    // file) has no notion of `draft` at all — that gate lives entirely in
    // the Rust resolver — so the only things this layer could get wrong are
    // (a) mangling the CJK/full-width-punctuation path before handing it to
    // resolveUrlForFile, or (b) bailing instead of arming the FileChanged
    // retry when the first resolve comes back empty. Neither happens: the
    // path travels byte-identical, and the existing retry-until-resolved
    // behavior (see 'keeps retrying on each FileChanged until URL resolves'
    // above) is exercised with this exact filename.
    const relPath = '文字/信息网络/DWeb Camp，下次见.md';
    const registry = new EntryRegistry();
    const id = registry.upsert(relPath, false);
    mockResolveUrlForFile
      .mockResolvedValueOnce({ status: 'ok', data: null }) // pre-seal: not resolvable yet
      .mockResolvedValueOnce({
        status: 'ok',
        data: '/文字/信息网络/DWeb Camp，下次见/',
      });

    const follower = mkFollower(registry);
    follower.start();

    registry.setActive(id);
    await vi.waitFor(() => expect(mockOnMossEvent).toHaveBeenCalled());
    expect(mockResolveUrlForFile).toHaveBeenCalledWith(`${PROJECT_ROOT}/${relPath}`);

    fireFileChanged();
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', {
      urlPath: '/文字/信息网络/DWeb Camp，下次见/',
    });
  });

  test('stop() unsubscribes from the registry and pending FileChanged', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('a.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    const follower = mkFollower(registry);
    follower.start();
    registry.setActive(id);
    await vi.waitFor(() => expect(fileChangedCallbacks).toHaveLength(1));

    follower.stop();
    expect(fileChangedCallbacks).toHaveLength(0);

    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/a/' });
    const idB = registry.upsert('b.md', false);
    registry.setActive(idB);
    await Promise.resolve();
    await Promise.resolve();
    expect(mockEmit).not.toHaveBeenCalled();
  });
  // ── Slot files (`footer.md`) ────────────────────────────────────────────
  //
  // A slot file is markdown and is not under `.moss/`, so the pure
  // isStructurallyPermanentMiss predicate calls it transient — but it emits no
  // page and never will. Without the describeSourceRole probe the follower
  // armed a FileChanged retry loop that could never resolve.

  test('slot file does not arm the FileChanged retry loop', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('footer.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });
    const describeSourceRole = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { slot: 'footer-left', language: null, can_be_page: true },
    });

    const follower = mkFollower(registry, describeSourceRole);
    follower.start();
    registry.setActive(id);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockOnMossEvent).not.toHaveBeenCalled();
    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', expect.anything());

    // Even if a rebuild lands, nothing is subscribed, so nothing navigates.
    fireFileChanged();
    await Promise.resolve();
    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', expect.anything());
  });

  test('slot file asks the shell to reveal the footer instead', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('zh-hans/footer.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });
    const describeSourceRole = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { slot: 'footer-left', language: 'zh-hans', can_be_page: true },
    });

    const follower = mkFollower(registry, describeSourceRole);
    follower.start();
    registry.setActive(id);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // The language travels with the request: the shell needs it to tell a page
    // that renders THIS footer from one that renders the other tree's.
    expect(mockEmit).toHaveBeenCalledWith('editor:reveal-slot', { language: 'zh-hans' });
  });

  test('an ordinary page never asks for a footer reveal', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/new.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/new/' });
    const describeSourceRole = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { slot: null, language: null, can_be_page: true },
    });

    const follower = mkFollower(registry, describeSourceRole);
    follower.start();
    registry.setActive(id);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockEmit).not.toHaveBeenCalledWith('editor:reveal-slot', expect.anything());
  });

  test('an ordinary unbuilt page still arms the retry loop', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/new.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });
    const describeSourceRole = vi.fn().mockResolvedValue({
      status: 'ok',
      data: { slot: null, language: null, can_be_page: true },
    });

    const follower = mkFollower(registry, describeSourceRole);
    follower.start();
    registry.setActive(id);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockOnMossEvent).toHaveBeenCalled();
  });

  test('a failing slot probe falls back to the retry loop, never suppressing navigation', async () => {
    const registry = new EntryRegistry();
    const id = registry.upsert('posts/new.md', false);
    mockResolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/new/' });
    const describeSourceRole = vi.fn().mockRejectedValue(new Error('ipc down'));

    const follower = mkFollower(registry, describeSourceRole);
    follower.start();
    registry.setActive(id);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', {
      urlPath: '/posts/new/',
    });
  });
});
