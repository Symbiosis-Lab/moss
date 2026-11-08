/**
 * URL Autocomplete - Address bar autocomplete for page navigation
 *
 * Provides a dropdown with filtered page suggestions as the user types.
 * Features:
 * - Instant filtering by title and path
 * - Path-shaped queries rank the page at that path first ("/" is home)
 * - Keyboard navigation (Arrow keys, Enter, Escape)
 * - Mouse interaction (hover, click)
 * - Accessibility (ARIA combobox pattern)
 */

import uFuzzy from '@leeoniya/ufuzzy';
import type { PageInfo } from '@symbiosis-lab/moss-ui';
import { engine } from '../engine';
import { error } from "../log";
import { urlPathToDisplayPath } from './path-utils';
import { t } from "../i18n/index";
import { armOutsideDismiss } from "../shared/ui/dismiss-outside";

interface PageWithDisplay {
  page: PageInfo;
  displayPath: string;
  /** Lowercased display path without its slashes: "" is home. */
  pathKey: string;
}

/** Slashes are navigational context, not search terms. */
function stripSlashes(path: string): string {
  return path.replace(/^\/+|\/+$/g, '');
}

export interface UrlAutocompleteOptions {
  /** Called when user selects a page */
  onSelect: (page: PageInfo) => void;
  /** Called when user cancels (Escape or blur) */
  onCancel: () => void;
  /** Initial value for the input */
  initialValue?: string;
}

export class UrlAutocomplete {
  private container: HTMLElement;
  private options: UrlAutocompleteOptions;
  private pages: PageWithDisplay[] = [];
  private filteredPages: PageWithDisplay[] = [];
  private haystack: string[] = [];
  private uf: uFuzzy;
  private highlightedIndex = 0;
  private pageLoad: { submittedQuery: string | null } | null = { submittedQuery: null };
  private inputElement: HTMLInputElement | null = null;
  private dropdownElement: HTMLElement | null = null;
  private listboxId: string;
  private disarmOutside: (() => void) | null = null;

  constructor(container: HTMLElement, options: UrlAutocompleteOptions) {
    this.container = container;
    this.options = options;
    this.listboxId = `url-autocomplete-listbox-${Date.now()}`;
    this.uf = new uFuzzy({
      unicode: true,
      interSplit: "[^\\p{L}\\d']+",
      intraSplit: "\\p{Ll}\\p{Lu}",
      intraBound: "\\p{L}\\d|\\d\\p{L}|\\p{Ll}\\p{Lu}",
      intraChars: "[\\p{L}\\d']",
      intraContr: "'\\p{L}{1,2}\\b",
      intraMode: 1,
      intraIns: 1,
    });

    this.render();
    this.loadPages();
  }

  private async loadPages(): Promise<void> {
    try {
      const result = await engine.getAllPages();
      const submittedQuery = this.pageLoad?.submittedQuery;
      this.pageLoad = null;
      if (result.status === 'ok') {
        this.pages = result.data.map((page) => {
          const displayPath = urlPathToDisplayPath(page.url_path);
          return { page, displayPath, pathKey: stripSlashes(displayPath).toLowerCase() };
        });
        this.haystack = this.pages.map((p) => `${p.page.title} ${p.displayPath}`);
        this.filterPages(this.inputElement?.value || '');
        if (submittedQuery != null && submittedQuery === this.inputElement?.value) this.selectHighlighted();
      }
    } catch (err) {
      this.pageLoad = null;
      error(`❌ [UrlAutocomplete] Failed to load pages: ${err}`);
      this.pages = [];
      this.filteredPages = [];
      this.haystack = [];
      this.updateDropdown();
    }
  }

