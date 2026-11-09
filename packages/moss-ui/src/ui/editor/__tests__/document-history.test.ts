import { describe, test, expect, beforeEach, afterEach } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState, Transaction } from '@codemirror/state';
import { history, undo as cmUndo, redo as cmRedo, undoDepth, redoDepth } from '@codemirror/commands';
import { DocumentHistory, deepEqual, type DocumentHistoryHooks } from '../document-history';

// CM6's real history extension drives every cm6 assertion here. Mocking
// undoDepth would test the arithmetic and miss the thing that is actually hard:
// CM6 coalesces, prunes and caps on its own schedule, and the module's whole job
// is to stay reconciled with it.

interface Harness {
  hist: DocumentHistory;
  view: EditorView;
  fields: Map<string, unknown>;
  surfaced: { key: string; removed: boolean; restored: boolean }[];
  /** Re-entrancy probe: runs inside `surface()`, which the chip bar can do for
   *  real (promoting a chip closes and reopens its popover, and that popover's
   *  boundary capture calls straight back into recordField). */
  onSurface: (fn: (() => void) | null) => void;
  generation: () => number;
  setGeneration: (g: number) => void;
  /** Replace the editor the way a file switch does: destroy, build a fresh one. */
  swapView: () => void;
  destroy: () => void;
}

function harness(doc = ''): Harness {
  const fields = new Map<string, unknown>();
  const surfaced: { key: string; removed: boolean; restored: boolean }[] = [];
  let surfaceHook: (() => void) | null = null;
  let gen = 1;
  // Wired through the REAL updateListener, exactly as production does. That
  // matters beyond fidelity: the listener fires synchronously inside
  // `view.dispatch`, which IS the re-entrancy the `applying` guard exists for.
  // A hand-called observeCm6 would never reproduce it.
  let hist: DocumentHistory | undefined;
  const build = (): EditorView =>
    new EditorView({
      state: EditorState.create({
        doc,
        extensions: [history(), EditorView.updateListener.of((u) => hist?.observeCm6(u))],
      }),
    });
  let view = build();
  const hooks: DocumentHistoryHooks = {
    view: () => view,
    generation: () => gen,
    readField: (k) => fields.get(k),
    writeField: (k, v) => {
      if (v === undefined) fields.delete(k);
      else fields.set(k, v);
    },
    surface: (key, removed, restored) => {
      surfaced.push({ key, removed, restored });
      surfaceHook?.();
    },
  };
  hist = new DocumentHistory(hooks);

  const self: Harness = {
    hist,
    get view() {
      return view;
    },
    fields,
    surfaced,
    onSurface: (fn) => {
      surfaceHook = fn;
    },
    generation: () => gen,
    setGeneration: (g) => {
      gen = g;
    },
    swapView: () => {
      const old = view;
      view = build();
      old.destroy();
    },
    destroy: () => view.destroy(),
  } as Harness;
  return self;
}

let h: Harness;
beforeEach(() => {
  h = harness();
});
afterEach(() => {
  h.destroy();
});

/**
 * End the current undo group, so the next typing is a SEPARATE step back.
 *
 * Needed because everything in a test happens inside CM6's 500ms
 * `newGroupDelay`, so consecutive `type()` calls merge into one event — the same
 * merge a real user gets by typing without pausing. A real user separates steps
 * by pausing or moving the caret; a test has to move the caret, since it cannot
 * wait half a second per assertion.
 */
function breakGroup(view: EditorView): void {
  view.dispatch({ selection: { anchor: view.state.selection.main.anchor } });
}

/** Type text as a user event, so CM6's history records it. */
function type(view: EditorView, text: string, at?: number): void {
  const pos = at ?? view.state.doc.length;
  view.dispatch({
    changes: { from: pos, insert: text },
    userEvent: 'input.type',
  });
}

// ── deepEqual ─────────────────────────────────────────────────────────────

