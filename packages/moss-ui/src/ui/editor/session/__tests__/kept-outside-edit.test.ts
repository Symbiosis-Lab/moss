import { afterEach, expect, test, vi } from 'vitest';
import { ChangeSet } from '@codemirror/state';

const toasts: Array<{ actions?: Array<{ onClick: () => void }> }> = [];
vi.mock('../../../shared/ui/toast/toast-manager', () => ({
  showToast: (_msg: unknown, opts: { actions?: Array<{ onClick: () => void }> }) => toasts.push(opts),
  removeToastById: () => {},
}));

import { createKeptOutsideEdit } from '../kept-outside-edit';
import { setHold } from '../../hold';

afterEach(() => {
  setHold('publish', false);
  toasts.length = 0;
});

function setup() {
  let body = 'mine';
  const values: Record<string, unknown> = { title: 'Mine' };
  const applyOwnEdit = vi.fn((c: ChangeSet) => { body = c.apply(body as never).toString(); });
  const setField = vi.fn((k: string, v: unknown) => { values[k] = v; });
  const kept = createKeptOutsideEdit({
    currentEntryId: () => 1,
    editor: () => ({ getContent: () => body, applyOwnEdit }),
    store: () => ({ getValues: () => ({ ...values }), setField, removeField: vi.fn() }),
  });
  kept.remember({ body: 'mine', frontmatter: { title: 'Mine' } }, { body: 'theirs', frontmatter: { title: 'Theirs' } });
  kept.announceKept('/site/page.md');
  return { undo: toasts[0].actions![0].onClick, applyOwnEdit, setField };
}

test('Undo during a hold writes nothing, neither the text nor the pills', () => {
  const { undo, applyOwnEdit, setField } = setup();
  setHold('publish', true);
  undo();
  expect(applyOwnEdit).not.toHaveBeenCalled();
  expect(setField).not.toHaveBeenCalled();
});

test('Undo outside a hold puts the other writer\'s text and pills back', () => {
  const { undo, applyOwnEdit, setField } = setup();
  undo();
  expect(applyOwnEdit).toHaveBeenCalledTimes(1);
  expect(setField).toHaveBeenCalledWith('title', 'Theirs');
});
