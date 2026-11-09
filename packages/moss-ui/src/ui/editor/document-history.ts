// One undo history for the whole document — body text AND frontmatter.
//
// The user edits a post in two places: the CodeMirror body and the chip bar's
// frontmatter fields. This keeps ONE chronological log across both, so ⌘Z
// means "undo the last thing I did to this document" rather than "undo the
// last thing in whichever box I happen to be focused on".
//
// The log does not store document content. It stores an ordered list of
// MARKERS — "the next step back is CodeMirror's" or "the next step back is
// setting `tags` to this older value" — and delegates the actual undoing to
// whichever surface owns it. CodeMirror keeps its own history; this module never
// tries to replicate it.
//
// Two consequences of that delegation shape the whole file:
//
//   1. CodeMirror's event count is NOT the number of transactions. It coalesces
//      fast typing, prunes on external updates, and caps its branch — so the
//      cm6 side of the log is RECONCILED against `undoDepth()` on every update
//      rather than counted. See observeCm6.
//   2. Undoing runs through `view.dispatch`, and CM6's update listener fires
//      SYNCHRONOUSLY inside it. Without the `applying` guard the listener would
//      see its own undo as a user action and eat the marker it was consuming.
//
// The four ways a naive count breaks:

import type { EditorView, ViewUpdate } from '@codemirror/view';
import { Transaction } from '@codemirror/state';
import { undo as cmUndo, redo as cmRedo, undoDepth, redoDepth } from '@codemirror/commands';

/**
 * Did this update carry a document change that CM6 filed in its history?
 *
 * This is what separates the two ways the depth can fall. `applyExternal` marks its
 * dispatch `addToHistory.of(false)` (cm-editor.ts) and CM6 then prunes the branch;
 * an ordinary edit carries no such annotation and can only lower the depth by
 * tripping the `minDepth` cap. Read off the same annotation CM6 itself reads,
 * rather than inferred from the numbers, which cannot tell the two apart.
 *
 * CM6's own undo/redo would fool it — `pop()` builds that transaction with
 * `changes` and `userEvent: "undo"` but leaves `addToHistory` unset, so it reads
 * as an ordinary history-adding edit — except that `historyNav` below intercepts
 * it before this is ever consulted.
 */
function wasAddedToHistory(update: ViewUpdate): boolean {
  return update.transactions.some(
    (tr) => tr.docChanged && tr.annotation(Transaction.addToHistory) !== false,
  );
}

/**
 * Was this update CM6 replaying its own history, rather than a new edit?
 *
 * `pop()` stamps its transaction `userEvent: "undo"` / `"redo"`, which is the only
 * thing that distinguishes it — it changes the document and carries no
 * `addToHistory: false`, so by the numbers alone it looks exactly like an
 * ordinary edit that happens to lower the depth, and got treated as one.
 *
 * This path is reachable no matter what keymap the panel installs: `minimalSetup`
 * bundles `historyKeymap`, and `history()` additionally installs a `beforeinput`
 * handler for `historyUndo`/`historyRedo`, which is where the macOS Edit▸Undo
 * item and the trackpad undo gesture arrive.
 */
function historyNav(update: ViewUpdate): 'undo' | 'redo' | null {
  for (const tr of update.transactions) {
    if (tr.isUserEvent('undo')) return 'undo';
    if (tr.isUserEvent('redo')) return 'redo';
  }
  return null;
}

/**
 * One step back. `generation` is the document load it was recorded under, so an
 * entry that outlived its document is unforgeable rather than merely unlikely
 * (a document opened again gets fresh stamps through `restore()`).
 */
export type HistoryEntry =
  | { source: 'cm6'; generation: number }
  | { source: 'fm'; generation: number; key: string; before: unknown; after: unknown };

/** Entries keep the generation they were recorded under; `restore` re-stamps them. */
export type HistorySnapshot = { entries: HistoryEntry[]; pointer: number };

