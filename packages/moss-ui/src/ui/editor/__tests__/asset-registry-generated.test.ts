import { describe, test, expect } from 'vitest';
import { lookupAsset, canEmbed, acceptOnDrop } from '../asset-registry.generated';

describe('generated asset registry', () => {
  test('avif is an embeddable image', () => {
    const a = lookupAsset('avif');
    expect(a?.kind).toBe('Image');
    expect(a?.canEmbed).toBe(true);
  });
  test('canEmbed/acceptOnDrop accept full paths', () => {
    expect(canEmbed('foo/bar.mp4')).toBe(true);
    expect(acceptOnDrop('clip.avi')).toBe(false); // not web-playable
    expect(canEmbed('notes.docx')).toBe(false);   // unknown
  });
});
