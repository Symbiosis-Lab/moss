/**
 * The rename/move toast message builder (plural/page-name variants, all
 * three locales) and the Undo wiring `showRenameSummaryToast` sets up.
 */
import { describe, test, expect, vi, afterEach } from 'vitest';
import { setLocale } from "../../../i18n/index";
import {
  buildRenameToastMessage,
  showRenameSummaryToast,
  mapMovedPath,
  type RenameToastSummary,
} from '../rename-toast';

const { mockShowAck } = vi.hoisted(() => ({ mockShowAck: vi.fn() }));
vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showAck: mockShowAck,
  showToast: vi.fn(),
}));

afterEach(() => {
  setLocale('en');
  mockShowAck.mockClear();
});

function summary(overrides: Partial<RenameToastSummary> = {}): RenameToastSummary {
  return {
    moved: [],
    edited_link_count: 1,
    edited_pages: ['other.md'],
    skipped_pages: [],
    undo_id: 'undo-1',
    ...overrides,
  };
}

describe('buildRenameToastMessage', () => {
  test('returns null when nothing was edited (no toast, same as today)', () => {
    expect(
      buildRenameToastMessage({ kind: 'rename', oldName: 'a', newName: 'b' }, summary({ edited_link_count: 0 })),
    ).toBeNull();
  });

  describe('rename: one page vs many pages', () => {
    test('en', () => {
      const one = buildRenameToastMessage(
        { kind: 'rename', oldName: 'draft', newName: 'post' },
        summary({ edited_link_count: 1, edited_pages: ['index.md'] }),
      );
      expect(one).toBe('Renamed "draft" to "post" and updated 1 link on index.md.');

      const many = buildRenameToastMessage(
        { kind: 'rename', oldName: 'draft', newName: 'post' },
        summary({ edited_link_count: 3, edited_pages: ['index.md', 'about.md'] }),
      );
      expect(many).toBe('Renamed "draft" to "post" and updated 3 links on 2 pages.');
    });

    test('zh-hans', () => {
      setLocale('zh-hans');
      const one = buildRenameToastMessage(
        { kind: 'rename', oldName: '草稿', newName: '文章' },
        summary({ edited_link_count: 1, edited_pages: ['index.md'] }),
      );
      expect(one).toBe('已将"草稿"重命名为"文章"，并更新了"index.md"中的 1 个链接。');

      const many = buildRenameToastMessage(
        { kind: 'rename', oldName: '草稿', newName: '文章' },
        summary({ edited_link_count: 3, edited_pages: ['index.md', 'about.md'] }),
      );
      expect(many).toBe('已将"草稿"重命名为"文章"，并更新了 2 个页面中的 3 个链接。');
    });

    test('zh-hant', () => {
      setLocale('zh-hant');
      const one = buildRenameToastMessage(
        { kind: 'rename', oldName: '草稿', newName: '文章' },
        summary({ edited_link_count: 1, edited_pages: ['index.md'] }),
      );
      expect(one).toBe('已將「草稿」重新命名為「文章」，並更新了「index.md」中的 1 個連結。');

      const many = buildRenameToastMessage(
        { kind: 'rename', oldName: '草稿', newName: '文章' },
        summary({ edited_link_count: 3, edited_pages: ['index.md', 'about.md'] }),
      );
      expect(many).toBe('已將「草稿」重新命名為「文章」，並更新了 2 個頁面中的 3 個連結。');
    });
  });

  describe('move: single item, one page vs many pages', () => {
    test('en', () => {
      const one = buildRenameToastMessage(
        { kind: 'move', itemName: 'photo.jpg' },
        summary({ edited_link_count: 1, edited_pages: ['index.md'] }),
      );
      expect(one).toBe('Moved "photo.jpg" and updated 1 link on index.md.');

      const many = buildRenameToastMessage(
        { kind: 'move', itemName: 'photo.jpg' },
        summary({ edited_link_count: 2, edited_pages: ['index.md', 'about.md'] }),
      );
      expect(many).toBe('Moved "photo.jpg" and updated 2 links on 2 pages.');
    });

    test('zh-hant', () => {
      setLocale('zh-hant');
      const one = buildRenameToastMessage(
        { kind: 'move', itemName: '相片.jpg' },
        summary({ edited_link_count: 1, edited_pages: ['index.md'] }),
      );
      expect(one).toBe('已移動「相片.jpg」，並更新了「index.md」中的 1 個連結。');
    });
  });

  describe('move: batch', () => {
    test('en', () => {
      const many = buildRenameToastMessage(
        { kind: 'move-batch', itemCount: 3 },
        summary({ edited_link_count: 5, edited_pages: ['a.md', 'b.md'] }),
      );
      expect(many).toBe('Moved 3 items and updated 5 links on 2 pages.');
    });

    test('zh-hans', () => {
      setLocale('zh-hans');
      const many = buildRenameToastMessage(
        { kind: 'move-batch', itemCount: 3 },
        summary({ edited_link_count: 5, edited_pages: ['a.md', 'b.md'] }),
      );
      expect(many).toBe('已移动 3 个项目，并更新了 2 个页面中的 5 个链接。');
    });
  });

  describe('skipped pages appended to the same toast', () => {
    test('en, singular and plural', () => {
      const singular = buildRenameToastMessage(
        { kind: 'rename', oldName: 'a', newName: 'b' },
        summary({ skipped_pages: ['stale.md'] }),
      );
      expect(singular).toBe(
        'Renamed "a" to "b" and updated 1 link on other.md. 1 page changed meanwhile and were left as is: stale.md.',
      );

      const plural = buildRenameToastMessage(
        { kind: 'rename', oldName: 'a', newName: 'b' },
        summary({ skipped_pages: ['stale.md', 'other-stale.md'] }),
      );
      expect(plural).toContain('2 pages changed meanwhile and were left as is: stale.md, other-stale.md.');
    });

    test('zh-hans', () => {
      setLocale('zh-hans');
      const withSkipped = buildRenameToastMessage(
        { kind: 'rename', oldName: 'a', newName: 'b' },
        summary({ skipped_pages: ['stale.md'] }),
      );
      expect(withSkipped).toContain('1 个页面在此期间有改动，已保持原样：stale.md。');
    });
  });
});

