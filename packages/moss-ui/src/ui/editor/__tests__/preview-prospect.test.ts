/**
 * The preview's "why didn't it move?" contract, driven end to end.
 *
 * PreviewFollower is the sole owner of both outcomes — whether to arm the
 * FileChanged retry, and which note to paint — so this test drives the REAL
 * follower and the REAL `setPreviewNote` against real DOM. Asserting against a
 * re-derived copy of the policy is what let a `.moss/theme` image be classified
 * site-wide by one writer and "not a page" by another.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import type { SourceRole } from '@symbiosis-lab/moss-ui';
import { PreviewFollower } from '../preview-follower';
import { setPreviewNote } from '../preview-note';

function makeRegistryStub() {
  return {
    subscribeActivePath: (_: (p: string | null) => void) => () => {},
    activeEntryId: null,
    getById: (_: number) => null,
  } as any;
}

/** Runs one navigation through the real follower; reports what it did. */
async function follow(path: string, role: SourceRole | null) {
  let retries = 0;
  let asked = 0;
  const follower = new PreviewFollower({
    registry: makeRegistryStub(),
    projectRoot: '/proj',
    resolveUrlForFile: async () => ({ status: 'ok', data: null }),
    emit: async () => {},
    onMossEvent: async () => { retries++; return () => {}; },
    setNote: setPreviewNote,
    describeSourceRole: async () => {
      asked++;
      return role
        ? { status: 'ok' as const, data: role }
        : { status: 'error' as const, error: 'unavailable' };
    },
  });
  await follower.navigate(path);
  const el = document.getElementById('preview-note')!;
  return { retries, asked, note: el.hidden ? null : el.dataset.i18n };
}

const page = (over: Partial<SourceRole> = {}): SourceRole =>
  ({ slot: null, language: null, can_be_page: true, ...over }) as SourceRole;

const SITE_WIDE = 'editor.preview_note_site_wide';
const NOT_A_PAGE = 'editor.preview_note_not_a_page';

describe('preview prospect → note + retry', () => {
  let noteEl: HTMLElement;

  beforeEach(() => {
    noteEl = document.createElement('div');
    noteEl.id = 'preview-note';
    noteEl.hidden = true;
    document.body.appendChild(noteEl);
  });
  afterEach(() => noteEl.remove());

  const cases: Array<{
    kind: string; path: string; role: SourceRole | null;
    note: string | null; retries: number; asked: number;
  }> = [
    // `.moss/` is a path fact — settled without a round trip, whatever the file is.
    { kind: '.moss config', path: '.moss/config.toml', role: null,
      note: SITE_WIDE, retries: 0, asked: 0 },
    { kind: '.moss theme asset', path: '.moss/theme/grain.png', role: null,
      note: SITE_WIDE, retries: 0, asked: 0 },

    // Assets navigate by their own built URL; an unbuilt one is not "not a page".
    { kind: 'image', path: 'photo.png', role: null, note: null, retries: 0, asked: 0 },
    { kind: 'folder (no extension)', path: 'posts', role: null, note: null, retries: 0, asked: 0 },

    // Text files: only the backend knows whether the build makes pages of them.
    { kind: 'markdown', path: 'posts/new.md', role: page(),
      note: null, retries: 1, asked: 1 },
    { kind: 'plain text', path: 'notes.txt', role: page({ can_be_page: false }),
      note: NOT_A_PAGE, retries: 0, asked: 1 },
    { kind: 'json', path: 'data.json', role: page({ can_be_page: false }),
      note: NOT_A_PAGE, retries: 0, asked: 1 },
    // `.mdx` reads as markdown in the editor but the build has no mdx arm —
    // the exact drift a TS extension set would have got wrong.
    { kind: 'mdx', path: 'notes.mdx', role: page({ can_be_page: false }),
      note: NOT_A_PAGE, retries: 0, asked: 1 },
    // A slot file has no page of its own but shows on every page.
    { kind: 'footer slot', path: 'footer.md', role: page({ slot: 'footer-left' }),
      note: SITE_WIDE, retries: 0, asked: 1 },
  ];

  it.each(cases)('$kind → note=$note retries=$retries', async (c) => {
    const got = await follow(c.path, c.role);
    expect(got).toEqual({ note: c.note, retries: c.retries, asked: c.asked });
  });

  it('a failed probe keeps waiting rather than claiming the file is not a page', async () => {
    // Fail-open: an unavailable answer must never suppress navigation.
    const got = await follow('posts/new.md', null);
    expect(got).toEqual({ note: null, retries: 1, asked: 1 });
  });

  it('clears a stale note when the next file resolves normally', async () => {
    await follow('notes.txt', page({ can_be_page: false }));
    expect(document.getElementById('preview-note')!.hidden).toBe(false);
    await follow('posts/new.md', page());
    expect(document.getElementById('preview-note')!.hidden).toBe(true);
  });
});