describe('deepEqual', () => {
  test('distinguishes undefined from a missing key, which the staleness check depends on', () => {
    // A frontmatter add records `before: undefined`; undoing it must route to a
    // REMOVE, so `undefined` may never compare equal to a present value.
    expect(deepEqual(undefined, undefined)).toBe(true);
    expect(deepEqual(undefined, null)).toBe(false);
    expect(deepEqual(undefined, '')).toBe(false);
  });

  test('compares arrays by position and objects by key', () => {
    expect(deepEqual(['a', 'b'], ['a', 'b'])).toBe(true);
    expect(deepEqual(['a', 'b'], ['b', 'a'])).toBe(false);
    expect(deepEqual({ a: 1, b: [2] }, { b: [2], a: 1 })).toBe(true);
    expect(deepEqual({ a: 1 }, { a: 1, b: undefined })).toBe(false);
  });
});

// ── The cm6 half is reconciled, not counted ───────────────────────────────

describe('observeCm6', () => {
  test('a run of fast typing is ONE step back, not one per character', () => {
    // The reason the log cannot count transactions: CM6 coalesces adjacent
    // input.type events inside newGroupDelay. Five dispatches, one event.
    for (const c of 'hello') type(h.view, c);
    expect(h.hist.depths().undo).toBe(1);
  });

  test('a cursor move mid-word breaks the group into two steps', () => {
    // Same five characters, one selection change in the middle — and now it is
    // two undo events. This is why the count is unpredictable and must be read
    // from CM6 rather than derived.
    type(h.view, 'ab');
    h.view.dispatch({ selection: { anchor: 0 } });
    type(h.view, 'cd', 0);
    expect(h.hist.depths().undo).toBe(2);
  });

  test('an external update that prunes CM6 history drops the same markers here', () => {
    type(h.view, 'hello');
    expect(h.hist.depths().undo).toBe(1);
    // What applyExternal does: change the doc without adding to history.
    h.view.dispatch({
      changes: { from: 0, to: h.view.state.doc.length, insert: 'replaced' },
      annotations: Transaction.addToHistory.of(false),
    });
    // CM6 kept no undoable event, so neither does the log — pressing ⌘Z here
    // would otherwise walk a marker CM6 has already forgotten.
    expect(h.hist.depths().undo).toBe(0);
  });

  test('crossing CM6 depth cap adds a step instead of removing one', () => {
    // The nastiest branch. `minDepth: 100` makes CM6 discard a BATCH of its
    // OLDEST events once the branch grows past ~120 — so the depth FALLS at the
    // very moment a new edit was made. Popping trailing markers there (the right
    // response to a prune, which slices from the newest end) is exactly
    // backwards, and the log would drift further from CM6 with every keystroke
    // past the cap. The two cases are told apart by the addToHistory annotation,
    // not by the numbers, which cannot distinguish them.
    // A frontmatter entry sits in the MIDDLE of the body edits, and that is the
    // whole design of this fixture. cm6 markers carry no content, so a run of
    // them is homogeneous — dropping from the newest end instead of the oldest
    // leaves an identical COUNT and an identical array, and a test that asserts
    // only `depths().undo` cannot tell the two apart. (It could not: reversing
    // the drop end left this test green.) An fm entry is never dropped, so it is
    // a fixed landmark, and the question "how many body steps are still
    // reachable AFTER it" has different answers for the two ends.
    for (let i = 0; i < 60; i++) {
      breakGroup(h.view);
      type(h.view, `${i} `);
    }
    h.hist.recordField('tags', undefined, ['x']);
    h.fields.set('tags', ['x']);
    for (let i = 0; i < 70; i++) {
      breakGroup(h.view);
      type(h.view, `${i} `);
    }
    // The cap really did fire, or this test proves nothing.
    const capped = undoDepth(h.view.state);
    expect(capped).toBeLessThan(130);
    // The log tracks CM6 exactly, rather than having drifted by the number of
    // edits made after the cap. Plus the one frontmatter entry, which is this
    // log's own and not CM6's.
    expect(h.hist.depths().undo).toBe(capped + 1);

    // What CM6 forgot is OLDER than the chip change, so all seventy body edits
    // made after it are still reachable and the chip change comes back on the
    // seventy-first press. Discarding from the newest end would put it sooner.
    for (let i = 0; i < 70; i++) {
      expect(h.hist.undo()).toBe(true);
      expect(h.fields.get('tags')).toEqual(['x']);
    }
    expect(h.hist.undo()).toBe(true);
    expect(h.fields.has('tags')).toBe(false);
  });

  test('a prune with steps on BOTH sides of the pointer strands neither', () => {
    // applyExternal maps both of CM6's branches, so an external replacement can
    // invalidate undo steps and redo steps in the same update. Reconciling only
    // the side that happened to change leaves markers on the other one pointing
    // at events CM6 has forgotten — and a marker CM6 will not honour makes ⌘Z a
    // silent no-op, the failure this module exists to prevent.
    // The two steps have to be an insertion and a DELETION, in that order, and
    // that asymmetry is not incidental. CM6 discards a stored event when mapping
    // it through the external change leaves it empty. What is stored on the undo
    // side is the edit's INVERSE, so an insertion parks a deletion there — which
    // a whole-document replace does empty. What is stored on the redo side is the
    // edit itself, so only a deletion parks something a replace can empty; a
    // redo-side insertion just re-inserts and survives anything.
    //
    // Get that backwards and the update prunes one branch instead of two, which
    // is precisely the case the bug handled correctly. Two earlier fixtures here
    // — both insertions, then both deletions — each left one branch untouched,
    // so restoring the bug kept them green.
    const g = harness('tail');
    try {
      type(g.view, 'abc'); // insertion → droppable on the UNDO side
      breakGroup(g.view);
      g.view.dispatch({ changes: { from: 0, to: 4, insert: '' }, userEvent: 'delete' });
      g.hist.undo(); // the deletion moves to the redo side, droppable there
      expect(g.hist.depths()).toEqual({ undo: 1, redo: 1 });

      // Replace the whole document, which maps BOTH branches to nothing. That
      // simultaneity is the point: reconciling the redo side only when the undo
      // side came out even (the shape this started as) skips exactly the update
      // that invalidated both, which is the only update that can strand a redo
      // marker in the first place.
      g.view.dispatch({
        changes: { from: 0, to: g.view.state.doc.length, insert: 'replaced' },
        annotations: Transaction.addToHistory.of(false),
      });
      // The premises: CM6 really did drop events on both sides, and the two
      // sides did NOT come out even. Without these the test can pass while
      // reconciling nothing.
      expect(undoDepth(g.view.state)).toBe(0);
      expect(redoDepth(g.view.state)).toBe(0);

      // Whatever CM6 kept, the log offers exactly that and no more, on each side.
      expect(g.hist.depths().undo).toBe(undoDepth(g.view.state));
      expect(g.hist.depths().redo).toBe(redoDepth(g.view.state));
    } finally {
      g.destroy();
    }
  });

  test('a selection-only change records nothing', () => {
    type(h.view, 'hello');
    const before = h.hist.depths();
    h.view.dispatch({ selection: { anchor: 1 } });
    expect(h.hist.depths()).toEqual(before);
  });
});

