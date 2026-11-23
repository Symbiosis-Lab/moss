import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../../shared/ui/toast/toast-manager', () => ({ showToast: vi.fn(), showAck: vi.fn() }));
vi.mock('../../engine', () => ({ engine: { pageComments: vi.fn() } }));

import { showAck, showToast } from '../../shared/ui/toast/toast-manager';
import { engine } from '../../engine';
import { host } from '../../host';
import { setLocale } from '../../i18n';
import { mountPageComments } from '../page-comments';

const mod = { hide: vi.fn(), unhide: vi.fn(), spam: vi.fn(), undoSpam: vi.fn() };
const pageComments = engine.pageComments as unknown as ReturnType<typeof vi.fn>;
const toast = showToast as unknown as ReturnType<typeof vi.fn>;
const ack = showAck as unknown as ReturnType<typeof vi.fn>;
const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0));
};

const comment = (id: string, over: Record<string, unknown> = {}) => ({
  source: 'artalk', id, author: `A${id}`, website: null, createdAt: '2026-11-20T10:00:00Z', text: `text ${id}`,
  hidden: false, directlyHidden: false, ...over,
});
const answer = (comments: unknown[]) => ({ status: 'ok', data: { comments } });
const outcome = { hidden: [['artalk', '1']], blocked: ['a.example', 'b.example'], notBlocked: [], alsoHidden: 3 };

function mount() {
  const parent = document.createElement('div');
  document.body.replaceChildren(parent);
  const foot = mountPageComments(parent, () => '/folder')!;
  return { parent, foot, bar: parent.querySelector<HTMLElement>('.pc-bar')! };
}
const rowOf = (bar: HTMLElement, id: string) => [...bar.querySelectorAll<HTMLElement>('.pc-row')].find((r) => r.dataset.id === id)!;
const buttons = (bar: HTMLElement, id: string) => [...rowOf(bar, id).querySelectorAll('button')].map((b) => b.textContent);

