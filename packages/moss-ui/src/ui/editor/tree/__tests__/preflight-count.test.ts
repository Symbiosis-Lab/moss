import { describe, expect, it } from 'vitest';
import { PreflightEvidence } from '../model';

describe('preflightCountForPath', () => {
  it('keeps duplicate source occurrences and derives enclosing folder counts', () => {
    const evidence = new PreflightEvidence();
    evidence.replace(['posts/a.md', 'posts/a.md', 'posts/nested/b.md', 'about.md']);
    expect(evidence.count('posts/a.md')).toBe(2);
    expect(evidence.count('posts')).toBe(3);
    expect(evidence.count('')).toBe(4);
  });

  it('does not confuse a sibling prefix for a descendant folder', () => {
    const evidence = new PreflightEvidence();
    evidence.replace(['posts-two/a.md']);
    expect(evidence.count('posts')).toBe(0);
  });
});