describe('showRenameSummaryToast', () => {
  test('shows nothing when no link was edited', () => {
    showRenameSummaryToast(
      { kind: 'rename', oldName: 'a', newName: 'b' },
      summary({ edited_link_count: 0 }),
      async () => {},
    );
    expect(mockShowAck).not.toHaveBeenCalled();
  });

  test('shows the toast with an 8s duration and an Undo action', () => {
    showRenameSummaryToast({ kind: 'rename', oldName: 'a', newName: 'b' }, summary(), async () => {});
    expect(mockShowAck).toHaveBeenCalledTimes(1);
    const [message, options] = mockShowAck.mock.calls[0];
    expect(message).toContain('Renamed');
    expect(options.duration).toBe(8000);
    expect(options.actions).toHaveLength(1);
    expect(options.actions[0].label).toBe('Undo');
  });

  test('the Undo action invokes the caller-supplied onUndo', async () => {
    const onUndo = vi.fn().mockResolvedValue(undefined);
    showRenameSummaryToast({ kind: 'rename', oldName: 'a', newName: 'b' }, summary(), onUndo);
    const [, options] = mockShowAck.mock.calls[0];
    options.actions[0].onClick();
    // runAction's work runs in a microtask past the click.
    await Promise.resolve();
    await Promise.resolve();
    expect(onUndo).toHaveBeenCalledTimes(1);
  });
});

describe('mapMovedPath', () => {
  const moved = [
    { old_path: 'posts', new_path: 'archive/posts', is_dir: true },
    { old_path: 'notes/a.md', new_path: 'notes/a-2.md', is_dir: false },
  ];

  test('an exact hit wins over a directory-prefix hit', () => {
    // "posts" itself is a dir entry; a file-level exact match for a path
    // that also happens to sit under a moved dir must prefer the exact one.
    const exactMoved = [
      ...moved,
      { old_path: 'posts/index.md', new_path: 'archive/posts/renamed-index.md', is_dir: false },
    ];
    expect(mapMovedPath('posts/index.md', exactMoved)).toBe('archive/posts/renamed-index.md');
  });

  test('a folder-prefix hit remaps a descendant that has no exact entry', () => {
    expect(mapMovedPath('posts/child/page.md', moved)).toBe('archive/posts/child/page.md');
  });

  test('an unrelated path is returned unchanged', () => {
    expect(mapMovedPath('unrelated.md', moved)).toBe('unrelated.md');
  });
});