  private render(): void {
    const initialValue = this.options.initialValue || '';

    this.container.innerHTML = `
      <div class="moss-url-autocomplete">
        <input
          type="text"
          class="moss-url-autocomplete-input"
          value="${this.escapeHtml(initialValue)}"
          placeholder="${this.escapeHtml(t('preview.search_pages_placeholder'))}"
          role="combobox"
          aria-expanded="true"
          aria-autocomplete="list"
          aria-controls="${this.listboxId}"
          aria-activedescendant=""
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          name="moss-url-search-${Date.now()}"
          data-lpignore="true"
          data-form-type="other"
        />
        <div
          class="moss-url-autocomplete-dropdown"
          role="listbox"
          id="${this.listboxId}"
        >
          <div class="moss-url-autocomplete-loading">${t('preview.search_loading')}</div>
        </div>
      </div>
    `;

    this.inputElement = this.container.querySelector('.moss-url-autocomplete-input');
    this.dropdownElement = this.container.querySelector('.moss-url-autocomplete-dropdown');

    this.setupEventListeners();

    // Focus input
    this.inputElement?.focus();
    this.inputElement?.select();
  }

  private setupEventListeners(): void {
    if (!this.inputElement) return;

    // Input event for filtering
    this.inputElement.addEventListener('input', () => {
      if (this.pageLoad) this.pageLoad.submittedQuery = null;
      this.filterPages(this.inputElement?.value || '');
    });

    // Keyboard navigation
    this.inputElement.addEventListener('keydown', (e) => {
      this.handleKeyDown(e);
    });

    // Every dismiss gesture, including the two a plain document listener here
    // could never see: a click on the shell titlebar, which is a window DRAG
    // REGION (Tauri starts a window drag on mousedown, so no `click` ever
    // fires), and a press in the preview iframe, which is a different document
    // altogether. See dismiss-outside.ts.
    this.disarmOutside = armOutsideDismiss({
      inside: [this.container],
      covers: [this.container],
      onDismiss: () => this.cancel(),
    });
  }

  private handleKeyDown(e: KeyboardEvent): void {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        this.moveHighlight(1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        this.moveHighlight(-1);
        break;
      case 'Enter':
        // IME guard: a CJK composition commit fires Enter; don't act on it.
        if (e.isComposing) return;
        e.preventDefault();
        this.selectHighlighted();
        break;
      case 'Escape':
        e.preventDefault();
        this.cancel();
        break;
    }
  }

  private moveHighlight(delta: number): void {
    if (this.filteredPages.length === 0) return;

    this.highlightedIndex += delta;

    // Wrap around
    if (this.highlightedIndex < 0) {
      this.highlightedIndex = this.filteredPages.length - 1;
    } else if (this.highlightedIndex >= this.filteredPages.length) {
      this.highlightedIndex = 0;
    }

    this.updateHighlight();
  }

  private updateHighlight(): void {
    if (!this.dropdownElement) return;

    const items = this.dropdownElement.querySelectorAll('.moss-url-autocomplete-item');
    items.forEach((item, index) => {
      if (index === this.highlightedIndex) {
        item.classList.add('moss-url-autocomplete-item--highlighted');
        // Update aria-activedescendant
        if (this.inputElement) {
          this.inputElement.setAttribute('aria-activedescendant', item.id);
        }
        // Scroll into view if needed (optional chaining for test environments)
        item.scrollIntoView?.({ block: 'nearest' });
      } else {
        item.classList.remove('moss-url-autocomplete-item--highlighted');
      }
    });
  }

  private cancel(): void {
    if (this.pageLoad) this.pageLoad.submittedQuery = null;
    this.options.onCancel();
  }

  private selectHighlighted(): void {
    // Enter belongs to the current query even if the filesystem-backed page
    // list has not arrived yet. Editing or dismissing cancels that request.
    if (this.pageLoad) {
      this.pageLoad.submittedQuery = this.inputElement?.value ?? null;
      return;
    }
    if (this.filteredPages.length === 0) return;
    const item = this.filteredPages[this.highlightedIndex];
    if (item) {
      this.options.onSelect(item.page);
    }
  }