// ── Re-entrancy: the listener fires inside the undo's own dispatch ────────

describe('the applying guard', () => {
  test('undo leaves a redo behind instead of eating its own marker', () => {
    // Without the guard, the update listener runs synchronously inside
    // cmUndo's dispatch, sees the depth fall, and pops the very marker being
    // consumed — so redo would be empty here and the first ⌘Z would be
    // unrepeatable.
    type(h.view, 'hello');
    expect(h.hist.undo()).toBe(true);
    expect(h.hist.depths()).toEqual({ undo: 0, redo: 1 });
  });

  test('redo does not append a duplicate marker', () => {
    type(h.view, 'hello');
    h.hist.undo();
    expect(h.hist.redo()).toBe(true);
    // The symmetric failure: redo raises the depth, and an unguarded listener
    // would read that as a new edit and push a second marker.
    expect(h.hist.depths()).toEqual({ undo: 1, redo: 0 });
  });

  test('an edit made AFTER an undo is itself undoable', () => {
    // The ordinary way a person uses undo: go back a step, then carry on typing.
    // Everything else in this file drives the module forward or backward, never
    // forward again after backward — which is exactly the sequence that catches a
    // floor left pointing at the post-undo depth. When that happens the new edit
    // reconciles to "nothing changed" and silently records no step, so the user's
    // most recent work is the one thing ⌘Z cannot reach.
    type(h.view, 'one');
    breakGroup(h.view);
    type(h.view, 'two');
    expect(h.hist.depths().undo).toBe(2);

    h.hist.undo();
    expect(h.hist.depths()).toEqual({ undo: 1, redo: 1 });

    breakGroup(h.view);
    type(h.view, 'three');
    expect(h.hist.depths()).toEqual({ undo: 2, redo: 0 });

    // And it really steps back, rather than merely counting.
    const afterEdit = h.view.state.doc.toString();
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).not.toBe(afterEdit);
  });

  test('repeated undo walks all the way back through several steps', () => {
    type(h.view, 'a');
    breakGroup(h.view);
    type(h.view, 'b');
    breakGroup(h.view);
    type(h.view, 'c');

    expect(h.hist.undo()).toBe(true);
    expect(h.hist.undo()).toBe(true);
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).toBe('');
    expect(h.hist.undo()).toBe(false);
    // Every step is still available to redo, in order.
    expect(h.hist.depths()).toEqual({ undo: 0, redo: 3 });
  });

  test('a native CM6 REDO moves the pointer forward by exactly one step', () => {
    // The undo direction had a test; this one did not, and an off-by-one here
    // (landing the pointer before the marker CM6 just replayed rather than after
    // it) breaks the invariant on every native redo without touching undo.
    type(h.view, 'a');
    breakGroup(h.view);
    type(h.view, 'b');
    cmUndo(h.view);
    expect(h.hist.depths()).toEqual({ undo: 1, redo: 1 });

    cmRedo(h.view);

    expect(h.hist.depths()).toEqual({ undo: 2, redo: 0 });
    // And the log agrees with CM6 about where the pointer is: nothing left to
    // redo on either side.
    expect(redoDepth(h.view.state)).toBe(0);
    expect(h.hist.redo()).toBe(false);
  });

  test('a chip reaction that records DURING an undo cannot corrupt the log', () => {
    // `surface()` promotes or flashes a chip, and for real that can close and
    // reopen the chip's popover — whose boundary capture calls straight back into
    // recordField. Landing between applyStep returning and the pointer moving, it
    // truncated the redo tail against a pointer that had not moved yet: the step
    // just undone came back marked undoable AGAIN, with a fabricated entry in the
    // redo slot. The guard spans the whole walk, so the re-entrant call is a no-op.
    h.hist.recordField('draft', undefined, true);
    h.fields.set('draft', true);
    expect(h.hist.depths()).toEqual({ undo: 1, redo: 0 });

    h.onSurface(() => h.hist.recordField('title', 'old', 'new'));
    expect(h.hist.undo()).toBe(true);
    h.onSurface(null);

    // One step went back and it is redoable. Not: two undoable steps and a
    // phantom redo.
    expect(h.hist.depths()).toEqual({ undo: 0, redo: 1 });
    expect(h.fields.has('draft')).toBe(false);
    expect(h.hist.redo()).toBe(true);
    expect(h.fields.get('draft')).toBe(true);
  });

  test('the depth floor follows a prune down even when CM6 drove the undo', () => {
    // The floor clamp is the one piece of reconciliation here that is NOT
    // absolute — it is a one-way latch on an event already gone by, so a branch
    // that returns before reaching it leaves the floor permanently too high and
    // nothing later repairs it. The native-undo branch used to return first.
    type(h.view, 'hello');
    h.hist.reset(); // floor parks at 1
    cmUndo(h.view); // depth 0, under the floor — the clamp has to see THIS update

    // No intervening update: any later one would clamp the floor down anyway, so
    // only the very next edit can tell a skipped clamp from a delayed one.
    type(h.view, 'new work');

    // Reachable. Without the clamp the target computes to zero, no marker is
    // pushed, and the user's newest work is the one thing ⌘Z cannot reach —
    // permanently, not until the next update.
    expect(h.hist.depths().undo).toBe(1);
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).not.toContain('new work');
  });

  test('an undo made through CM6 ITSELF moves the pointer, it does not add a step', () => {
    // CM6's history is reachable without going through this module at all:
    // `minimalSetup` bundles `historyKeymap`, and `history()` also installs a
    // `beforeinput` handler for `historyUndo`/`historyRedo`, so the macOS
    // Edit▸Undo item and the trackpad undo gesture both land here directly.
    //
    // That transaction changes the document and carries no `addToHistory: false`,
    // so it read as an ordinary edit — and since it LOWERS the depth, it took the
    // cap branch: append a marker at the end, delete one off the front. The log
    // then said the oldest surviving edit was the newest action.
    type(h.view, 'a');
    breakGroup(h.view);
    h.hist.recordField('tags', undefined, ['x']);
    h.fields.set('tags', ['x']);
    breakGroup(h.view);
    type(h.view, 'b');
    expect(h.hist.depths()).toEqual({ undo: 3, redo: 0 });

    cmUndo(h.view); // straight to CM6, bypassing this module

    // A step moved from the undoable side to the redoable side. Nothing was
    // created and nothing was discarded.
    expect(h.hist.depths()).toEqual({ undo: 2, redo: 1 });
    // And the order survived: the next step back is the chip change, which is
    // what actually happened before the body edit CM6 just reversed.
    expect(h.hist.undo()).toBe(true);
    expect(h.fields.has('tags')).toBe(false);
  });
});

