/**
 * Shell side of the preview right-click menu: payload guard, coordinate
 * translation through the (possibly zoomed) iframe, published-URL
 * construction, and which entries render for which context.
 *
 * Copied links are the PUBLISHED address, never the localhost dev-server URL;
 * with no site URL configured the copy is the site-relative path.
 */
import { describe, it, expect, vi } from 'vitest';
import {
  isPreviewCtxMessage,
  translateIframePoint,
  localPreviewUrl,
  publishedUrlFor,
  buildPreviewCtxEntries,
  type PreviewCtxPayload,
  type PreviewCtxDeps,
} from '../preview-context-menu';
import type { CtxMenuItem } from "../../shared/ui/ctx-menu";

const PAGE = 'http://localhost:4832/posts/hello/?__moss_shell=1';

function ctx(overrides: Partial<PreviewCtxPayload> = {}): PreviewCtxPayload {
  return {
    type: 'moss-context-menu',
    x: 5,
    y: 6,
    page: PAGE,
    targets: [],
    styles: [],
    link: null,
    image: null,
    selection: null,
    ...overrides,
  };
}

function deps(overrides: Partial<PreviewCtxDeps> = {}): PreviewCtxDeps {
  return {
    siteUrl: null,
    canEdit: false,
    pageFile: null,
    openTarget: vi.fn(),
    copy: vi.fn(),
    openExternal: vi.fn(),
    openStyle: vi.fn(),
    ...overrides,
  };
}

/** Labels of the rendered items, separators shown as '---'. */
function labels(entries: ReturnType<typeof buildPreviewCtxEntries>): string[] {
  return entries.map((e) => ('label' in e ? e.label : '---'));
}

function item(entries: ReturnType<typeof buildPreviewCtxEntries>, label: string): CtxMenuItem {
  const found = entries.find((e): e is CtxMenuItem => 'action' in e && e.label === label);
  if (!found) throw new Error(`no item ${label}`);
  return found;
}

const click = new MouseEvent('click');

describe('isPreviewCtxMessage', () => {
  it('accepts the bridge payload and rejects other messages', () => {
    expect(isPreviewCtxMessage(ctx())).toBe(true);
    expect(isPreviewCtxMessage({ type: 'moss-navigation' })).toBe(false);
    expect(isPreviewCtxMessage(null)).toBe(false);
    expect(isPreviewCtxMessage({ type: 'moss-context-menu', x: 'NaN', y: 0, page: '' })).toBe(false);
  });
});

describe('translateIframePoint', () => {
  function iframeAt(rect: Partial<DOMRect>, clientW: number, clientH: number): HTMLIFrameElement {
    const iframe = document.createElement('iframe');
    iframe.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 0, height: 0, ...rect }) as DOMRect;
    Object.defineProperty(iframe, 'clientWidth', { value: clientW });
    Object.defineProperty(iframe, 'clientHeight', { value: clientH });
    return iframe;
  }

  it('offsets by the iframe position at zoom 1', () => {
    const iframe = iframeAt({ left: 300, top: 48, width: 800, height: 600 }, 800, 600);
    expect(translateIframePoint(iframe, 100, 50)).toEqual({ x: 400, y: 98 });
  });

  it('scales by the CSS-transform zoom recovered from the rect', () => {
    // Preview zoom 0.8: iframe laid out at 1000px, painted at 800px.
    const iframe = iframeAt({ left: 300, top: 48, width: 800, height: 480 }, 1000, 600);
    expect(translateIframePoint(iframe, 100, 100)).toEqual({ x: 380, y: 128 });
  });

  it('falls back to scale 1 when the iframe has no layout (jsdom)', () => {
    const iframe = iframeAt({ left: 10, top: 20 }, 0, 0);
    expect(translateIframePoint(iframe, 7, 8)).toEqual({ x: 17, y: 28 });
  });
});

describe('localPreviewUrl', () => {
  it('strips the shell marker but keeps everything else', () => {
    expect(localPreviewUrl('http://localhost:4832/a/?__moss_shell=1&x=2#h')).toBe(
      'http://localhost:4832/a/?x=2#h',
    );
  });
});

