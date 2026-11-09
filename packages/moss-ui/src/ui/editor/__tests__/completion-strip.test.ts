/**
 * Where a picked file lands in the document.
 *
 * The assertions that matter here are the RESULTING DOCUMENT TEXT, not the
 * parse. An earlier draft of this feature routed every phase through the
 * shortcode accept path; it parsed correctly, produced the right ranges, and
 * emitted `![[cover.png` with no closing brackets — in exactly the state the
 * picker exists to serve. A test that stopped at the range would have shipped
 * it green.
 */
import { describe, it, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { startCompletion, completionStatus } from '@codemirror/autocomplete';
import { applyCompletion } from '../cm-completion';
import { assetBrowseExtension, pickSiteAt } from '../asset-picker/completion-strip';
import { wikilinkCompleteExtension, type WikilinkCompleteOptions } from '../cm-wikilink-complete';
import { host } from "../../host";

// A host that turns a picked file into a reference, which is what Browse needs.
host.assetRefForPath = async () => null;

const PICK = { insert: 'img/photo.jpg', label: 'photo.jpg', detail: 'img/photo.jpg', kind: 'asset' as const };

/** A live view with the caret at `|` (the marker is stripped from the doc). */
function viewAt(marked: string): EditorView {
  const pos = marked.indexOf('|');
  const doc = marked.replace('|', '');
  const container = document.createElement('div');
  document.body.appendChild(container);
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: [markdown()] }),
    parent: container,
  });
  view.dispatch({ selection: { anchor: pos } });
  return view;
}

function pickInto(marked: string): string {
  const view = viewAt(marked);
  const site = pickSiteAt(view.state, view.state.selection.main.head);
  expect(site).not.toBeNull();
  applyCompletion(view, site!.from, site!.to, PICK, site!.ctx);
  const text = view.state.doc.toString();
  view.destroy();
  return text;
}

describe('pickSiteAt', () => {
  it('says nothing in prose', () => {
    const view = viewAt('just some words|');
    expect(pickSiteAt(view.state, view.state.selection.main.head)).toBeNull();
    view.destroy();
  });

  it('says nothing for an attr that takes no asset', () => {
    const view = viewAt(':::grid {cols=|');
    expect(pickSiteAt(view.state, view.state.selection.main.head)).toBeNull();
    view.destroy();
  });

  it('offers every embeddable kind inside an embed wikilink', () => {
    const view = viewAt('![[cov|');
    expect(pickSiteAt(view.state, view.state.selection.main.head)).toMatchObject({
      ctx: { phase: 'wikilink' },
      kinds: null,
    });
    view.destroy();
  });

  it('narrows to the kinds a hero image attr accepts', () => {
    const view = viewAt(':::hero {image=|');
    const site = pickSiteAt(view.state, view.state.selection.main.head);
    expect(site?.ctx).toMatchObject({ phase: 'attr-value', blockName: 'hero', attrName: 'image' });
    expect(site?.kinds).toContain('Image');
    view.destroy();
  });
});

describe('a pick lands through applyCompletion', () => {
  // One case proves the routing; every per-phase form of the accept path is
  // pinned in cm-completion.test.ts and cm-wikilink-complete.test.ts.
  it('closes the brackets of an embed wikilink', () => {
    expect(pickInto('![[cov|')).toBe('![[img/photo.jpg]]');
  });
});