  private filterPages(query: string): void {
    const needle = stripSlashes(query.trim());
    const key = needle.toLowerCase();

    // Pages whose path starts with what was typed come first — the exact page,
    // then shallower before deeper — so "/" is home, "/posts" is the posts
    // index before its posts, and a partial "/po" already reaches it. The
    // backend lists pages sorted by path, which buries home after "about/" and
    // every other section. Fuzzy title matches follow.
    const rank = (p: PageWithDisplay) => (p.pathKey === key ? -1 : p.pathKey.split('/').length);
    const byPath = this.pages.filter((p) => p.pathKey.startsWith(key)).sort((a, b) => rank(a) - rank(b));
    const seen = new Set(byPath);
    this.filteredPages = [...byPath, ...this.fuzzySearch(needle).filter((p) => !seen.has(p))];

    // Reset highlight to first item
    this.highlightedIndex = 0;
    this.updateDropdown();
  }

  private fuzzySearch(needle: string): PageWithDisplay[] {
    if (!needle) return [];
    const [idxs, _info, order] = this.uf.search(this.haystack, needle);
    if (!idxs) return [];
    return order ? order.map((i) => this.pages[idxs[i]]) : idxs.map((i) => this.pages[i]);
  }

  private updateDropdown(): void {
    if (!this.dropdownElement || this.pageLoad) return;

    if (this.filteredPages.length === 0) {
      this.dropdownElement.innerHTML = `
        <div class="moss-url-autocomplete-empty">${t('preview.search_none')}</div>
      `;
      return;
    }

    const query = this.inputElement?.value.toLowerCase().trim() || '';

    this.dropdownElement.innerHTML = this.filteredPages
      .map((item, index) => {
        const isHighlighted = index === this.highlightedIndex;
        const itemId = `${this.listboxId}-item-${index}`;

        return `
          <div
            class="moss-url-autocomplete-item ${isHighlighted ? 'moss-url-autocomplete-item--highlighted' : ''}"
            role="option"
            id="${itemId}"
            aria-selected="${isHighlighted}"
            data-index="${index}"
          >
            <span class="moss-url-autocomplete-title">${this.highlightMatch(item.page.title, query)}</span>
            <span class="moss-url-autocomplete-path">${this.highlightMatch(item.displayPath, query)}</span>
          </div>
        `;
      })
      .join('');

    // Update aria-activedescendant
    if (this.inputElement && this.filteredPages.length > 0) {
      this.inputElement.setAttribute(
        'aria-activedescendant',
        `${this.listboxId}-item-${this.highlightedIndex}`
      );
    }

    // Setup item event listeners
    this.setupItemListeners();
  }

  private setupItemListeners(): void {
    if (!this.dropdownElement) return;

    const items = this.dropdownElement.querySelectorAll('.moss-url-autocomplete-item');
    items.forEach((item) => {
      item.addEventListener('mouseenter', () => {
        const index = parseInt(item.getAttribute('data-index') || '0', 10);
        this.highlightedIndex = index;
        this.updateHighlight();
      });

      item.addEventListener('click', () => {
        const index = parseInt(item.getAttribute('data-index') || '0', 10);
        const entry = this.filteredPages[index];
        if (entry) {
          this.options.onSelect(entry.page);
        }
      });
    });
  }

  private highlightMatch(text: string, query: string): string {
    if (!query) return this.escapeHtml(text);

    const lowerText = text.toLowerCase();
    const index = lowerText.indexOf(query);

    if (index === -1) return this.escapeHtml(text);

    const before = text.slice(0, index);
    const match = text.slice(index, index + query.length);
    const after = text.slice(index + query.length);

    return `${this.escapeHtml(before)}<span class="moss-url-autocomplete-match">${this.escapeHtml(match)}</span>${this.escapeHtml(after)}`;
  }

  private escapeHtml(text: string): string {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  public destroy(): void {
    this.pageLoad = null;
    this.disarmOutside?.();
    this.disarmOutside = null;
    this.container.innerHTML = '';
    this.inputElement = null;
    this.dropdownElement = null;
  }
}
