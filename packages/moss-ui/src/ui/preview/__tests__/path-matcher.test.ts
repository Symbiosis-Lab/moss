import { describe, it, expect } from 'vitest';
import {
  pathMatchesCurrentPage,
  extractPathFromUrl,
  buildPreviewUrl,
  sourcePathToHtmlPath,
} from '../path-matcher';

describe('pathMatchesCurrentPage', () => {
  describe('.md to .html conversion', () => {
    it('matches simple markdown to html', () => {
      expect(pathMatchesCurrentPage('post.md', 'post.html')).toBe(true);
    });

    it('matches nested markdown to html', () => {
      expect(pathMatchesCurrentPage('blog/post.md', 'blog/post.html')).toBe(true);
    });

    it('matches deeply nested paths', () => {
      expect(pathMatchesCurrentPage('blog/2024/01/post.md', 'blog/2024/01/post.html')).toBe(true);
    });
  });

  describe('index file inference', () => {
    it('matches folder/index.md to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/index.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/index.md to folder/', () => {
      expect(pathMatchesCurrentPage('blog/index.md', 'blog/')).toBe(true);
    });

    it('matches root index.md to index.html', () => {
      expect(pathMatchesCurrentPage('index.md', 'index.html')).toBe(true);
    });

    it('matches root index.md to empty path', () => {
      expect(pathMatchesCurrentPage('index.md', '')).toBe(true);
    });

    // README.md as index stem
    it('matches folder/README.md to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/README.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/README.md to folder/', () => {
      expect(pathMatchesCurrentPage('blog/README.md', 'blog/')).toBe(true);
    });

    it('matches root README.md to index.html', () => {
      expect(pathMatchesCurrentPage('README.md', 'index.html')).toBe(true);
    });

    it('matches root README.md to empty path', () => {
      expect(pathMatchesCurrentPage('README.md', '')).toBe(true);
    });

    // _index.md as index stem
    it('matches folder/_index.md to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/_index.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/_index.md to folder/', () => {
      expect(pathMatchesCurrentPage('blog/_index.md', 'blog/')).toBe(true);
    });

    it('matches root _index.md to index.html', () => {
      expect(pathMatchesCurrentPage('_index.md', 'index.html')).toBe(true);
    });

    it('matches root _index.md to empty path', () => {
      expect(pathMatchesCurrentPage('_index.md', '')).toBe(true);
    });

    // main.md as index stem
    it('matches folder/main.md to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/main.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/main.md to folder/', () => {
      expect(pathMatchesCurrentPage('blog/main.md', 'blog/')).toBe(true);
    });

    it('matches root main.md to index.html', () => {
      expect(pathMatchesCurrentPage('main.md', 'index.html')).toBe(true);
    });

    it('matches root main.md to empty path', () => {
      expect(pathMatchesCurrentPage('main.md', '')).toBe(true);
    });

    // Case insensitivity
    it('matches folder/readme.md (lowercase) to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/readme.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/Readme.md (mixed case) to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/Readme.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/MAIN.md (uppercase) to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/MAIN.md', 'blog/index.html')).toBe(true);
    });

    it('matches folder/INDEX.md (uppercase) to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/INDEX.md', 'blog/index.html')).toBe(true);
    });

    it('matches root Readme.md to index.html', () => {
      expect(pathMatchesCurrentPage('Readme.md', 'index.html')).toBe(true);
    });

    it('matches root MAIN.md to index.html', () => {
      expect(pathMatchesCurrentPage('MAIN.md', 'index.html')).toBe(true);
    });

    // Non-index .md files should NOT match index patterns
    it('does not match folder/article.md to folder/index.html', () => {
      expect(pathMatchesCurrentPage('blog/article.md', 'blog/index.html')).toBe(false);
    });

    it('does not match folder/about.md to folder/', () => {
      expect(pathMatchesCurrentPage('blog/about.md', 'blog/')).toBe(false);
    });
  });

  describe('direct matches', () => {
    it('matches identical paths (for non-markdown files)', () => {
      expect(pathMatchesCurrentPage('images/photo.jpg', 'images/photo.jpg')).toBe(true);
    });

    it('matches html files directly', () => {
      expect(pathMatchesCurrentPage('about.html', 'about.html')).toBe(true);
    });
  });

  describe('leading slash handling', () => {
    it('matches with leading slash in source', () => {
      expect(pathMatchesCurrentPage('/blog/post.md', 'blog/post.html')).toBe(true);
    });

    it('matches with leading slash in html path', () => {
      expect(pathMatchesCurrentPage('blog/post.md', '/blog/post.html')).toBe(true);
    });

    it('matches with leading slashes in both', () => {
      expect(pathMatchesCurrentPage('/blog/post.md', '/blog/post.html')).toBe(true);
    });
  });

  describe('non-matches', () => {
    it('does not match unrelated paths', () => {
      expect(pathMatchesCurrentPage('blog/post.md', 'other/page.html')).toBe(false);
    });

    it('does not match different filenames', () => {
      expect(pathMatchesCurrentPage('blog/article.md', 'blog/post.html')).toBe(false);
    });

    it('does not match partial paths', () => {
      expect(pathMatchesCurrentPage('post.md', 'blog/post.html')).toBe(false);
    });
  });
});