describe('publishedUrlFor', () => {
  it('joins a same-origin path onto the published site URL', () => {
    expect(publishedUrlFor('http://localhost:4832/posts/foo/', PAGE, 'https://her-blog.mosspub.com'))
      .toEqual({ text: 'https://her-blog.mosspub.com/posts/foo/', siteRelative: false });
  });

  it('strips the shell marker before joining', () => {
    expect(
      publishedUrlFor('http://localhost:4832/posts/foo/?__moss_shell=1', PAGE, 'https://example.com'),
    ).toEqual({ text: 'https://example.com/posts/foo/', siteRelative: false });
  });

  it('falls back to the site-relative path when no site URL is configured', () => {
    expect(publishedUrlFor('http://localhost:4832/posts/foo/#sec', PAGE, null)).toEqual({
      text: '/posts/foo/#sec',
      siteRelative: true,
    });
  });

  it('passes a cross-origin (external) URL through verbatim', () => {
    expect(publishedUrlFor('https://example.com/a?b=1', PAGE, 'https://her-blog.mosspub.com'))
      .toEqual({ text: 'https://example.com/a?b=1', siteRelative: false });
  });

  it('never emits a trailing-slash doubled join', () => {
    expect(publishedUrlFor('http://localhost:4832/x/', PAGE, 'https://example.com/')).toEqual({
      text: 'https://example.com/x/',
      siteRelative: false,
    });
  });
});