// ── The two halves share one order ────────────────────────────────────────

describe('one log across both surfaces', () => {
  test('undo walks back through body and frontmatter in the order they happened', () => {
    type(h.view, 'body');
    breakGroup(h.view);
    h.fields.set('tags', ['a']);
    h.hist.recordField('tags', undefined, ['a']);
    type(h.view, ' more');

    // Most recent first: the second body edit, then the chip, then the first.
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).toBe('body');
    expect(h.fields.get('tags')).toEqual(['a']);

    expect(h.hist.undo()).toBe(true);
    expect(h.fields.has('tags')).toBe(false); // before was undefined → removed
    expect(h.view.state.doc.toString()).toBe('body');

    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).toBe('');

    expect(h.hist.undo()).toBe(false);
  });

  test('undoing an ADD removes the field rather than writing undefined over it', () => {
    h.fields.set('draft', true);
    h.hist.recordField('draft', undefined, true);
    h.hist.undo();
    expect(h.fields.has('draft')).toBe(false);
    // And the user is told, with the signal a vanished chip needs.
    expect(h.surfaced).toEqual([{ key: 'draft', removed: true, restored: false }]);
  });

  test('undoing a REMOVAL reports the field as restored, which is what moves it to the front', () => {
    // A chip that was just re-created can be anywhere in the bar, including
    // collapsed inside the overflow `+N`. `removed` and `restored` are not two
    // names for the same edge: the wash plays on a chip nobody can see unless
    // the restore is distinguished from an ordinary value change.
    h.fields.set('tags', undefined);
    h.hist.recordField('tags', ['a'], undefined); // the user removed it
    h.hist.undo();
    expect(h.fields.get('tags')).toEqual(['a']);
    expect(h.surfaced).toEqual([{ key: 'tags', removed: false, restored: true }]);
  });

  test('an ordinary value change is neither removed nor restored', () => {
    // The other half of the same guard: `restored` must not fire for a chip
    // that was already on screen, or every undo reorders the bar.
    h.fields.set('title', 'new');
    h.hist.recordField('title', 'old', 'new');
    h.hist.undo();
    expect(h.surfaced).toEqual([{ key: 'title', removed: false, restored: false }]);
  });

  test('a new edit discards the redo tail', () => {
    type(h.view, 'one');
    h.hist.undo();
    expect(h.hist.depths().redo).toBe(1);
    type(h.view, 'two');
    expect(h.hist.depths().redo).toBe(0);
  });

  test('a no-op field change records nothing', () => {
    h.hist.recordField('title', 'same', 'same');
    expect(h.hist.depths().undo).toBe(0);
  });
});

