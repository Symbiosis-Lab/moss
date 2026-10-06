import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { createPaper } from '../default.js';

describe('createPaper', () => {
  // The bytes the landing page's paper generator has always produced. A change
  // to the noise recipe changes how every wash looks, so it has to be a
  // deliberate update of this hash and not a side effect.
  it('reproduces the default sheet byte for byte', () => {
    const paper = createPaper();
    expect(paper.width).toBe(256);
    expect(paper.height).toBe(256);
    expect(paper.data.byteLength).toBe(256 * 256 * 4);
    expect(createHash('sha256').update(paper.data as Uint8Array).digest('hex')).toBe('eadbefea1b4737e384add70e6d44140d51bdf8457be84c6015e82d5756b6e926');
  });

  it('is deterministic for a given seed and differs between seeds', () => {
    const a = createPaper({ seed: 1 }), b = createPaper({ seed: 1 }), c = createPaper({ seed: 2 });
    expect(Buffer.compare(Buffer.from(a.data as Uint8Array), Buffer.from(b.data as Uint8Array))).toBe(0);
    expect(Buffer.compare(Buffer.from(a.data as Uint8Array), Buffer.from(c.data as Uint8Array))).not.toBe(0);
  });
});
