import { describe, it, expect } from 'vitest';
import { CurrentFileHold, splitFilePath, MIN_HOLD_MS } from '../upload-file';

describe('splitFilePath', () => {
  it('splits at the last separator so the directory can recede', () => {
    expect(splitFilePath('assets/video/harvest.mp4')).toEqual({
      dir: 'assets/video/',
      base: 'harvest.mp4',
    });
  });

  it('gives an empty directory for a file at the site root', () => {
    expect(splitFilePath('index.html')).toEqual({ dir: '', base: 'index.html' });
  });
});

describe('CurrentFileHold', () => {
  it('holds a winner for the minimum even as the backend keeps picking', () => {
    // The reason this class exists: `deploy.rs` uploads 20 files at once and
    // reports the largest at 4 Hz, so near the tail the pick can change on
    // every tick. Text that changes four times a second cannot be read, and
    // flashing content is an accessibility problem besides.
    const hold = new CurrentFileHold();
    expect(hold.offer('a/big.mp4', 0)).toBe('a/big.mp4');
    expect(hold.offer('b/small.css', 100)).toBe('a/big.mp4');
    expect(hold.offer('c/other.js', MIN_HOLD_MS - 1)).toBe('a/big.mp4');
  });

  it('lets the next pick through once the hold expires', () => {
    const hold = new CurrentFileHold();
    hold.offer('a/big.mp4', 0);
    expect(hold.offer('b/next.mp4', MIN_HOLD_MS)).toBe('b/next.mp4');
    // …and the new one starts its own hold, rather than inheriting the old
    // one's clock and being replaceable immediately.
    expect(hold.offer('c/third.mp4', MIN_HOLD_MS + 1)).toBe('b/next.mp4');
  });

  it('clears immediately when nothing is in flight', () => {
    // Not held: a filename kept after its upload finished claims work is
    // happening that isn't, which the author has no way to check.
    const hold = new CurrentFileHold();
    hold.offer('a/big.mp4', 0);
    expect(hold.offer(null, 1)).toBeNull();
    expect(hold.current()).toBeNull();
  });

  it('forgets the last file on reset, so a new pass starts blank', () => {
    const hold = new CurrentFileHold();
    hold.offer('a/big.mp4', 0);
    hold.reset();
    expect(hold.current()).toBeNull();
    // Reset also clears the clock — the first pick of the new pass wins now,
    // not 700 ms from the previous pass's stamp.
    expect(hold.offer('b/new.mp4', 1)).toBe('b/new.mp4');
  });
});
