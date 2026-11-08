/**
 * Tests for editor-skeleton.ts — the cold-start skeleton dismissers.
 *
 * editor.html ships static skeleton markup inside #nav-grid and
 * #moss-editor-cm so the editor webview paints before any JS executes.
 * These dismissers are tiny pure-DOM removers; the structural-removal
 * story (every content mount path clears #moss-editor-cm) is covered by
 * the editor-main boot tests.
 */

import { describe, test, expect, beforeEach } from 'vitest';
import {
  dismissNavSkeleton,
  dismissContentSkeleton,
  dismissEditorSkeleton,
} from '../editor-skeleton';

/** Mirrors the skeleton markup editor.html ships inside the real containers. */
function setupSkeletonDOM(): void {
  document.body.innerHTML = `
    <div id="nav-grid">
      <div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div>
    </div>
    <div id="moss-editor-cm">
      <div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true">
        <div class="moss-editor-skeleton__line moss-editor-skeleton__line--heading"></div>
        <div class="moss-editor-skeleton__line"></div>
      </div>
    </div>
  `;
}

describe('editor cold-start skeleton dismissers', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  test('dismissNavSkeleton removes #moss-nav-skeleton only, and is idempotent', () => {
    setupSkeletonDOM();
    dismissNavSkeleton();
    expect(document.getElementById('moss-nav-skeleton')).toBeNull();
    // The content skeleton is untouched (its removal is structural).
    expect(document.getElementById('moss-editor-skeleton')).not.toBeNull();
    // Idempotent: a second call on an already-removed node must not throw.
    expect(() => dismissNavSkeleton()).not.toThrow();
  });

  test('dismissContentSkeleton removes #moss-editor-skeleton only, and is idempotent', () => {
    setupSkeletonDOM();
    dismissContentSkeleton();
    expect(document.getElementById('moss-editor-skeleton')).toBeNull();
    expect(document.getElementById('moss-nav-skeleton')).not.toBeNull();
    expect(() => dismissContentSkeleton()).not.toThrow();
  });

  test('dismissEditorSkeleton removes both skeleton nodes', () => {
    setupSkeletonDOM();
    dismissEditorSkeleton();
    expect(document.getElementById('moss-nav-skeleton')).toBeNull();
    expect(document.getElementById('moss-editor-skeleton')).toBeNull();
  });

  test('all dismissers no-throw on a document without skeleton nodes', () => {
    expect(() => dismissNavSkeleton()).not.toThrow();
    expect(() => dismissContentSkeleton()).not.toThrow();
    expect(() => dismissEditorSkeleton()).not.toThrow();
  });
});