describe('the open page’s comments', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    host.comments = mod as unknown as typeof host.comments;
    for (const f of [mod.hide, mod.unhide, mod.undoSpam]) f.mockResolvedValue({ status: 'ok', data: null });
    mod.spam.mockResolvedValue({ status: 'ok', data: outcome });
  });
  afterEach(() => setLocale('en'));

  it('offers nothing where the host cannot moderate', () => {
    host.comments = undefined;
    const parent = document.createElement('div');
    expect(mountPageComments(parent, () => '/folder')).toBeNull();
    expect(parent.children).toHaveLength(0);
  });

  it('shows the count, Hide and Spam on a shown comment, Show only where Show can bring it back', async () => {
    pageComments.mockResolvedValue(answer([
      comment('1', { website: 'meds.example', text: '<b>bold</b>' }),
      comment('2', { hidden: true, directlyHidden: true }),
      comment('3', { hidden: true }),
    ]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    expect(pageComments).toHaveBeenCalledWith('/folder/a.md');
    expect(bar.hidden).toBe(false);
    expect(bar.querySelector('.pc-count')!.textContent).toBe('3 comments');
    expect(buttons(bar, '1')).toEqual(['Hide', 'Spam']);
    expect(buttons(bar, '2')).toEqual(['Show']);
    expect(buttons(bar, '3')).toEqual([]);
    const first = rowOf(bar, '1');
    expect(first.querySelector('b')).toBeNull();
    expect(first.querySelector('.pc-text')!.textContent).toBe('<b>bold</b>');
    expect(first.textContent).toContain('meds.example');
  });

  it('opens and closes from the count, saying so to assistive tech', async () => {
    pageComments.mockResolvedValue(answer([comment('1')]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    const toggle = bar.querySelector<HTMLButtonElement>('.pc-toggle')!;
    expect(toggle.textContent).toBe('1 comment');
    toggle.click();
    expect(bar.hasAttribute('data-open')).toBe(true);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
  });

  it('is hidden for a page with no comments and for a file that is not a page', async () => {
    pageComments.mockResolvedValue(answer([]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    expect(bar.hidden).toBe(true);
    await foot.show(null);
    expect(bar.hidden).toBe(true);
    expect(pageComments).toHaveBeenCalledTimes(1);
  });

  it('speaks the reader’s language: German counts, labels and the Spam toast', async () => {
    setLocale('de');
    pageComments.mockResolvedValue(answer([comment('1'), comment('2')]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    expect(bar.querySelector('.pc-count')!.textContent).toBe('2 Kommentare');
    expect(buttons(bar, '1')).toEqual(['Ausblenden', 'Spam melden']);
    expect(rowOf(bar, '1').querySelectorAll('button')[1].getAttribute('aria-label')).toBe('Kommentar von A1 als Spam melden');
    rowOf(bar, '1').querySelectorAll('button')[1].click();
    await flush();
    expect(ack.mock.calls[0][0]).toBe('Als Spam gemeldet · a.example und b.example gesperrt · 3 weitere Kommentare ausgeblendet');
    expect(ack.mock.calls[0][1].actions[0].label).toBe('Widerrufen');
    pageComments.mockResolvedValue(answer([comment('1')]));
    await foot.show('/folder/a.md');
    expect(bar.querySelector('.pc-count')!.textContent).toBe('1 Kommentar');
  });

  it('Spam names the hosts it blocked and how many more it hid, and Undo hands the outcome back', async () => {
    pageComments.mockResolvedValue(answer([comment('1')]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    rowOf(bar, '1').querySelectorAll('button')[1].click();
    await flush();
    expect(mod.spam).toHaveBeenCalledWith('/folder', 'artalk', '1');
    const [what, options] = ack.mock.calls[0];
    expect(what).toBe('Marked as spam · a.example and b.example blocked · 3 more comments hidden');
    expect(pageComments).toHaveBeenCalledTimes(2);
    options.actions[0].onClick();
    await flush();
    expect(mod.undoSpam).toHaveBeenCalledWith('/folder', outcome);
    expect(pageComments).toHaveBeenCalledTimes(3);
  });

  it('Hide names the comment by source and id, its busy state keeps focus, and Show reverses it', async () => {
    pageComments.mockResolvedValue(answer([comment('1'), comment('2', { hidden: true, directlyHidden: true })]));
    let answerHide!: (v: unknown) => void;
    mod.hide.mockReturnValueOnce(new Promise((r) => { answerHide = r; }));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    const hide = rowOf(bar, '1').querySelector('button')!;
    hide.focus();
    hide.click();
    expect(hide.getAttribute('aria-disabled')).toBe('true');
    expect(hide.disabled).toBe(false);
    expect(document.activeElement).toBe(hide);
    pageComments.mockResolvedValue(answer([comment('1', { hidden: true, directlyHidden: true }), comment('2', { hidden: true, directlyHidden: true })]));
    answerHide({ status: 'ok', data: null });
    await flush();
    expect(mod.hide).toHaveBeenCalledWith('/folder', 'artalk', '1');
    expect(document.activeElement).toBe(rowOf(bar, '1').querySelector('button'));
    expect(document.activeElement!.textContent).toBe('Show');
    const [what, options] = ack.mock.calls[0];
    expect(what).toBe('Comment hidden');
    options.actions[0].onClick();
    await flush();
    expect(mod.unhide).toHaveBeenCalledWith('/folder', 'artalk', '1');
    rowOf(bar, '2').querySelector('button')!.click();
    await flush();
    expect(mod.unhide).toHaveBeenLastCalledWith('/folder', 'artalk', '2');
  });

  it('focus falls back to the count when the comment has gone', async () => {
    pageComments.mockResolvedValue(answer([comment('1'), comment('2')]));
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    pageComments.mockResolvedValue(answer([comment('2')]));
    rowOf(bar, '1').querySelector('button')!.click();
    await flush();
    expect(document.activeElement).toBe(bar.querySelector('.pc-toggle'));
  });

  it('a refused write says why and changes nothing else', async () => {
    pageComments.mockResolvedValue(answer([comment('1')]));
    mod.spam.mockResolvedValue({ status: 'error', error: 'owner identity unavailable' });
    const { foot, bar } = mount();
    await foot.show('/folder/a.md');
    rowOf(bar, '1').querySelectorAll('button')[1].click();
    await flush();
    expect(toast.mock.calls[0][0]).toEqual({ what: "Couldn't mark the comment as spam: owner identity unavailable", severity: 'NeedsAction' });
    expect(ack).not.toHaveBeenCalled();
    expect(mod.undoSpam).not.toHaveBeenCalled();
  });
});