// ── Dead entries are walked past, never applied silently ──────────────────

describe('stale entries', () => {
  test('a field changed on disk since the entry is skipped, and the walk continues', () => {
    type(h.view, 'body');
    h.fields.set('title', 'mine');
    h.hist.recordField('title', 'old', 'mine');
    // onFileChanged replaces store state wholesale, with no history entry.
    h.fields.set('title', 'theirs');

    // One press: the title entry no longer describes reality, so it is dropped
    // and the body edit is undone instead. The external value survives.
    expect(h.hist.undo()).toBe(true);
    expect(h.fields.get('title')).toBe('theirs');
    expect(h.view.state.doc.toString()).toBe('');
    expect(h.surfaced).toEqual([]);
  });

  test('entries from a previous document are unreachable after the generation bumps', () => {
    type(h.view, 'body');
    h.fields.set('tags', ['a']);
    h.hist.recordField('tags', undefined, ['a']);
    h.setGeneration(2);

    expect(h.hist.undo()).toBe(false);
    expect(h.hist.depths()).toEqual({ undo: 0, redo: 0 });
    expect(h.fields.get('tags')).toEqual(['a']); // untouched
  });

  test('reset forgets the log and re-reads CM6 depth', () => {
    type(h.view, 'hello');
    h.hist.reset();
    expect(h.hist.depths()).toEqual({ undo: 0, redo: 0 });
    // And a subsequent edit is recorded from the new baseline rather than
    // re-pushing the events CM6 still holds. The group break is what makes the
    // next edit a new CM6 event at all — without it the depth never rises and
    // there is nothing for the module to notice. (Production never sees that
    // case: reset() runs at teardown, where the view is destroyed with it.)
    breakGroup(h.view);
    type(h.view, ' again');
    expect(h.hist.depths().undo).toBe(1);
  });

  test('a prune that empties ONLY the redo branch is still noticed', () => {
    // The redo reconciliation sits outside the if/else on purpose. One applyExternal
    // maps both of CM6's branches, and it can empty one while leaving the other
    // intact — here the undo branch holds a deletion's inverse (an insertion,
    // which survives any remapping) and the redo branch holds a deletion (which a
    // whole-doc replace maps to nothing). So the undo depth does not move, and a
    // redo reconciliation reached only when it moved would never run: the log
    // keeps a redo marker CM6 cannot serve, and the redo button lies.
    const g = harness('abcdefgh');
    g.view.dispatch({ changes: { from: 0, to: 2 } });
    breakGroup(g.view);
    g.view.dispatch({ changes: { from: 0, to: 2 } });
    expect(g.hist.undo()).toBe(true);
    expect(g.hist.depths()).toEqual({ undo: 1, redo: 1 });

    g.view.dispatch({
      changes: { from: 0, to: g.view.state.doc.length, insert: 'zz' },
      annotations: Transaction.addToHistory.of(false),
    });

    // The premises: the undo side did NOT move, the redo side emptied.
    expect(undoDepth(g.view.state)).toBe(1);
    expect(redoDepth(g.view.state)).toBe(0);
    expect(g.hist.depths()).toEqual({ undo: 1, redo: 0 });
    expect(g.hist.canRedo()).toBe(false);
    g.destroy();
  });

  test('a prune below the floor lets the floor down with it', () => {
    // reset() parks the floor at CM6's current depth. If a prune then takes the
    // real depth BELOW that floor and the floor does not follow, every later
    // comparison is against a baseline no longer reachable — the target goes
    // negative and stays negative, so the next edits record no marker at all and
    // ⌘Z silently does nothing to the newest work. The floor has to be a floor.
    type(h.view, 'hello');
    h.hist.reset(); // floor parks at 1
    breakGroup(h.view);
    type(h.view, ' more'); // depth 2, one marker
    expect(h.hist.depths().undo).toBe(1);

    // A prune to nothing — depth 0, which is under the parked floor.
    h.view.dispatch({
      changes: { from: 0, to: h.view.state.doc.length, insert: 'replaced' },
      annotations: Transaction.addToHistory.of(false),
    });
    expect(undoDepth(h.view.state)).toBe(0);
    expect(h.hist.depths().undo).toBe(0);

    // The next edit is reachable again, which is the whole point.
    breakGroup(h.view);
    type(h.view, '!');
    expect(h.hist.depths().undo).toBe(1);
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).toBe('replaced');
  });

  test('the FIRST edit in a newly opened document is undoable', () => {
    // The file-switch lifecycle, and the one place the depth floor can strand a
    // real edit. reset() runs BEFORE the teardown (it has to — ChipBar.destroy
    // fires a boundary capture, which must not stamp itself with the new
    // generation), so it reads the depth of the OUTGOING editor. The incoming one
    // starts at zero. If the floor stayed where the old document left it, the
    // first thing the user types in the new file reconciles to "nothing changed"
    // and ⌘Z does nothing at all — on a fresh document, which is when a user is
    // most likely to try it.
    for (let i = 0; i < 3; i++) {
      breakGroup(h.view);
      type(h.view, `old ${i} `);
    }
    expect(h.hist.depths().undo).toBe(3);

    h.hist.reset(); // before the destroy, per the teardown contract
    h.swapView(); // the new document's editor

    type(h.view, 'first edit in the new file');
    expect(h.hist.depths().undo).toBe(1);
    expect(h.hist.undo()).toBe(true);
    expect(h.view.state.doc.toString()).toBe('');
  });

});

