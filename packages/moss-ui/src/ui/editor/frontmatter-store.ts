// packages/moss-ui/src/ui/editor/frontmatter-store.ts
//
// Reactive store for a single file's frontmatter values. ChipBar and any
// future raw-YAML editor are both projections of this store; the file on
// disk is the durable serialization.
//
// Consistency model:
//   1. setField mutates state IMMEDIATELY and notifies subscribers (UI = instant).
//   2. setField asks the single writer to save (150ms debounce, or 0ms with immediate)
//      via the injected `requestSave` — the store performs no IPC itself.
//   3. A disk text another writer produced arrives through onFileChanged, from
//      editor-main's reconcile, which has already dropped moss's own echoes by
//      content hash.
//
// IMPORTANT — the store does not write:
//   The store does not own a path and does not perform any IPC. It requests a
//   save via editor-main's single writer (`requestSave`, wired to
//   saveActiveDocument). That writer reads BOTH the live path (from the
//   registry) AND body+frontmatter fresh at write time, so neither the body
//   nor the frontmatter snapshot can clobber the other and a rename between
//   scheduling and writing can never target the old path. Interleaved
//   frontmatter and body edits converge to the correct merged state regardless
//   of order, because everything is read-at-write-time inside the one writer.
//

const DEFAULT_DEBOUNCE_MS = 150;

export type FrontmatterValues = Record<string, unknown>;

export interface FrontmatterStoreOptions {
  initial: FrontmatterValues;
  /** Request a save through the single writer (editor-main's saveActiveDocument). */
  requestSave: () => Promise<void>;
  /** Debounce for text-like edits. Default 150ms. */
  debounceMs?: number;
}

export interface SetFieldOptions {
  /** Skip debounce — fire the write on the next microtask. Use for toggles, selects, datepicker commits. */
  immediate?: boolean;
}

type Subscriber = (values: FrontmatterValues) => void;

export class FrontmatterStore {
  private state: FrontmatterValues;
  private readonly requestSave: () => Promise<void>;
  private readonly debounceMs: number;
  private writeTimer: ReturnType<typeof setTimeout> | null = null;
  private subscribers: Set<Subscriber> = new Set();

  constructor(opts: FrontmatterStoreOptions) {
    this.state = { ...opts.initial };
    this.requestSave = opts.requestSave;
    this.debounceMs = opts.debounceMs ?? DEFAULT_DEBOUNCE_MS;
  }

  subscribe(cb: Subscriber): () => void {
    this.subscribers.add(cb);
    return () => { this.subscribers.delete(cb); };
  }

  getValues(): FrontmatterValues {
    return { ...this.state };
  }

  setField(key: string, value: unknown, opts: SetFieldOptions = {}): void {
    if (deepEqual(this.state[key], value)) return;
    this.state = { ...this.state, [key]: value };
    this.notify();
    if (opts.immediate) {
      this.clearTimer();
      void this.requestSave();
    } else {
      this.scheduleWrite();
    }
  }

  /**
   * Remove a frontmatter field entirely (YAML key deletion). Equivalent to
   * assigning undefined to the key — used by ChipBar's "Remove field" context
   * menu and the unified "add field → undo" path.
   *
   * Distinct from `setField(key, null)` which keeps the key with a null value.
   * After removeField the key is absent from `getValues()`.
   */
  removeField(key: string): void {
    if (!(key in this.state)) return;
    const next = { ...this.state };
    delete next[key];
    this.state = next;
    this.notify();
    // Remove is a discrete commit (context-menu action), not text typing —
    // write immediately to avoid the "Remove then Undo" gesture falling into
    // the debounce gap.
    this.clearTimer();
    void this.requestSave();
  }

  /** Replace the values with a disk text another writer produced. */
  onFileChanged(values: FrontmatterValues): void {
    if (deepEqualMap(this.state, values)) return;
    this.state = { ...values };
    this.notify();
  }

  /** True while a debounced write is queued (timer pending). */
  hasPendingWrite(): boolean {
    return this.writeTimer !== null;
  }

  /** Cancel a queued debounce timer without requesting a save. */
  cancelPendingWrite(): void {
    this.clearTimer();
  }

  destroy(): void {
    this.clearTimer();
    this.subscribers.clear();
  }

  // ── internals ──────────────────────────────────────────────────────────

  private notify(): void {
    const snapshot = this.getValues();
    for (const cb of this.subscribers) cb(snapshot);
  }

  private scheduleWrite(): void {
    this.clearTimer();
    this.writeTimer = setTimeout(() => {
      this.writeTimer = null;
      void this.requestSave();
    }, this.debounceMs);
  }

  private clearTimer(): void {
    if (this.writeTimer) {
      clearTimeout(this.writeTimer);
      this.writeTimer = null;
    }
  }
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null) return false;
  if (typeof a !== typeof b) return false;
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    return a.every((v, i) => deepEqual(v, b[i]));
  }
  if (typeof a === 'object' && typeof b === 'object') {
    return deepEqualMap(a as FrontmatterValues, b as FrontmatterValues);
  }
  return false;
}

function deepEqualMap(a: FrontmatterValues, b: FrontmatterValues): boolean {
  const ak = Object.keys(a), bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  return ak.every(k => deepEqual(a[k], b[k]));
}