describe('buildPreviewCtxEntries', () => {
  it('offers the page address even when nothing is under the cursor', () => {
    expect(labels(buildPreviewCtxEntries(ctx(), deps()))).toEqual([
      'Copy page address',
      '---',
      'Open in browser',
    ]);
  });

  it('names the page copy as published once a site URL exists', () => {
    expect(labels(buildPreviewCtxEntries(ctx(), deps({ siteUrl: 'https://example.com' })))).toEqual([
      'Copy published URL',
      '---',
      'Open in browser',
    ]);
  });

  it('renders the full menu with separators between groups', () => {
    const entries = buildPreviewCtxEntries(
      ctx({
        selection: 'hi',
        link: 'http://localhost:4832/posts/foo/',
        image: 'http://localhost:4832/img/a.png',
      }),
      deps({ canEdit: true, siteUrl: 'https://example.com' }),
    );
    expect(labels(entries)).toEqual([
      'Edit this page',
      '---',
      'Copy',
      'Copy link',
      'Copy image address',
      'Copy published URL',
      '---',
      'Open in browser',
    ]);
  });

  it('labels the link copy honestly when only a path can be copied', () => {
    const entries = buildPreviewCtxEntries(
      ctx({ link: 'http://localhost:4832/posts/foo/' }),
      deps({ siteUrl: null }),
    );
    expect(labels(entries)).toEqual([
      'Copy link address',
      'Copy page address',
      '---',
      'Open in browser',
    ]);
  });

  it('copies the published link, not the localhost URL', () => {
    const d = deps({ siteUrl: 'https://her-blog.mosspub.com' });
    const entries = buildPreviewCtxEntries(ctx({ link: 'http://localhost:4832/posts/foo/' }), d);
    item(entries, 'Copy link').action(click);
    expect(d.copy).toHaveBeenCalledWith('https://her-blog.mosspub.com/posts/foo/');
  });

  it('copies an external link verbatim even with a site URL configured', () => {
    const d = deps({ siteUrl: 'https://her-blog.mosspub.com' });
    const entries = buildPreviewCtxEntries(ctx({ link: 'https://example.com/a' }), d);
    item(entries, 'Copy link').action(click);
    expect(d.copy).toHaveBeenCalledWith('https://example.com/a');
  });

  it('copies the selection text', () => {
    const d = deps();
    const entries = buildPreviewCtxEntries(ctx({ selection: 'quoted words' }), d);
    item(entries, 'Copy').action(click);
    expect(d.copy).toHaveBeenCalledWith('quoted words');
  });

  it('opens the LOCAL preview page in the browser — the sanctioned localhost use', () => {
    const d = deps({ siteUrl: 'https://her-blog.mosspub.com' });
    const entries = buildPreviewCtxEntries(ctx(), d);
    item(entries, 'Open in browser').action(click);
    expect(d.openExternal).toHaveBeenCalledWith('http://localhost:4832/posts/hello/');
  });

  it('routes "Edit this page" to openTarget(null)', () => {
    const d = deps({ canEdit: true });
    const entries = buildPreviewCtxEntries(ctx(), d);
    item(entries, 'Edit this page').action(click);
    expect(d.openTarget).toHaveBeenCalledWith(null);
  });

  it('chrome (a none target) offers nothing to edit, not even the page; an unannotated element still offers the page', () => {
    const chrome = buildPreviewCtxEntries(ctx({ targets: [{ kind: 'none' }] }), deps({ canEdit: true }));
    expect(labels(chrome)).toEqual(['Copy page address', '---', 'Open in browser']);
    const plain = buildPreviewCtxEntries(ctx({ targets: [] }), deps({ canEdit: true }));
    expect(labels(plain)[0]).toBe('Edit this page');
  });

  it('hero headline: the heading and the hero, nearest first, both in the shown page', () => {
    const entries = buildPreviewCtxEntries(
      ctx({
        targets: [
          { kind: 'body-line', line: 4, element: 'h1' },
          { kind: 'body-range', startLine: 2, endLine: 7, element: 'section.moss-hero' },
        ],
      }),
      deps({ canEdit: true, pageFile: '/site/index.md' }),
    );
    expect(labels(entries).slice(0, 2)).toEqual(['Edit heading', 'Hero settings']);
    expect(entries.filter((e): e is CtxMenuItem => 'action' in e && e.detail !== undefined).map((e) => e.detail)).toEqual([
      'index.md',
      'index.md',
    ]);
  });

  it('a picture in a grid column: the column is not a second grid row', () => {
    const entries = buildPreviewCtxEntries(
      ctx({
        targets: [
          { kind: 'body-line', line: 10, element: 'p' },
          { kind: 'body-range', startLine: 3, endLine: 10, element: 'div.moss-grid-card' },
          { kind: 'body-range', startLine: 2, endLine: 22, element: 'div.moss-grid' },
        ],
        image: 'http://localhost:4832/pictures/room.jpg',
      }),
      deps({ canEdit: true, pageFile: '/site/index.md' }),
    );
    expect(labels(entries).slice(0, 3)).toEqual(['Edit paragraph', 'Edit block', 'Grid settings']);
  });

  it('nav entry: a settings row, then the named page row that opens the page itself', () => {
    const d = deps();
    const entries = buildPreviewCtxEntries(
      ctx({ targets: [{ kind: 'frontmatter', field: 'nav', path: 'About.md', element: 'a' }] }),
      d,
    );
    expect(labels(entries).slice(0, 2)).toEqual(['Nav settings', 'Edit About page']);
    expect(item(entries, 'Nav settings').detail).toBe('About.md');
    expect(item(entries, 'Edit About page').detail).toBe('About.md');
    item(entries, 'Nav settings').action(click);
    expect(d.openTarget).toHaveBeenLastCalledWith({ kind: 'frontmatter', field: 'nav', path: 'About.md', element: 'a' });
    item(entries, 'Edit About page').action(click);
    expect(d.openTarget).toHaveBeenLastCalledWith({ kind: 'page', path: 'About.md' });
  });

  it('a page target with no path is the shown page: "Edit this page", not a named row', () => {
    const d = deps();
    const page = { kind: 'page', element: 'div' };
    const entries = buildPreviewCtxEntries(ctx({ targets: [page] }), d);
    expect(labels(entries).slice(0, 1)).toEqual(['Edit this page']);
    item(entries, 'Edit this page').action(click);
    expect(d.openTarget).toHaveBeenCalledWith(page);
  });

  it('card: no extra page row when the targets already name the page', () => {
    const entries = buildPreviewCtxEntries(
      ctx({
        targets: [
          { kind: 'frontmatter', field: 'title', path: 'posts/a.md', element: 'h3' },
          { kind: 'page', path: 'posts/a.md', element: 'article' },
          { kind: 'body-line', line: 9, element: 'div' },
        ],
      }),
      deps({ pageFile: '/site/index.md' }),
    );
    expect(labels(entries).slice(0, 3)).toEqual(['Edit title', 'Edit a page', 'Edit line']);
  });

  it('offers no Reload/Back/Forward — navigation belongs to the shell chrome', () => {
    const entries = buildPreviewCtxEntries(
      ctx({ selection: 's', link: 'https://e.com', image: 'https://e.com/i.png' }),
      deps({ canEdit: true }),
    );
    for (const l of labels(entries)) {
      expect(l).not.toMatch(/reload|back|forward/i);
    }
  });
});

describe('buildPreviewCtxEntries style group', () => {
  it('offers one row per theme rule after the edit rows, opening that selector', () => {
    const openStyle = vi.fn();
    const entries = buildPreviewCtxEntries(
      ctx({ styles: ['.moss-hero h1', 'h1'], targets: [{ kind: 'page', path: 'posts/a.md' }] }),
      deps({ openStyle, pageFile: 'posts/a.md' }),
    );
    const rows = entries.filter((e): e is CtxMenuItem => 'action' in e && e.detail === 'style.css');
    expect(rows.map((r) => r.label)).toStrictEqual(['Style .moss-hero h1', 'Style h1']);
    expect(labels(entries).indexOf('Style .moss-hero h1')).toBeGreaterThan(labels(entries).indexOf('Edit a page'));
    item(entries, 'Style h1').action(new MouseEvent('click'));
    expect(openStyle).toHaveBeenCalledWith('h1');
  });

  it('adds no style group when the element has no theme rule', () => {
    const entries = buildPreviewCtxEntries(ctx({ styles: [] }), deps());
    expect(entries.some((e) => 'detail' in e && e.detail === 'style.css')).toBe(false);
  });
});