export interface DocumentHistoryHooks {
  /** The live CM6 view, or null between documents / for a non-text file. */
  view(): EditorView | null;
  /** Current load generation. Entries stamped with an older one are dead. */
  generation(): number;
  /** The store's current value for a key — the staleness check reads this. */
  readField(key: string): unknown;
  /** Write a field. `undefined` REMOVES it (see applyFieldValue at the call site). */
  writeField(key: string, value: unknown): void;
  /**
   * Show the user what moved.
   *
   * `removed` is true when the write deleted the field, which needs a different
   * signal: a chip that no longer exists cannot be promoted or flashed.
   *
   * `restored` is true when the field did not exist a moment ago and now does.
   * That chip is brand new, so it can be anywhere in the bar — including
   * collapsed into the overflow `+N` — and highlighting it in place would
   * animate something the user cannot see. It has to be pulled to the front,
   * which an ordinary value change must NOT do (reordering the bar under a
   * pointer-driven reveal is jarring, and nothing moved).
   */
  surface(key: string, removed: boolean, restored: boolean): void;
}

/** Structural equality for frontmatter values. The store's own is module-private. */
export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || a === undefined || b === undefined) return false;
  if (typeof a !== 'object' || typeof b !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
  }
  const ka = Object.keys(a as object);
  const kb = Object.keys(b as object);
  if (ka.length !== kb.length) return false;
  return ka.every(
    (k) =>
      Object.prototype.hasOwnProperty.call(b, k) &&
      deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  );
}

export class DocumentHistory {
  /**
   * The log, oldest first. `[0, pointer)` is undoable and `[pointer, length)` is
   * redoable — one array and one index rather than two stacks, so "what order
   * did these happen in" survives a mix of cm6 and fm entries.
   */
  private entries: HistoryEntry[] = [];
  private pointer = 0;

  /**
   * CM6 depth that corresponds to ZERO cm6 markers here.
   *
   * Normally 0. `reset()` raises it to CM6's current depth, because the log is
   * emptied while the view keeps its own history — so without a floor the next
   * update would re-push markers for events belonging to the previous document.
   */
  private depthFloor = 0;

  /**
   * The view the floor was computed against.
   *
   * A new EditorView starts its history at zero, so the floor must too — and
   * `reset()` cannot do that job, because the teardown contract makes it run
   * BEFORE the old editor is destroyed, when the only view it can read is the
   * outgoing one. Keying on identity means the floor is right without any call
   * site having to be sequenced correctly.
   */
  private observedView: EditorView | null = null;

  /**
   * Set while THIS module is driving a dispatch. CM6's update listener runs
   * synchronously inside `view.dispatch`, so without this an undo would be
   * observed as a user edit — popping the marker it was consuming (killing redo
   * on the first press) or, on redo, appending a duplicate.
   */
  /** Depth, not a flag. `surface()` can make a chip close and reopen a popover,
   * whose boundary capture calls back in here; if that path ever reaches a
   * second `applyStep`, a boolean would be cleared by the INNER call's `finally`
   * while the outer one is still driving, reopening the very window the guard
   * exists to close. A counter nests. */
  private applying = 0;

  constructor(private hooks: DocumentHistoryHooks) {}

  canUndo(): boolean {
    return this.pointer > 0;
  }

  canRedo(): boolean {
    return this.pointer < this.entries.length;
  }

  /** Entry count either side of the pointer — for tests and the button tooltips. */
  depths(): { undo: number; redo: number } {
    return { undo: this.pointer, redo: this.entries.length - this.pointer };
  }

  /**
   * Forget everything and re-read CM6's depth. Called from the teardown sites
   * BEFORE the destroy sequence, never after: `ChipBar.destroy()` closes its
   * popover, which fires the boundary capture, and after the generation has
   * moved on that entry would stamp itself with the INCOMING document's
   * generation — forging exactly the stale entry the stamp exists to prevent.
   * `ChipBar.tearingDown` is what suppresses that emit; this module carried a
   * second `beginTeardown`/`endTeardown` pair for it that nothing ever called,
   * so the belt it described had only ever had one half.
   */
  reset(): void {
    this.entries = [];
    this.pointer = 0;
    this.syncDepth();
  }

  snapshot(): HistorySnapshot {
    return { entries: this.entries.map((e) => ({ ...e })), pointer: this.pointer };
  }

  /** Restored entries take the current generation; the CM6 history comes back with the view. */
  restore(s: HistorySnapshot): void {
    const generation = this.hooks.generation();
    this.entries = s.entries.map((e) => ({ ...e, generation }));
    this.pointer = s.pointer;
  }

