import { describe, it, expect, vi, beforeEach } from 'vitest';

import { host } from "../../../host";
import { sourceTargetPosition, deliverEditorPosition } from '../editor-position';

const emit = vi.fn();
host.bus = { ...host.bus, emit };

describe('sourceTargetPosition', () => {
  it('maps a nav frontmatter target to a seeded field position', () => {
    expect(sourceTargetPosition({ kind: 'frontmatter', field: 'nav' }))
      .toStrictEqual({ kind: 'field', field: 'nav', seed: true });
  });

  it('maps any other frontmatter target to an unseeded field position', () => {
    expect(sourceTargetPosition({ kind: 'frontmatter', field: 'title' }))
      .toStrictEqual({ kind: 'field', field: 'title' });
  });
});

describe('deliverEditorPosition', () => {
  beforeEach(() => emit.mockClear());

  it('emits editor:open-field carrying the seed', () => {
    deliverEditorPosition({ kind: 'field', field: 'nav', seed: true });
    expect(emit).toHaveBeenCalledWith('editor:open-field', { field: 'nav', seed: true });
  });

  it('emits editor:goto-rule carrying the selector', () => {
    deliverEditorPosition({ kind: 'rule', selector: '.moss-hero h1' });
    expect(emit).toHaveBeenCalledWith('editor:goto-rule', { selector: '.moss-hero h1' });
  });

  it('omits the seed key when the position has none', () => {
    deliverEditorPosition({ kind: 'field', field: 'title' });
    expect(emit.mock.calls[0][1]).toStrictEqual({ field: 'title' });
  });
});