// ── The buttons' enable/disable predicates ────────────────────────────────

describe('canUndo / canRedo', () => {
  test('each side reports empty exactly when it is empty', () => {
    // These two one-liners are the only thing standing between a disabled button
    // and a button that lies. They were briefly deleted as redundant with the
    // depths() assertions elsewhere — they are not: `pointer > 1` instead of
    // `pointer > 0` passes the whole rest of the file.
    expect(h.hist.canUndo()).toBe(false);
    expect(h.hist.canRedo()).toBe(false);
    type(h.view, 'x');
    expect(h.hist.canUndo()).toBe(true);
    expect(h.hist.canRedo()).toBe(false);
    h.hist.undo();
    expect(h.hist.canUndo()).toBe(false);
    expect(h.hist.canRedo()).toBe(true);
  });
});

// ── Opening a document again ──────────────────────────────────────────────

describe('restore after the document is opened again', () => {
  test('a field change survives snapshot, reset and restore under the new generation', () => {
    h.fields.set('title', 'new');
    h.hist.recordField('title', 'old', 'new');
    const snap = h.hist.snapshot();
    h.hist.reset();
    h.setGeneration(2);
    h.hist.restore(snap);

    expect(h.hist.canUndo()).toBe(true);
    expect(h.hist.undo()).toBe(true);
    expect(h.fields.get('title')).toBe('old');
  });
});