  private syncDepth(): void {
    const view = this.hooks.view();
    // `view.state` rather than `view`: a surface can exist without a history to
    // read a depth from. CM6 answers 0 for a state carrying no history field,
    // and a view carrying no state at all means the same thing — there is
    // nothing behind this editor yet, so the floor is the bottom.
    this.depthFloor = view?.state ? undoDepth(view.state) : 0;
    // Pin the floor to the view it was measured on, so the identity check in
    // observeCm6 does not immediately zero it as if this were a new editor.
    this.observedView = view;
  }

  /**
   * cm6 markers either side of the pointer. Both are reconciled, each against
   * its own CM6 branch — a single total is not enough, because a marker sitting
   * in the redo tail satisfies a total while being on the wrong side of the
   * pointer. That is not hypothetical: undo then type, and CM6's depth returns to
   * exactly what it was, so a total-only check sees nothing to do and leaves the
   * superseded redo step reachable.
   */
  private cm6Split(): { before: number; after: number } {
    let before = 0;
    let after = 0;
    for (let i = 0; i < this.entries.length; i++) {
      if (this.entries[i].source !== 'cm6') continue;
      if (i < this.pointer) before++;
      else after++;
    }
    return { before, after };
  }

  /**
   * Move the pointer across one cm6 marker, for a step CM6 took by itself.
   *
   * Frontmatter entries between the pointer and that marker are stepped OVER,
   * not applied — CM6 reversed a body edit and knows nothing about the chip bar,
   * so pretending otherwise would write a field the user did not ask to change.
   *
   * Be precise about what that costs, because it is worse than "they move to the
   * redo side": they land there, and the NEXT body edit truncates the redo tail
   * and deletes them. So a chip change the user made before a native undo becomes
   * permanently unreachable by ⌘Z while still being applied to the document. The
   * redo path at least degrades safely — `applyStep` finds the store no longer
   * holds what the entry claims to reverse, returns false, and the walk drops it
   * and continues rather than clobbering.
   *
   * This is the unavoidable price of a route that bypasses the module: neither
   * choice is right, and writing the field would be the worse one. It is why the
   * panel binds ⌘Z at `Prec.highest` — the goal is for this path to stay rare,
   * not to be made correct in a way the keymap makes unnecessary.
   */
  private stepPointer(dir: 'undo' | 'redo'): void {
    if (dir === 'undo') {
      for (let i = this.pointer - 1; i >= 0; i--) {
        if (this.entries[i].source === 'cm6') {
          this.pointer = i;
          return;
        }
      }
    } else {
      for (let i = this.pointer; i < this.entries.length; i++) {
        if (this.entries[i].source === 'cm6') {
          this.pointer = i + 1;
          return;
        }
      }
    }
  }

  /** A new entry invalidates everything ahead of the pointer. */
  private truncateRedoTail(): void {
    if (this.pointer < this.entries.length) this.entries.length = this.pointer;
  }

  private push(entry: HistoryEntry): void {
    this.truncateRedoTail();
    this.entries.push(entry);
    this.pointer = this.entries.length;
  }

