import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { search } from '@codemirror/search';
import { CmFindController } from '../cm-find';

function makeView(doc: string): { view: EditorView; container: HTMLElement } {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const state = EditorState.create({
    doc,
    extensions: [search({ top: false })],
  });
  const view = new EditorView({ state, parent: container });
  return { view, container };
}

describe('CmFindController', () => {
  let view: EditorView;
  let container: HTMLElement;
  let ctrl: CmFindController;

  beforeEach(() => {
    ({ view, container } = makeView('hello world\nhello again\ngoodbye world'));
    ctrl = new CmFindController();
  });

  afterEach(() => {
    ctrl.close();
    view.destroy();
    document.body.innerHTML = '';
  });

  it('open() appends the pill element to the container', () => {
    ctrl.open(view, container);
    expect(container.querySelector('.find-pill')).not.toBeNull();
  });

  it('open() is idempotent — calling twice does not create two pills', () => {
    ctrl.open(view, container);
    ctrl.open(view, container);
    expect(container.querySelectorAll('.find-pill').length).toBe(1);
  });

  it('close() removes the pill from the DOM', () => {
    ctrl.open(view, container);
    ctrl.close();
    expect(container.querySelector('.find-pill')).toBeNull();
  });

  it('isOpen() reflects state', () => {
    expect(ctrl.isOpen()).toBe(false);
    ctrl.open(view, container);
    expect(ctrl.isOpen()).toBe(true);
    ctrl.close();
    expect(ctrl.isOpen()).toBe(false);
  });

  it('countMatches returns correct count for a known query', () => {
    ctrl.open(view, container);
    const count = ctrl.countMatches(view, 'hello');
    expect(count).toBe(2);
  });

  it('countMatches returns 0 for a query with no matches', () => {
    ctrl.open(view, container);
    const count = ctrl.countMatches(view, 'zzz');
    expect(count).toBe(0);
  });

  it('countMatches is case-insensitive', () => {
    ctrl.open(view, container);
    expect(ctrl.countMatches(view, 'HELLO')).toBe(2);
  });
});
