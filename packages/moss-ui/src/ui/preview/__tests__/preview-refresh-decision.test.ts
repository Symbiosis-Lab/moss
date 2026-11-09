import { describe, test, expect } from 'vitest';
import { formatRefreshDecisionLine, type RefreshDecisionContext, type RefreshTrigger, type RefreshOutcome } from '../preview-refresh-decision';

describe('formatRefreshDecisionLine', () => {
  const ctx = (overrides: Partial<RefreshDecisionContext>): RefreshDecisionContext => ({
    action: 'refresh',
    path: 'blog/post.html',
    changedCount: 1,
    ...overrides,
  });

  test.each([
    [
      'file-changed' as RefreshTrigger,
      ctx({ action: 'refresh', path: 'blog/post.html', changedCount: 1 }),
      'morph-applied' as RefreshOutcome,
      '[preview-refresh] trigger=file-changed action=refresh path=blog/post.html changed=1 outcome=morph-applied',
    ],
    [
      'file-changed' as RefreshTrigger,
      ctx({ action: 'none', path: 'blog/post.html', changedCount: 1 }),
      'none(page unaffected)' as RefreshOutcome,
      '[preview-refresh] trigger=file-changed action=none path=blog/post.html changed=1 outcome=none(page unaffected)',
    ],
    [
      'other' as RefreshTrigger,
      ctx({ action: 'refresh', path: null, changedCount: 0 }),
      'skipped(no page)' as RefreshOutcome,
      '[preview-refresh] trigger=other action=refresh path=null changed=0 outcome=skipped(no page)',
    ],
    [
      'newPageWait-recheck' as RefreshTrigger,
      ctx({ action: 'recheck', path: '/notes/untitled/', changedCount: 0 }),
      'recheck(1/3)' as RefreshOutcome,
      '[preview-refresh] trigger=newPageWait-recheck action=recheck path=/notes/untitled/ changed=0 outcome=recheck(1/3)',
    ],
    [
      'newPageWait-recheck' as RefreshTrigger,
      ctx({ action: 'give-up', path: '/notes/untitled/', changedCount: 0 }),
      'wait-ended' as RefreshOutcome,
      '[preview-refresh] trigger=newPageWait-recheck action=give-up path=/notes/untitled/ changed=0 outcome=wait-ended',
    ],
  ])('trigger=%s outcome=%s renders the exact line', (trigger, decisionCtx, outcome, expected) => {
    expect(formatRefreshDecisionLine(trigger, decisionCtx, outcome)).toBe(expected);
  });

  test('path=null renders the literal string "null", not the JS value', () => {
    const line = formatRefreshDecisionLine('other', ctx({ path: null }), 'skipped(no page)');
    expect(line).toContain('path=null ');
  });
});
