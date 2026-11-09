import { describe, test, expect, vi } from 'vitest';
import { toAbsolute, toRelative, normalizePath, siblingPath, childPath, resolveCreateTarget, PROJECT_ROOT_REL, isMossInternalPath, displayName, shortenKnownPaths } from '../path-domain';

describe('path-domain helpers', () => {
  describe('isMossInternalPath', () => {
    test('.moss and its descendants, in either slash form; never a name that merely contains .moss', () => {
      expect(isMossInternalPath('.moss')).toBe(true);
      expect(isMossInternalPath('.moss/config.toml')).toBe(true);
      expect(isMossInternalPath('.moss\\theme\\grain.png')).toBe(true);
      expect(isMossInternalPath('notes/.mossy-draft.md')).toBe(false);
      expect(isMossInternalPath('my.moss.md')).toBe(false);
    });
  });

  describe('resolveCreateTarget', () => {
    test('directory anchor is its own create target', () => {
      expect(resolveCreateTarget('posts', true)).toBe('posts');
      expect(resolveCreateTarget('posts/essays', true)).toBe('posts/essays');
    });

    test('file anchor resolves to its parent directory', () => {
      expect(resolveCreateTarget('posts/2026-08-02-on-gardens.md', false)).toBe('posts');
      expect(resolveCreateTarget('a/b/c/note.md', false)).toBe('a/b/c');
    });

    test('root-level file anchor resolves to the project root', () => {
      expect(resolveCreateTarget('about.md', false)).toBe(PROJECT_ROOT_REL);
    });

    test('the project root anchor (empty string, isDir) stays the root', () => {
      expect(resolveCreateTarget(PROJECT_ROOT_REL, true)).toBe(PROJECT_ROOT_REL);
    });
  });

  describe('siblingPath', () => {
    test('replaces the basename of a nested file, preserving the parent dir', () => {
      expect(siblingPath('works/post.md', 'renamed.md')).toBe('works/renamed.md');
    });

    test('root-level file returns just the basename — NO leading slash', () => {
      // Regression: `parentDir + "/" + name` produced "/renamed.md" for root
      // files, which toAbsolute treats as already-absolute → outside project →
      // "Paths must be within the project directory".
      expect(siblingPath('plain-article.md', 'renamed.md')).toBe('renamed.md');
      expect(siblingPath('plain-article.md', 'renamed.md').startsWith('/')).toBe(false);
    });

    test('deeply nested path keeps the full parent dir', () => {
      expect(siblingPath('a/b/c/old.md', 'new.md')).toBe('a/b/c/new.md');
    });

    test('round-trips through toAbsolute for a root file (stays within project)', () => {
      const rel = siblingPath('plain-article.md', 'renamed.md');
      expect(toAbsolute(rel, '/Users/me/site')).toBe('/Users/me/site/renamed.md');
    });
  });

  describe('toAbsolute', () => {
    test('prepends projectRoot to a relative path', () => {
      expect(toAbsolute('index.md', '/Users/me/Sites/blog')).toBe('/Users/me/Sites/blog/index.md');
    });

    test('handles nested relative paths', () => {
      expect(toAbsolute('works/post.md', '/Users/me/Sites/blog'))
        .toBe('/Users/me/Sites/blog/works/post.md');
    });

    test('strips a trailing slash on projectRoot', () => {
      expect(toAbsolute('index.md', '/Users/me/Sites/blog/'))
        .toBe('/Users/me/Sites/blog/index.md');
    });

    test('returns input unchanged if it is already absolute', () => {
      const abs = '/Users/me/Sites/blog/index.md';
      expect(toAbsolute(abs, '/Users/me/Sites/blog')).toBe(abs);
    });

    test('handles paths with spaces (e.g. "Mira Vale.md")', () => {
      expect(toAbsolute('Mira Vale.md', '/Users/me/Sites/Mira Vale'))
        .toBe('/Users/me/Sites/Mira Vale/Mira Vale.md');
    });
  });

  describe('toRelative', () => {
    test('strips projectRoot prefix from an absolute path', () => {
      expect(toRelative('/Users/me/Sites/blog/index.md', '/Users/me/Sites/blog'))
        .toBe('index.md');
    });

    test('handles nested absolute paths', () => {
      expect(toRelative('/Users/me/Sites/blog/works/post.md', '/Users/me/Sites/blog'))
        .toBe('works/post.md');
    });

    test('strips a trailing slash on projectRoot', () => {
      expect(toRelative('/Users/me/Sites/blog/index.md', '/Users/me/Sites/blog/'))
        .toBe('index.md');
    });

    test('returns input unchanged if it is already relative (no leading slash)', () => {
      expect(toRelative('index.md', '/Users/me/Sites/blog')).toBe('index.md');
    });

    test('handles paths with spaces', () => {
      expect(toRelative('/Users/me/Sites/Mira Vale/Mira Vale.md', '/Users/me/Sites/Mira Vale'))
        .toBe('Mira Vale.md');
    });

    test('warns and passes through an absolute path outside projectRoot', () => {
      // Silent pass-through would mask wire-format bugs (e.g., watcher
      // accidentally emitting /etc/hosts). Warn but don't throw —
      // throwing would crash the rename pipeline on a bad event we'd
      // rather log and continue past.
      const spy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      try {
        const outside = '/etc/hosts';
        expect(toRelative(outside, '/Users/me/Sites/blog')).toBe(outside);
        expect(spy).toHaveBeenCalledOnce();
        expect(spy.mock.calls[0][0]).toMatch(/outside projectRoot/);
      } finally {
        spy.mockRestore();
      }
    });
  });

  describe('childPath', () => {
    test('joins a basename under a nested dir', () => {
      expect(childPath('works/2026', 'post.md')).toBe('works/2026/post.md');
    });

    test('project root ("") returns just the basename — NO leading slash', () => {
      // The naive `dir + '/' + name` minted '/untitled.md' at the root, which
      // toAbsolute treats as already absolute (outside the project).
      expect(childPath('', 'untitled.md')).toBe('untitled.md');
      expect(childPath('', 'untitled.md').startsWith('/')).toBe(false);
    });
  });

  describe('defensive empty-root behavior', () => {
    test('toAbsolute returns input unchanged when projectRoot is empty', () => {
      expect(toAbsolute('index.md', '')).toBe('index.md');
    });

    test('toRelative returns input unchanged when projectRoot is empty', () => {
      // No root to strip — pass through silently (no warn — empty root is
      // a caller-state issue, not a path-shape issue).
      expect(toRelative('/Users/me/Sites/blog/index.md', '')).toBe('/Users/me/Sites/blog/index.md');
    });
  });

  /**
   * The rule these helpers must NOT break: a path is carried byte-for-byte,
   * and Unicode form is settled only where two paths are COMPARED
   * (normalizePath, used at the three comparison sites in file-tree.ts).
   *
   * Normalizing here instead is the tempting version and it is unsafe: every
   * filesystem call derives from these outputs, and Linux filesystems are
   * normalization-sensitive — a decomposed filename would be shown and then
   * fail to open, rename or delete. macOS hides that, so only an assertion
   * can hold the line.
   */
  describe('Unicode form is preserved, never rewritten', () => {
    const NFC = 'caf\u00e9';   // precomposed: caf + é
    const NFD = 'cafe\u0301';  // decomposed: caf + e + combining acute
    const root = '/Users/me/Sites/blog';

    test('the two spellings are different strings that mean one name', () => {
      // Guard: with a name that has no decomposed form (any CJK example) every
      // assertion below would hold whether or not the code rewrites anything.
      expect(NFC).not.toBe(NFD);
      expect(NFD.normalize('NFC')).toBe(NFC);
      expect(NFD.length).not.toBe(NFC.length);
    });

    test('siblingPath keeps the typed spelling — this is what reaches the filesystem', () => {
      // siblingPath's output becomes the argument to renameEntryWithRefs. On
      // ext4 the bytes ARE the filename.
      expect(siblingPath('posts/old.md', `${NFD}.md`)).toBe(`posts/${NFD}.md`);
      expect(siblingPath('posts/old.md', `${NFC}.md`)).toBe(`posts/${NFC}.md`);
      expect(siblingPath('old.md', `${NFD}.md`)).not.toBe(`${NFC}.md`);
    });

    test('childPath keeps the typed spelling too', () => {
      expect(childPath('posts', `${NFD}.md`)).toBe(`posts/${NFD}.md`);
      expect(childPath(PROJECT_ROOT_REL, `${NFD}.md`)).toBe(`${NFD}.md`);
      expect(childPath('posts', `${NFD}.md`)).not.toBe(`posts/${NFC}.md`);
    });

    test('toRelative hands back the filesystem\'s own spelling', () => {
      expect(toRelative(`${root}/${NFD}/a.md`, root)).toBe(`${NFD}/a.md`);
      expect(toRelative(`${root}/${NFD}/a.md`, root)).not.toBe(`${NFC}/a.md`);
    });

    test('toAbsolute hands it back unchanged as well', () => {
      expect(toAbsolute(`${NFD}/a.md`, root)).toBe(`${root}/${NFD}/a.md`);
    });

    test('normalizePath is the comparison form, and only that', () => {
      expect(normalizePath(NFD)).toBe(NFC);
      expect(normalizePath(NFC)).toBe(NFC);
      // Idempotent, and it touches nothing else about the path.
      expect(normalizePath('posts/a.md')).toBe('posts/a.md');
    });
  });

  describe('Windows drive-letter absolute paths', () => {
    // Regression: a `C:\...` sourcePath from the Rust side has no leading
    // `/`, so toRelative used to treat it as "already relative" and pass it
    // through unchanged; the caller's later toAbsolute call then read that
    // same absolute string as relative and prepended projectRoot AGAIN,
    // producing `<root>/<root>\<file>` — a path that does not exist, which
    // is what made parseFrontmatter fail and the editor panel stay on the
    // pre-warmed "generated" surface instead of the real article.
    const root = 'C:\\Users\\moss\\sites\\parity-cjk-site';

    test('toAbsolute treats a drive-letter path as already absolute', () => {
      const abs = `${root}\\纽约诸法门.md`;
      expect(toAbsolute(abs, root)).toBe(abs);
    });

    test('toAbsolute treats a forward-slash drive-letter path as already absolute too', () => {
      const abs = 'C:/Users/moss/sites/parity-cjk-site/index.md';
      expect(toAbsolute(abs, 'C:/Users/moss/sites/parity-cjk-site')).toBe(abs);
    });

    test('toRelative strips a backslash-joined projectRoot prefix', () => {
      expect(toRelative(`${root}\\纽约诸法门.md`, root)).toBe('纽约诸法门.md');
    });

    test('toRelative strips a nested backslash-joined path', () => {
      expect(toRelative(`${root}\\隨筆\\散步紀錄.md`, root)).toBe('隨筆\\散步紀錄.md');
    });

    test('toRelative maps the project root itself to PROJECT_ROOT_REL', () => {
      expect(toRelative(root, root)).toBe(PROJECT_ROOT_REL);
    });

    test('round-trips: toAbsolute(toRelative(x)) names the same file', () => {
      // toAbsolute's join is always '/' (the internal relative-path
      // convention — see childPath/siblingPath), so a backslash-rooted
      // Windows path comes back with a mixed separator at that one join:
      // `<root>/<file>` rather than `<root>\<file>`. That's not a defect —
      // a prior investigation's native Windows binary test confirmed
      // Windows/NTFS resolves a mixed '/'/'\' non-verbatim path exactly like
      // an all-backslash one — so this asserts the logical path survives,
      // not the exact separator bytes.
      const abs = `${root}\\纽约诸法门.md`;
      const roundTripped = toAbsolute(toRelative(abs, root), root);
      expect(roundTripped.replace(/\\/g, '/')).toBe(abs.replace(/\\/g, '/'));
    });
  });

  describe('round trips', () => {
    test('toAbsolute then toRelative returns the original relative path', () => {
      const rel = 'works/2026/post.md';
      const root = '/Users/me/Sites/blog';
      expect(toRelative(toAbsolute(rel, root), root)).toBe(rel);
    });

    test('toRelative then toAbsolute returns the original absolute path', () => {
      const abs = '/Users/me/Sites/blog/works/post.md';
      const root = '/Users/me/Sites/blog';
      expect(toAbsolute(toRelative(abs, root), root)).toBe(abs);
    });
  });

  describe('displayName', () => {
    test('a nested path shows its last segment', () => {
      expect(displayName('posts/2026/hello.md', '/Users/me/blog')).toBe('hello.md');
      expect(displayName('posts', '/Users/me/blog')).toBe('posts');
    });

    test('the project root shows the folder name on disk, trailing slash or not', () => {
      expect(displayName(PROJECT_ROOT_REL, '/Users/me/blog')).toBe('blog');
      expect(displayName(PROJECT_ROOT_REL, '/Users/me/blog/')).toBe('blog');
    });
  });

  describe('shortenKnownPaths', () => {
    test('replaces a known absolute path with its basename', () => {
      const abs = "/Users/liuguo/Library/Mobile Documents/com~apple~CloudDocs/site/文稿/测试.md";
      const message = `Couldn't move '${abs}' to the Trash. Try again, or delete it in Finder.`;
      expect(shortenKnownPaths(message, [abs])).toBe(
        "Couldn't move '测试.md' to the Trash. Try again, or delete it in Finder.",
      );
    });

    test('replaces every occurrence, not just the first', () => {
      const abs = '/Users/me/site/notes.md';
      const message = `Failed to move '${abs}' to trash: error deleting '${abs}'`;
      expect(shortenKnownPaths(message, [abs])).toBe("Failed to move 'notes.md' to trash: error deleting 'notes.md'");
    });

    test('a message naming none of the known paths passes through unchanged', () => {
      expect(shortenKnownPaths('disk on fire', ['/Users/me/site/notes.md'])).toBe('disk on fire');
    });

    test('only substitutes paths from the given list, in a batch of several', () => {
      const failed = '/Users/me/site/b.md';
      const others = ['/Users/me/site/a.md', failed, '/Users/me/site/c.md'];
      expect(shortenKnownPaths(`Failed to delete '${failed}'`, others)).toBe("Failed to delete 'b.md'");
    });
  });
});
