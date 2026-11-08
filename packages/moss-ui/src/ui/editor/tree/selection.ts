import type { EntryId } from '../entry-registry';

export type SelectionSnapshot = {
  readonly selected: ReadonlySet<EntryId>;
  readonly anchor: EntryId | null;
  readonly focus: EntryId | null;
};

export class FileSelection {
  private _selected = new Set<EntryId>();
  private _anchor: EntryId | null = null;
  private _focus: EntryId | null = null;

  get size(): number { return this._selected.size; }
  get anchor(): EntryId | null { return this._anchor; }
  get focus(): EntryId | null { return this._focus; }

  has(id: EntryId): boolean { return this._selected.has(id); }

  setSingle(id: EntryId): void {
    this._selected.clear();
    this._selected.add(id);
    this._anchor = id;
    this._focus = id;
  }

  toggle(id: EntryId): void {
    if (this._selected.has(id)) {
      // Deselect: focus moves to the deselected row (ARIA convention) but
      // anchor stays at the last additive action — matches Finder/VS Code.
      this._selected.delete(id);
      this._focus = id;
    } else {
      this._selected.add(id);
      this._anchor = id;
      this._focus = id;
    }
  }

  setRange(id: EntryId, visibleOrder: readonly EntryId[]): void {
    const range = computeRange(this._anchor ?? id, id, visibleOrder);
    this._selected.clear();
    for (const x of range) this._selected.add(x);
    this._focus = id;
  }

  addRange(id: EntryId, visibleOrder: readonly EntryId[]): void {
    const from = this._anchor ?? id;
    const range = computeRange(from, id, visibleOrder);
    for (const x of range) this._selected.add(x);
    this._focus = id;
  }

  clear(): void {
    this._selected.clear();
    this._anchor = null;
    this._focus = null;
  }

  setFocus(id: EntryId | null): void {
    this._focus = id;
  }

  reconcile(validIds: ReadonlySet<EntryId>): void {
    for (const id of [...this._selected]) {
      if (!validIds.has(id)) this._selected.delete(id);
    }
    // When anchor or focus disappears, fall back to any surviving member so
    // subsequent Shift+click has a real starting point instead of silently
    // truncating the range to a single item.
    const survivor = (): EntryId | null => {
      for (const id of this._selected) return id;
      return null;
    };
    if (this._anchor !== null && !validIds.has(this._anchor)) {
      this._anchor = survivor();
    }
    if (this._focus !== null && !validIds.has(this._focus)) {
      this._focus = survivor();
    }
  }

  snapshot(): SelectionSnapshot {
    return {
      selected: new Set(this._selected),
      anchor: this._anchor,
      focus: this._focus,
    };
  }
}

function computeRange(
  from: EntryId,
  to: EntryId,
  order: readonly EntryId[],
): EntryId[] {
  const i = order.indexOf(from);
  const j = order.indexOf(to);
  if (i === -1 || j === -1) return [to];
  const [lo, hi] = i <= j ? [i, j] : [j, i];
  return order.slice(lo, hi + 1);
}