describe('the tooltip strip', () => {
  // The completion tooltip is library-owned DOM that jsdom will not produce
  // from a real async completion, so the test supplies the node CM6 would and
  // checks what the plugin does with it. What is being proven is the plugin's
  // rule — a Browse button exactly when the caret is asking for a file — not
  // @codemirror/autocomplete's rendering.
  function withTooltip(marked: string): { view: EditorView; strip(): Element | null } {
    const pos = marked.indexOf('|');
    const doc = marked.replace('|', '');
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown(), assetBrowseExtension({
          pagePath: () => 'posts/hello.md',
          projectRoot: () => '/vault',
        })],
      }),
      parent: container,
    });
    const tooltip = document.createElement('div');
    tooltip.className = 'cm-tooltip-autocomplete';
    view.dom.appendChild(tooltip);
    view.dispatch({ selection: { anchor: pos } });
    return { view, strip: () => view.dom.querySelector('.cm-asset-browse-strip') };
  }

  it('appears when the caret is asking for a file', () => {
    const { view, strip } = withTooltip('![[cov|');
    expect(strip()).not.toBeNull();
    expect(strip()!.querySelector('button')?.textContent?.trim()).not.toBe('');
    view.destroy();
  });

  it('stays away in a host that cannot turn a picked file into a reference', () => {
    const kept = host.assetRefForPath;
    host.assetRefForPath = undefined;
    try {
      const { view, strip } = withTooltip('![[cov|');
      expect(strip()).toBeNull();
      view.destroy();
    } finally {
      host.assetRefForPath = kept;
    }
  });

  it('stays away when the completion is not about a file', () => {
    const { view, strip } = withTooltip('just words|');
    expect(strip()).toBeNull();
    view.destroy();
  });

  it('leaves when the caret moves out of an asset position', () => {
    const { view, strip } = withTooltip('![[cov|\n\nplain prose here');
    expect(strip()).not.toBeNull();
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    expect(strip()).toBeNull();
    view.destroy();
  });

  it('drops the pick when the caret has left the asset position', () => {
    // The dialog is modal and slow; an external edit or a save-and-reload can
    // move the document under it. The range captured before the dialog opened
    // then points at whatever now occupies those offsets, so the pick is
    // dropped rather than spliced somewhere it was never meant to go.
    const { view, strip } = withTooltip('![[cov|\n\nplain prose here');
    const btn = strip()!.querySelector('button') as HTMLButtonElement;
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    const before = view.state.doc.toString();
    btn.click();
    expect(view.state.doc.toString()).toBe(before);
    view.destroy();
  });

  it('takes no tab stop — focusing inside the tooltip closes the completion', () => {
    const { view, strip } = withTooltip('![[cov|');
    expect((strip() as HTMLElement).tabIndex).toBe(-1);
    view.destroy();
  });
});

describe('the Browse button inside a REAL completion tooltip (CM6-owned DOM)', () => {
  // The other `describe` above hands the plugin a tooltip DIV it built by
  // hand, because jsdom cannot produce one from a real async completion. This
  // block does the opposite on purpose: it lets @codemirror/autocomplete build
  // its OWN `.cm-tooltip-autocomplete`, with its OWN `mousedown` listener on
  // that element. The strip's button lives INSIDE that element (completion-
  // strip.ts inserts it as `tooltip.firstChild`), so a `mousedown` on the
  // button bubbles into the library's own ancestor listener — which is the
  // chain the fake-tooltip tests above cannot exercise at all.

  /** Poll a real-time condition; throws rather than letting a stall read as a pass. */
  async function pollUntil(done: () => boolean, deadlineMs = 5000): Promise<void> {
    for (let waited = 0; waited < deadlineMs; waited += 5) {
      if (done()) return;
      await new Promise((r) => setTimeout(r, 5));
    }
    throw new Error('condition never settled');
  }

  async function viewWithOpenAssetTooltip(
    marked: string,
  ): Promise<{ view: EditorView; container: HTMLElement }> {
    const pos = marked.indexOf('|');
    const doc = marked.replace('|', '');
    const container = document.createElement('div');
    document.body.appendChild(container);
    const opts: WikilinkCompleteOptions = {
      complete: async () => [{ insert: 'photo.jpg', label: 'photo.jpg', detail: null, kind: 'asset' }],
      getCurrentPath: () => 'index.md',
    };
    const view = new EditorView({
      state: EditorState.create({
        doc,
        selection: { anchor: pos },
        extensions: [
          markdown(),
          wikilinkCompleteExtension(opts),
          assetBrowseExtension({ pagePath: () => 'index.md', projectRoot: () => '/vault' }),
        ],
      }),
      parent: container,
    });
    startCompletion(view);
    // Same settle signal as cm-wikilink-complete.test.ts: 'pending' only
    // clears once every source has resolved and CM6 has built the real tooltip.
    await pollUntil(() => completionStatus(view.state) !== 'pending');
    // The strip plugin's MutationObserver callback runs as a separate
    // microtask/macrotask after the tooltip lands — poll rather than assume.
    await pollUntil(() => view.dom.querySelector('.cm-asset-browse-strip button') !== null);
    return { view, container };
  }

  it('a mousedown on Browse does not throw inside the tooltip\'s own ancestor listener', async () => {
    const { view, container } = await viewWithOpenAssetTooltip('![[cov|');
    const btn = view.dom.querySelector('.cm-asset-browse-strip button') as HTMLButtonElement;
    expect(btn).not.toBeNull();
    expect(view.dom.querySelector('.cm-tooltip-autocomplete')).not.toBeNull();

    const errors: unknown[] = [];
    const onError = (e: ErrorEvent) => errors.push(e.error ?? e.message);
    window.addEventListener('error', onError);
    try {
      // A real click is pointerdown -> mousedown -> ... ; `.click()` alone
      // (as the fake-tooltip tests above use) never fires `mousedown` and so
      // never reaches the ancestor listener this test is about.
      btn.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    } finally {
      window.removeEventListener('error', onError);
    }

    expect(errors).toEqual([]);
    view.destroy();
    container.remove();
  });
});
