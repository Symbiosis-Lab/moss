import { describe, expect, it } from 'vitest';
import { sha256Lower, sourceSpanToBodyRange, utf8ByteToUtf16 } from '../cm-reveal-flash';

describe('publish preflight source coordinates', () => {
  it('converts UTF-8 byte spans before a target into CM UTF-16 offsets', () => {
    expect(utf8ByteToUtf16('前😀x', 7)).toBe(3);
    expect(utf8ByteToUtf16('前😀x', 2)).toBeNull();
  });

  it('removes CRLF frontmatter bytes at the sole source/body seam', () => {
    const raw = '---\r\ntitle: 前\r\n---\r\n😀lost';
    const start = new TextEncoder().encode('---\r\ntitle: 前\r\n---\r\n').length;
    expect(sourceSpanToBodyRange(raw, '😀lost', { start_byte: start + 4, end_byte: start + 8 })).toEqual({ from: 2, to: 6 });
  });

  it('normalizes CRLF body coordinates after locating the raw source bytes', () => {
    const raw = 'first\r\n前😀lost';
    const start = new TextEncoder().encode('first\r\n前').length;
    expect(sourceSpanToBodyRange(raw, 'first\n前😀lost', { start_byte: start, end_byte: start + 8 })).toEqual({ from: 7, to: 13 });
  });

  it('uses lowercase SHA-256 over the source UTF-8 bytes', async () => {
    await expect(sha256Lower('moss')).resolves.toBe('9e135ab954828446379a956e7e12eb7254a482059c765a4e055b617ffe624057');
  });
});
