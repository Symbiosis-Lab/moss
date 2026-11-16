import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, dir, file } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath } = vi.hoisted(() => ({ mockListTree: vi.fn(), mockGetProjectPath: vi.fn() }));
vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { PreflightEvidence } from '../model';

describe('preflightCountForPath', () => {
  it('keeps duplicate source occurrences and derives enclosing folder counts', () => {
    const evidence = new PreflightEvidence();
    evidence.replace(['posts/a.md', 'posts/a.md', 'posts/nested/b.md', 'about.md']);
    expect(evidence.count('posts/a.md')).toBe(2);
    expect(evidence.count('posts')).toBe(3);
    expect(evidence.count('')).toBe(4);
  });

  it('does not confuse a sibling prefix for a descendant folder', () => {
    const evidence = new PreflightEvidence();
    evidence.replace(['posts-two/a.md']);
    expect(evidence.count('posts')).toBe(0);
  });
});

describe('where a row shows its count', () => {
  const ROOT = '/Users/me/Sites/garden';
  const PAGE = 'en/writing/case.md';
  let fileTree: FileTree;
  let container: HTMLElement;

  const seg = (path: string) => container.querySelector<HTMLElement>(`.seg[data-path="${path}"]`)!;
  const shown = (path: string) => seg(path).querySelector('.tree-preflight-count')?.textContent ?? null;
  const missing = (...sources: Array<[string, number]>) => ({
    build_generation: 1,
    missing_references: sources.flatMap(([source_path, n]) => Array.from({ length: n }, () => ({ source_path }))),
  }) as never;

  beforeEach(async () => {
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: dir(ROOT, [
      file(`${ROOT}/index.md`),
      dir(`${ROOT}/drafts`, [file(`${ROOT}/drafts/d.md`)]),
      dir(`${ROOT}/en`, [dir(`${ROOT}/en/writing`, [file(`${ROOT}/${PAGE}`), file(`${ROOT}/en/writing/other.md`)])]),
    ], { name: 'garden' }) });
    fileTree = new FileTree(new EntryRegistry());
    container = document.body.appendChild(document.createElement('div'));
    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/${PAGE}`);
    fileTree.setPublishPreflight(missing([PAGE, 7], ['drafts/d.md', 2]));
  });
  afterEach(() => { fileTree.unmount(); container.remove(); });

  it('numbers the open page once and no folder above it', () => {
    expect(shown(PAGE)).toBe('7');
    for (const folder of ['', 'en', 'en/writing']) {
      expect(shown(folder)).toBeNull();
      expect(seg(folder).dataset.preflightCount).toBeDefined();
    }
  });

  it('numbers a folder only while it is closed', () => {
    priv(fileTree).handleToggleTop(0);
    expect(shown('drafts')).toBe('2');
    (seg('drafts').querySelector('.chev') as HTMLElement).click();
    expect(shown('drafts')).toBeNull();
    expect(shown('drafts/d.md')).toBe('2');
    expect(seg('drafts/d.md').querySelector('button')).toBeNull();
  });

  it("the open page's number steps to the next problem", () => {
    const next = vi.fn();
    fileTree.onNextProblem = next;
    const button = seg(PAGE).querySelector<HTMLButtonElement>('button.tree-preflight-count')!;
    expect(button.getAttribute('aria-label')).toBe('7 unresolved references. Go to the next');
    button.click();
    expect(next).toHaveBeenCalledOnce();
  });
});
