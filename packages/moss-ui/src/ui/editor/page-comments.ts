/**
 * The open page's comments, under the article as on the page: a count that
 * opens to each comment with Hide and Spam, or Show once hidden. Every press
 * is a signed moderation event, which the next publish carries.
 */
import type { EngineResult, PageComment, SpamOutcome } from '@symbiosis-lab/moss-ui';
import { engine } from '../engine';
import { host, type CommentModeration } from '../host';
import { andList, formatDate, t } from '../i18n';
import { bindAction, runAction, type ActionSpec } from '../shared/ui/run-action';
import { showAck } from '../shared/ui/toast/toast-manager';
import type { ToastAction } from '../shared/ui/toast/toast-types';

const ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719"/></svg>';
const CHEV = '<svg class="pc-chev" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6"/></svg>';

type Key = Parameters<typeof t>[0];
type Write<T> = (folder: string) => Promise<EngineResult<T>>;

export interface PageCommentsFoot {
  /** Lists the comments of the page built from `filePath`; `null` empties the foot. */
  show(filePath: string | null): Promise<void>;
}

/** Appends the foot to `parent`; none where the host cannot moderate. */
export function mountPageComments(parent: HTMLElement, folder: () => string | null): PageCommentsFoot | null {
  const mod = host.comments;
  if (!mod) return null;
  const bar = document.createElement('section');
  bar.className = 'pc-bar';
  bar.hidden = true;
  bar.innerHTML = `<button type="button" class="pc-toggle" aria-expanded="false" aria-controls="pc-list">${ICON}<span class="pc-count"></span>${CHEV}</button><ol class="pc-list" id="pc-list"></ol>`;
  parent.append(bar);
  const toggle = bar.querySelector<HTMLButtonElement>('.pc-toggle')!;
  const list = bar.querySelector<HTMLOListElement>('.pc-list')!;
  toggle.addEventListener('click', () => {
    const open = !bar.hasAttribute('data-open');
    bar.toggleAttribute('data-open', open);
    toggle.setAttribute('aria-expanded', String(open));
  });

  let path: string | null = null;
  let request = 0;
  /** Lists the page again; with `focus`, the press stays on that comment's action, or on the count once it has none. */
  const reload = async (focus?: PageComment): Promise<void> => {
    const ask = ++request;
    const page = path ? await engine.pageComments(path).catch(() => null) : null;
    if (ask !== request) return;
    const comments = page?.status === 'ok' ? page.data.comments : [];
    bar.hidden = comments.length === 0;
    bar.querySelector('.pc-count')!.textContent = t(comments.length === 1 ? 'comments.count_one' : 'comments.count_other', { count: comments.length });
    list.replaceChildren(...comments.map((c) => row(c, mod, folder, () => reload(c))));
    if (!focus) return;
    // Source and id are a stranger's text, so they are compared, never put in a selector.
    const same = [...list.children].find((li) => li instanceof HTMLElement && li.dataset.source === focus.source && li.dataset.id === focus.id);
    (same?.querySelector('button') ?? toggle).focus();
  };
  return {
    show(filePath) {
      path = filePath;
      return reload();
    },
  };
}

function row(c: PageComment, mod: CommentModeration, folder: () => string | null, reload: () => Promise<void>): HTMLLIElement {
  const li = document.createElement('li');
  li.className = c.hidden ? 'pc-row is-hidden' : 'pc-row';
  li.dataset.source = c.source;
  li.dataset.id = c.id;
  const head = document.createElement('div');
  head.className = 'pc-head';
  const time = document.createElement('time');
  time.dateTime = c.createdAt;
  const when = new Date(c.createdAt);
  time.textContent = Number.isNaN(when.getTime()) ? c.createdAt : formatDate(when, { month: 'short', day: 'numeric', year: 'numeric' });
  head.append(span('pc-author', c.author), ...(c.website ? [span('', c.website)] : []), time);
  const text = document.createElement('p');
  text.className = 'pc-text';
  text.textContent = c.text;
  const actions = document.createElement('div');
  actions.className = 'pc-actions';

  /** A write on the open folder; the list reloads after it on every path, so the busy state covers both. */
  const write = <T,>(fail: Key, run: Write<T>, onOk?: (value: T) => void): Omit<ActionSpec<EngineResult<T>>, 'trigger'> => ({
    run: async () => {
      const f = folder();
      if (!f) throw new Error('no folder is open');
      try {
        return await run(f);
      } finally {
        await reload();
      }
    },
    failMessage: (error) => t(fail, { error }),
    onOk: (value) => onOk?.(value as T),
  });
  const undo = (run: Write<null>): ToastAction => ({
    label: t('editor.undo'),
    onClick: () => void runAction({ key: `comment-undo:${c.source}:${c.id}`, ...write('comments.undo_failed', run) }),
  });
  const button = <T,>(label: Key, aria: Key, spec: Omit<ActionSpec<EngineResult<T>>, 'trigger'>): HTMLButtonElement => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'moss-btn moss-btn-secondary moss-btn--small';
    b.textContent = t(label);
    b.setAttribute('aria-label', t(aria, { author: c.author }));
    bindAction(b, spec);
    return b;
  };

  if (!c.hidden) {
    actions.append(
      button('comments.hide', 'comments.hide_label', write('comments.hide_failed', (f) => mod.hide(f, c.source, c.id), () =>
        showAck(t('comments.hidden'), { actions: [undo((f) => mod.unhide(f, c.source, c.id))] }),
      )),
      button('comments.spam', 'comments.spam_label', write('comments.spam_failed', (f) => mod.spam(f, c.source, c.id), (done) =>
        // Long enough to read several host names.
        showAck(spamLine(done), { duration: 12000, actions: [undo((f) => mod.undoSpam(f, done))] }),
      )),
    );
  } else if (c.directlyHidden) {
    actions.append(button('comments.show', 'comments.show_label', write('comments.show_failed', (f) => mod.unhide(f, c.source, c.id))));
  }
  li.append(head, text, actions);
  return li;
}

/** What Spam did: the hosts it blocked, those it left open, and how many other comments it hid. */
function spamLine(o: SpamOutcome): string {
  const parts = [t('comments.spam_done')];
  if (o.blocked.length) parts.push(t('comments.spam_blocked', { hosts: andList(o.blocked) }));
  if (o.notBlocked.length) parts.push(t('comments.spam_not_blocked', { hosts: andList(o.notBlocked) }));
  if (o.alsoHidden) parts.push(t(o.alsoHidden === 1 ? 'comments.spam_more_one' : 'comments.spam_more_other', { count: o.alsoHidden }));
  return parts.join(' · ');
}

function span(className: string, text: string): HTMLSpanElement {
  const s = document.createElement('span');
  if (className) s.className = className;
  s.textContent = text;
  return s;
}