  /**
   * Reconcile the cm6 half against CodeMirror's own depth. Call from an
   * `EditorView.updateListener`.
   *
   * Reading the depth rather than counting transactions is not defensive
   * programming — a count is wrong four separate ways. CM6 coalesces runs of
   * typing inside `newGroupDelay` (so `hello` is one event, unless the caret
   * moved mid-word), can carry several transactions in one update, PRUNES the
   * branch tail when `applyExternal` dispatches `addToHistory.of(false)`, and caps
   * the branch at `minDepth: 100`.
   *
   * THE INVARIANT, both halves: the cm6 markers BEFORE the pointer number
   * `undoDepth - depthFloor`, and those after it never exceed `redoDepth`. Every
   * branch below restores it, which is what makes the reconciliation
   * self-correcting — the log cannot drift a little and stay drifted, whatever
   * CM6 did.
   *
   * WHERE a marker is added or removed is the part the depth cannot tell you:
   *
   *   too few   the user made an edit → append, so it lands after any frontmatter
   *             entry that preceded it.
   *   too many, and this update filed a doc change in history → the CAP. CM6
   *             discarded its OLDEST events to make room for a new one, so the
   *             new edit is appended AND the dead markers come off the FRONT.
   *   too many, otherwise → a prune (`applyExternal` maps both branches and slices
   *             from the newest end) or an undo driven from outside this module.
   *             Trailing markers come off. This is also the default, because it
   *             is the safe direction: a marker CM6 has forgotten makes ⌘Z a
   *             silent no-op, which is worse than a step the user cannot reach.
   */
  observeCm6(update: ViewUpdate): void {
    if (this.applying > 0) return;

    // A different editor than the floor was computed against — a file switch
    // built a new one. Its history starts empty, so the floor returns to zero.
    if (update.view !== this.observedView) {
      this.observedView = update.view;
      this.depthFloor = 0;
    }

    const d = undoDepth(update.state);
    // A prune can take the depth below the floor reset() recorded; the floor
    // follows it down, or the target would go negative and stick.
    //
    // This runs BEFORE the history-nav branch below, and must. Every other piece
    // of reconciliation here is absolute — it recomputes from the current depth,
    // so a skipped update is repaired by the next one. This clamp is the one
    // exception: it is edge-triggered, a one-way latch on an event that has
    // already gone by. Skip it once and the floor stays permanently too high, so
    // the next edit reconciles to "nothing changed" and is unreachable by ⌘Z
    // forever. Whether it healed depended on an unrelated update happening to
    // land in between.
    if (d < this.depthFloor) this.depthFloor = d;

    // CM6's history is reachable without passing through this module: the
    // `historyKeymap` inside `minimalSetup`, and a `beforeinput` handler for
    // `historyUndo`/`historyRedo` that catches the macOS Edit▸Undo item and the
    // trackpad gesture. Such a step is the pointer MOVING, not a step created or
    // destroyed — and every other branch below would get it wrong: it lowers the
    // depth, so it read as a prune and deleted the marker outright.
    const nav = historyNav(update);
    if (nav) {
      this.stepPointer(nav);
      return;
    }

    const target = d - this.depthFloor;
    const { before } = this.cm6Split();

    if (target > before) {
      // The user made an edit. push() truncates the redo tail, which is exactly
      // right here and load-bearing: a new CM6 edit clears CM6's whole undone
      // branch in one go (`addChanges` returns a state with an empty `undone`),
      // so every redo step — frontmatter ones included — is superseded.
      for (let i = before; i < target; i++) {
        this.push({ source: 'cm6', generation: this.hooks.generation() });
      }
    } else if (target < before) {
      if (update.docChanged && wasAddedToHistory(update)) {
        this.push({ source: 'cm6', generation: this.hooks.generation() });
        this.dropCm6(before + 1 - target, 'oldest', 'undoable');
      } else {
        this.dropCm6(before - target, 'newest', 'undoable');
      }
    }

    // The redo side is reconciled SEPARATELY and unconditionally, because
    // `applyExternal` maps both of CM6's branches — one update can invalidate undo
    // steps and redo steps at once, and handling only the side that happened to
    // change strands markers on the other. Only ever drop here: a redo step the
    // user never performed in this document must never be invented.
    const stranded = this.cm6Split().after - redoDepth(update.state);
    if (stranded > 0) this.dropCm6(stranded, 'newest', 'redo');
    // target === count is the ordinary coalescing case: the edit merged into the
    // previous event (a run of typing inside newGroupDelay), so it is ONE step
    // back and there is nothing to record. An earlier draft pushed here whenever
    // the doc had changed, which put one marker per KEYSTROKE in the log — the
    // exact outcome reconciling against undoDepth exists to avoid.
  }

  /**
   * Remove `count` cm6 markers from one end of one REGION, leaving frontmatter
   * entries alone.
   *
   * The region matters as much as the end. The two sides of the pointer answer to
   * different CM6 branches, so a count derived from the undoable side must be
   * taken from the undoable side — scanning the whole array from its newest end
   * would eat redo markers to satisfy an undo-side deficit and leave both sides
   * wrong.
   */
  private dropCm6(count: number, from: 'oldest' | 'newest', region: 'undoable' | 'redo'): void {
    let remaining = count;
    if (from === 'newest') {
      const floor = region === 'undoable' ? 0 : this.pointer;
      const start = region === 'undoable' ? this.pointer - 1 : this.entries.length - 1;
      for (let i = start; i >= floor && remaining > 0; i--) {
        if (this.entries[i].source !== 'cm6') continue;
        this.drop(i);
        remaining--;
      }
    } else {
      const start = region === 'undoable' ? 0 : this.pointer;
      // `drop` splices in place, so after a removal `i` already points at the
      // next entry; the bound is re-read each pass for the same reason.
      for (let i = start; i < (region === 'undoable' ? this.pointer : this.entries.length) && remaining > 0; ) {
        if (this.entries[i].source !== 'cm6') {
          i++;
          continue;
        }
        this.drop(i);
        remaining--;
      }
    }
  }