describe('extractPathFromUrl', () => {
  // One table, not 17 one-line `it`s across five sub-describes. Every case was
  // the same shape — one URL in, one path (or null) out — so the sub-describe
  // headings were carrying the only information the test names did not, and
  // they are preserved here as the first column.
  it.each([
    ['valid URL',      'http://localhost:3000/blog/post.html',            'blog/post.html'],
    ['nested path',    'http://localhost:8080/blog/2024/01/post.html',    'blog/2024/01/post.html'],
    ['other port',     'http://localhost:8080/about.html',                'about.html'],

    ['root → index',   'http://localhost:3000/',                          'index.html'],
    ['no path → index','http://localhost:3000',                           'index.html'],
    ['explicit index', 'http://localhost:3000/index.html',                'index.html'],

    // Browser shows "/slug/" for pretty URLs; the backend uses "slug/index.html".
    ['pretty dir',     'http://localhost:3000/blog/',                     'blog/index.html'],
    ['nested dir',     'http://localhost:3000/blog/2024/',                'blog/2024/index.html'],
    ['CJK dir',        'http://localhost:3000/%E6%96%87%E7%AB%A0/',       '文章/index.html'],

    ['not a URL',      'not-a-url',                                       null],
    ['empty string',   '',                                                null],
    ['relative path',  'blog/post.html',                                  null],

    ['percent CJK',    'http://localhost:3000/%E6%96%87%E7%AB%A0/post.html', '文章/post.html'],
    ['deep CJK',
     'http://localhost:3000/%E6%96%87%E7%AB%A0/%E6%97%A0%E7%94%A8%E4%B9%8B%E6%97%85/1-%E4%BB%8E%E4%BC%98%E8%83%9C%E7%BE%8E%E5%9C%B0%E5%87%BA%E5%8F%91.html',
     '文章/无用之旅/1-从优胜美地出发.html'],
    ['%20 spaces',     'http://localhost:3000/my%20folder/post.html',     'my folder/post.html'],
    ['escaped &',      'http://localhost:3000/path%26with%26ampersands.html', 'path&with&ampersands.html'],
    // Literal (already-decoded) characters must pass through, not double-decode.
    ['literal CJK',    'http://localhost:3000/文章/post.html',            '文章/post.html'],
  ])('%s: %s', (_label, url, expected) => {
    expect(extractPathFromUrl(url)).toBe(expected);
  });
});

describe('buildPreviewUrl', () => {
  it('builds URL with port only', () => {
    expect(buildPreviewUrl(3000)).toBe('http://localhost:3000');
  });

  it('builds URL with path', () => {
    expect(buildPreviewUrl(3000, 'blog/post.html')).toBe('http://localhost:3000/blog/post.html');
  });

  it('builds URL with nested path', () => {
    expect(buildPreviewUrl(8080, 'blog/2024/01/post.html')).toBe('http://localhost:8080/blog/2024/01/post.html');
  });

  it('handles index.html path by returning base URL', () => {
    expect(buildPreviewUrl(3000, 'index.html')).toBe('http://localhost:3000');
  });

  it('handles undefined path', () => {
    expect(buildPreviewUrl(3000, undefined)).toBe('http://localhost:3000');
  });

  it('handles empty path', () => {
    expect(buildPreviewUrl(3000, '')).toBe('http://localhost:3000');
  });
});

describe('sourcePathToHtmlPath', () => {
  it('converts .md to .html', () => {
    expect(sourcePathToHtmlPath('post.md')).toBe('post.html');
  });

  it('converts nested path', () => {
    expect(sourcePathToHtmlPath('blog/post.md')).toBe('blog/post.html');
  });

  it('preserves non-markdown files', () => {
    expect(sourcePathToHtmlPath('about.html')).toBe('about.html');
  });

  it('preserves paths without extension', () => {
    expect(sourcePathToHtmlPath('README')).toBe('README');
  });
});
