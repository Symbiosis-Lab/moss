/**
 * URL Autocomplete Tests
 *
 * Tests for the UrlAutocomplete component that provides
 * address bar autocomplete functionality with page suggestions.
 */

import { vi, describe, it, expect, beforeEach, afterEach, Mock } from 'vitest';

// The page list comes from the engine; the test controls its answer.
vi.mock('../../engine', () => ({ engine: { getAllPages: vi.fn() } }));

import { UrlAutocomplete, UrlAutocompleteOptions } from '../url-autocomplete';
import { engine } from '../../engine';
import type { PageInfo } from '@symbiosis-lab/moss-ui';

describe('UrlAutocomplete', () => {
  let container: HTMLElement;
  let autocomplete: UrlAutocomplete | null;
  let onSelectMock: Mock;
  let onCancelMock: Mock;
  let defaultOptions: UrlAutocompleteOptions;

  // In get_all_pages' order: the backend sorts by url_path, so home
  // (index.html) lands after about/, articles/ and contact/, never first.
  const samplePages: PageInfo[] = [
    { url_path: 'index.html', title: 'Home' },
    { url_path: 'about/index.html', title: 'About Us' },
    { url_path: 'posts/index.html', title: 'Blog Posts' },
    { url_path: 'posts/hello-world/index.html', title: 'Hello World' },
    { url_path: 'posts/second-post/index.html', title: 'Second Post' },
    { url_path: 'contact/index.html', title: 'Contact' },
    { url_path: 'articles/割烹/index.html', title: '割烹料理' },
    { url_path: '文档/中文.html', title: '中文文档' },
    { url_path: 'posts/한국어-포스트/index.html', title: '한국어 포스트' },
    { url_path: 'articles/2025年の記事/index.html', title: '日本語の記事' },
  ].sort((a, b) => (a.url_path < b.url_path ? -1 : a.url_path > b.url_path ? 1 : 0));
  const homePage = samplePages.find((p) => p.url_path === 'index.html')!;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();

    // Create container
    container = document.createElement('div');
    document.body.appendChild(container);

    // Default mock - return sample pages
    (engine.getAllPages as Mock).mockResolvedValue({
      status: 'ok',
      data: samplePages,
    });

    onSelectMock = vi.fn();
    onCancelMock = vi.fn();

    defaultOptions = {
      onSelect: onSelectMock,
      onCancel: onCancelMock,
      initialValue: '',
    };
  });

  afterEach(() => {
    autocomplete?.destroy();
    autocomplete = null;
    container.remove();
    vi.useRealTimers();
  });

  describe('Initialization', () => {
    it('should render the autocomplete input and dropdown', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      expect(container.querySelector('.moss-url-autocomplete')).toBeTruthy();
      expect(container.querySelector('.moss-url-autocomplete-input')).toBeTruthy();
      expect(container.querySelector('.moss-url-autocomplete-dropdown')).toBeTruthy();
    });

    it('should load pages on initialization', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      expect(engine.getAllPages).toHaveBeenCalledTimes(1);
    });

    it('should set initial value in input', async () => {
      autocomplete = new UrlAutocomplete(container, {
        ...defaultOptions,
        initialValue: '/about',
      });
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      expect(input.value).toBe('/about');
    });

    it('should disable autocomplete, autocorrect, autocapitalize, and spellcheck on input', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;

      expect(input.getAttribute('autocomplete')).toBe('off');
      expect(input.getAttribute('autocorrect')).toBe('off');
      expect(input.getAttribute('autocapitalize')).toBe('off');
      expect(input.getAttribute('spellcheck')).toBe('false');
    });

    it('should focus input on initialization', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      expect(document.activeElement).toBe(input);
    });
  });

  describe('Filtering', () => {
    it('should show all pages when input is empty', async () => {
      autocomplete = new UrlAutocomplete(container, {
        ...defaultOptions,
        initialValue: '',
      });
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBe(samplePages.length);
    });

    it('should filter pages by title substring', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = 'post';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      // Should match "Blog Posts", "Hello World" (in posts), "Second Post"
      expect(items.length).toBeGreaterThan(0);
      expect(items.length).toBeLessThan(samplePages.length);
    });

    it('should filter pages by path substring', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = 'about';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBe(1);
    });

    it('should be case-insensitive', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = 'ABOUT';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBe(1);
    });

    it('should match when query has leading slash', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      // Type "/about" — should match since displayPath is "/about/"
      input.value = '/about';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBe(1);
    });

    it('should support fuzzy matching by title substring', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      // Type "hello" — should find "Hello World" page by title
      input.value = 'hello';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);

      // The "Hello World" page should be in the results
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('Hello World'))).toBe(true);
    });

    it('should support fuzzy subsequence matching (not just substring)', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      // Type "helo" — missing an 'l', should still fuzzy-match "Hello World"
      // This would fail with String.includes() but succeeds with uFuzzy
      input.value = 'helo';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);

      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('Hello World'))).toBe(true);
    });

    it('should show empty state when no matches', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = 'nonexistent';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const emptyState = container.querySelector('.moss-url-autocomplete-empty');
      expect(emptyState).toBeTruthy();
    });

    it('should list home first for "/", shallow pages before nested ones, and go home on Enter', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '/';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const paths = Array.from(container.querySelectorAll('.moss-url-autocomplete-path')).map(
        (el) => el.textContent ?? ''
      );
      expect(paths.length).toBe(samplePages.length);
      expect(paths[0]).toBe('/');
      const depths = paths.map((p) => p.split('/').filter(Boolean).length);
      expect(depths).toEqual([...depths].sort((a, b) => a - b));

      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
      expect(onSelectMock).toHaveBeenCalledWith(homePage);
    });

    it('should rank the page at a typed path above pages whose title matches it', async () => {
      const story = { url_path: 'story/index.html', title: '我們的故事' };
      (engine.getAllPages as Mock).mockResolvedValue({
        status: 'ok',
        data: [{ url_path: 'archive/story-of-a-year/index.html', title: 'Story of a year' }, story],
      });
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '/story';
      input.dispatchEvent(new Event('input'));
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));

      expect(onSelectMock).toHaveBeenCalledWith(story);
    });

    it('should match with trailing slash in query', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '/about/';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBe(1);
    });
  });

  describe('CJK Filtering', () => {
    it('should match Chinese characters in path', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '割';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('割烹料理'))).toBe(true);
    });

    it('should match Chinese characters in title', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '中文';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('中文文档'))).toBe(true);
    });

    it('should match CJK characters with leading slash', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '/割';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('割烹料理'))).toBe(true);
    });

    it('should match Korean characters', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '한국어';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('한국어 포스트'))).toBe(true);
    });

    it('should match Japanese characters', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '記事';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
      const titles = Array.from(items).map(
        (item) => item.querySelector('.moss-url-autocomplete-title')?.textContent
      );
      expect(titles.some((t) => t?.includes('日本語の記事'))).toBe(true);
    });

    it('should match mixed CJK and ASCII', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = '2025年';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items.length).toBeGreaterThanOrEqual(1);
    });
  });

  describe('Keyboard Navigation', () => {
    it('should highlight first item initially', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const highlighted = container.querySelector('.moss-url-autocomplete-item--highlighted');
      expect(highlighted).toBeTruthy();
    });

    it('should move highlight down with ArrowDown', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      const items = container.querySelectorAll('.moss-url-autocomplete-item');

      // First item should be highlighted initially
      expect(items[0].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(true);

      // Press ArrowDown
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
      await vi.runAllTimersAsync();

      // Second item should be highlighted
      expect(items[0].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(false);
      expect(items[1].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(true);
    });

    it('should move highlight up with ArrowUp', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;

      // Move down twice first
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
      await vi.runAllTimersAsync();

      // Now move up
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp' }));
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items[1].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(true);
    });

    it('should wrap around at bottom', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;

      // Move down past the last item
      for (let i = 0; i < samplePages.length; i++) {
        input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
      }
      await vi.runAllTimersAsync();

      // Should wrap to first item
      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      expect(items[0].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(true);
    });

    it('should select highlighted item on Enter', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
      await vi.runAllTimersAsync();

      expect(onSelectMock).toHaveBeenCalledTimes(1);
      expect(onSelectMock).toHaveBeenCalledWith(homePage);
    });

    it('should cancel on Escape', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
      await vi.runAllTimersAsync();

      expect(onCancelMock).toHaveBeenCalledTimes(1);
    });
  });

  describe('Mouse Interaction', () => {
    it('should highlight item on hover', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      items[2].dispatchEvent(new MouseEvent('mouseenter'));
      await vi.runAllTimersAsync();

      expect(items[2].classList.contains('moss-url-autocomplete-item--highlighted')).toBe(true);
    });

    it('should select item on click', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      const title = items[1].querySelector('.moss-url-autocomplete-title')?.textContent;
      items[1].dispatchEvent(new MouseEvent('click'));
      await vi.runAllTimersAsync();

      expect(onSelectMock).toHaveBeenCalledTimes(1);
      expect(onSelectMock).toHaveBeenCalledWith(samplePages.find((p) => p.title === title));
    });
  });

  describe('Display', () => {
    it('should display page title and display path (not raw url_path)', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const firstItem = container.querySelector('.moss-url-autocomplete-item');
      expect(firstItem?.textContent).toContain('Home');
      // Should show display path "/" (from urlPathToDisplayPath("index.html"))
      // NOT the raw "index.html"
      const pathEl = firstItem?.querySelector('.moss-url-autocomplete-path');
      expect(pathEl?.textContent).toContain('/');
      expect(pathEl?.textContent).not.toContain('index.html');
    });

    it('should highlight matched text in results', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      input.value = 'about';
      input.dispatchEvent(new Event('input'));
      await vi.runAllTimersAsync();

      const highlight = container.querySelector('.moss-url-autocomplete-match');
      expect(highlight).toBeTruthy();
    });
  });

  describe('Accessibility', () => {
    it('should have proper ARIA attributes on input', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      expect(input.getAttribute('role')).toBe('combobox');
      expect(input.getAttribute('aria-expanded')).toBe('true');
      expect(input.getAttribute('aria-autocomplete')).toBe('list');
      expect(input.getAttribute('aria-controls')).toBeTruthy();
    });

    it('should have proper ARIA attributes on listbox', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const dropdown = container.querySelector('.moss-url-autocomplete-dropdown');
      expect(dropdown?.getAttribute('role')).toBe('listbox');
    });

    it('should have proper ARIA attributes on items', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      items.forEach((item) => {
        expect(item.getAttribute('role')).toBe('option');
      });
    });

    it('should update aria-activedescendant on navigation', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      const input = container.querySelector('.moss-url-autocomplete-input') as HTMLInputElement;
      const initialActiveDescendant = input.getAttribute('aria-activedescendant');

      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
      await vi.runAllTimersAsync();

      const newActiveDescendant = input.getAttribute('aria-activedescendant');
      expect(newActiveDescendant).not.toBe(initialActiveDescendant);
    });
  });

  describe('Error Handling', () => {
    it('should handle API error gracefully', async () => {
      (engine.getAllPages as Mock).mockResolvedValue({
        status: 'error',
        error: 'Failed to load pages',
      });

      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Should still render without crashing
      expect(container.querySelector('.moss-url-autocomplete')).toBeTruthy();
      // Should show empty state or error message
      const dropdown = container.querySelector('.moss-url-autocomplete-dropdown');
      expect(dropdown).toBeTruthy();
    });
  });

  describe('Click outside to dismiss', () => {
    it('should call onCancel when pressing outside the autocomplete', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // The shared dismisser closes on the press, not the release.
      document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
      await vi.runAllTimersAsync();

      expect(onCancelMock).toHaveBeenCalledTimes(1);
    });

    it('should NOT call onCancel when clicking inside the autocomplete', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Click inside the container
      container.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await vi.runAllTimersAsync();

      expect(onCancelMock).not.toHaveBeenCalled();
    });

    it('should NOT call onCancel when clicking on an autocomplete item', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Click on an autocomplete item (should trigger select, not cancel)
      const items = container.querySelectorAll('.moss-url-autocomplete-item');
      items[0].dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await vi.runAllTimersAsync();

      // Should call onSelect, not onCancel
      expect(onSelectMock).toHaveBeenCalled();
      expect(onCancelMock).not.toHaveBeenCalled();
    });

    it('should call onCancel when window loses focus (e.g., iframe click)', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Simulate window losing focus (happens when user clicks on iframe)
      window.dispatchEvent(new Event('blur'));
      await vi.runAllTimersAsync();

      expect(onCancelMock).toHaveBeenCalledTimes(1);
    });

    it('should remove window blur listener on destroy', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Destroy the component
      autocomplete.destroy();
      autocomplete = null;

      // Window blur should NOT call onCancel after destroy
      window.dispatchEvent(new Event('blur'));
      await vi.runAllTimersAsync();

      expect(onCancelMock).not.toHaveBeenCalled();
    });
  });

  describe('submitting while the page list is loading', () => {
    const places = { url_path: 'places/index.html', title: '地方' };
    const placePages = [
      { url_path: 'places/上海/index.html', title: 'Places' },
      { url_path: 'places/北京/index.html', title: '北京' },
      places,
    ];
    let complete: (result: { status: 'ok'; data: PageInfo[] }) => void;

    function submitBeforePagesLoad(query: string): HTMLInputElement {
      (engine.getAllPages as Mock).mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      const input = container.querySelector<HTMLInputElement>('input')!;
      input.value = query;
      input.dispatchEvent(new Event('input'));
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      expect(onSelectMock).not.toHaveBeenCalled();
      return input;
    }

    it('keeps loading feedback until the page list arrives', () => {
      submitBeforePagesLoad('places');
      expect(container.querySelector('.moss-url-autocomplete-loading')).not.toBeNull();
    });

    it.each(['places', '/places/'])('follows %s when the pending page list arrives', async (query) => {
      submitBeforePagesLoad(query);
      complete({ status: 'ok', data: placePages });
      await vi.runAllTimersAsync();
      expect(onSelectMock).toHaveBeenCalledExactlyOnceWith(places);
    });

    it.each(['Escape', 'edit', 'outside', 'destroy'])('does not follow a pending submission after %s', async (action) => {
      const input = submitBeforePagesLoad('places');
      await vi.runAllTimersAsync();
      if (action === 'Escape') input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      else if (action === 'edit') { input.value = 'about'; input.dispatchEvent(new Event('input')); }
      else if (action === 'outside') document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
      else autocomplete!.destroy();
      complete({ status: 'ok', data: [places] });
      await vi.runAllTimersAsync();
      expect(onSelectMock).not.toHaveBeenCalled();
    });
  });

  describe('Cleanup', () => {
    it('should remove all elements on destroy', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      autocomplete.destroy();
      autocomplete = null;

      expect(container.querySelector('.moss-url-autocomplete')).toBeNull();
    });

    it('should remove document click listener on destroy', async () => {
      autocomplete = new UrlAutocomplete(container, defaultOptions);
      await vi.runAllTimersAsync();

      // Destroy the component
      autocomplete.destroy();
      autocomplete = null;

      // Click outside should NOT call onCancel after destroy
      document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await vi.runAllTimersAsync();

      expect(onCancelMock).not.toHaveBeenCalled();
    });
  });
});
