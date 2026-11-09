import { EditorView } from '@codemirror/view';
import {
  findNext,
  findPrevious,
  replaceNext,
  replaceAll,
  setSearchQuery,
  SearchQuery,
  SearchCursor,
} from '@codemirror/search';
import { createFindPill, type FindPill } from "../components/find-pill";

export class CmFindController {
  private pill: FindPill | null = null;
  private currentQuery = '';

  open(view: EditorView, container: HTMLElement): void {
    if (this.pill) {
      this.pill.focusInput();
      return;
    }

    this.pill = createFindPill({
      mode: 'find-replace',
      onQueryChange: (q) => {
        this.currentQuery = q;
        this._updateSearch(view, q);
      },
      onFindNext: () => { findNext(view); this._updateCounter(view); },
      onFindPrev: () => { findPrevious(view); this._updateCounter(view); },
      onReplace: (replacement) => {
        const query = new SearchQuery({
          search: this.currentQuery,
          replace: replacement,
          caseSensitive: false,
        });
        view.dispatch({ effects: setSearchQuery.of(query) });
        replaceNext(view);
        this._updateCounter(view);
      },
      onReplaceAll: (replacement) => {
        const query = new SearchQuery({
          search: this.currentQuery,
          replace: replacement,
          caseSensitive: false,
        });
        view.dispatch({ effects: setSearchQuery.of(query) });
        replaceAll(view);
        this._updateCounter(view);
      },
      onClose: () => this.close(view),
    });

    container.appendChild(this.pill.el);
    this.pill.focusInput();
  }

  close(view?: EditorView): void {
    if (!this.pill) return;
    this.pill.destroy();
    this.pill = null;
    this.currentQuery = '';
    if (view) {
      view.dispatch({
        effects: setSearchQuery.of(new SearchQuery({ search: '' })),
      });
      view.focus();
    }
  }

  isOpen(): boolean {
    return this.pill !== null;
  }

  countMatches(view: EditorView, query: string): number {
    if (!query) return 0;
    const cursor = new SearchCursor(
      view.state.doc,
      query.toLowerCase(),
      0,
      view.state.doc.length,
      (s) => s.toLowerCase(),
    );
    let count = 0;
    while (!cursor.done) {
      cursor.next();
      if (!cursor.done) count++;
    }
    return count;
  }

  private _updateSearch(view: EditorView, query: string): void {
    view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({ search: query, caseSensitive: false }),
      ),
    });
    this._updateCounter(view);
  }

  private _updateCounter(view: EditorView): void {
    if (!this.pill) return;
    const total = this.countMatches(view, this.currentQuery);
    if (total === 0 && this.currentQuery) {
      this.pill.setQueryState('no-matches');
      this.pill.setCounter(0, 0);
      return;
    }
    this.pill.setQueryState('normal');
    if (!this.currentQuery) {
      this.pill.clearCounter();
      return;
    }
    const sel = view.state.selection.main;
    let current = 0;
    if (total > 0 && sel.from !== sel.to) {
      const cursor = new SearchCursor(
        view.state.doc,
        this.currentQuery.toLowerCase(),
        0,
        view.state.doc.length,
        (s) => s.toLowerCase(),
      );
      let i = 0;
      while (!cursor.done) {
        cursor.next();
        if (!cursor.done) {
          i++;
          if (cursor.value.from === sel.from) {
            current = i;
            break;
          }
        }
      }
      if (current === 0) current = 1;
    }
    this.pill.setCounter(current, total);
  }
}