  /**
   * Record one frontmatter change. Called at the popover boundary — one entry
   * per visit to a field, not one per keystroke, because the chip widgets commit
   * on `input` and a 60-character description would otherwise be 60 entries and
   * make ⌘Z strictly worse than the native undo it replaces.
   */
  recordField(key: string, before: unknown, after: unknown): void {
    if (this.applying > 0) return;
    if (deepEqual(before, after)) return;
    this.push({ source: 'fm', generation: this.hooks.generation(), key, before, after });
  }

  /**
   * Step back one visible change, or return false if there is none.
   *
   * The walk is the point. A marker can be dead — its generation expired, CM6
   * refused it, or the store no longer holds the value the entry claims to
   * reverse (an external file change replaces store state wholesale with no
   * entry of its own). A dead marker is dropped and the walk CONTINUES, so one
   * press always produces one visible change or reports that it cannot. A silent
   * no-op would be the worst outcome: the user presses again, and the second
   * press destroys something.
   *
   * The `applying` guard wraps the WHOLE walk, not just each dispatch. `surface()`
   * can make a chip close and reopen its popover, and that popover's boundary
   * capture calls straight back into `recordField` — which, if it landed between
   * `applyStep` returning and `pointer--` running, would truncate the redo tail
   * against a pointer that had not moved yet. The step just undone came back
   * marked undoable AGAIN, with a fabricated entry sitting in the redo slot.
   */
  undo(): boolean {
    this.applying++;
    try {
      return this.walkUndo();
    } finally {
      this.applying--;
    }
  }

  private walkUndo(): boolean {
    while (this.pointer > 0) {
      const at = this.pointer - 1;
      const entry = this.entries[at];
      if (this.applyStep(entry, 'undo')) {
        this.pointer--;
        return true;
      }
      this.drop(at);
    }
    return false;
  }

  redo(): boolean {
    this.applying++;
    try {
      return this.walkRedo();
    } finally {
      this.applying--;
    }
  }

  private walkRedo(): boolean {
    while (this.pointer < this.entries.length) {
      const at = this.pointer;
      const entry = this.entries[at];
      if (this.applyStep(entry, 'redo')) {
        this.pointer++;
        return true;
      }
      this.drop(at);
    }
    return false;
  }

  /** Remove a dead entry entirely — it must not be reachable from either side. */
  private drop(index: number): void {
    this.entries.splice(index, 1);
    if (this.pointer > index) this.pointer--;
  }

  /** Try one entry. False means "dead, drop it and keep walking". */
  private applyStep(entry: HistoryEntry, dir: 'undo' | 'redo'): boolean {
    if (entry.generation !== this.hooks.generation()) return false;

    if (entry.source === 'cm6') {
      const view = this.hooks.view();
      if (!view) return false;
      this.applying++;
      let ok: boolean;
      try {
        ok = dir === 'undo' ? cmUndo(view) : cmRedo(view);
      } finally {
        this.applying--;
      }
      // Deliberately NOT syncDepth() here. The floor answers "what depth means
      // zero markers", which only `reset()` changes — a history-driven step moves
      // CM6's depth and this log's pointer by one together, so the invariant holds
      // by construction. Re-floating the floor to the post-undo depth instead made
      // the NEXT edit reconcile to "nothing changed", so typing after an undo
      // recorded no step and the user's newest work was the one thing ⌘Z could not
      // reach. Caught by 'an edit made AFTER an undo is itself undoable'.
      return ok;
    }

    // Frontmatter. Apply only if the store still holds what this entry claims to
    // be reversing — otherwise something outside the history (a file change on
    // disk) has moved it, and writing would clobber that silently.
    const expected = dir === 'undo' ? entry.after : entry.before;
    const target = dir === 'undo' ? entry.before : entry.after;
    if (!deepEqual(this.hooks.readField(entry.key), expected)) return false;

    this.applying++;
    try {
      this.hooks.writeField(entry.key, target);
    } finally {
      this.applying--;
    }
    this.hooks.surface(entry.key, target === undefined, expected === undefined && target !== undefined);
    return true;
  }
}
