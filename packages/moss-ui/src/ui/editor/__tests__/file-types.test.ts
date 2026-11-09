import { describe, test, expect } from 'vitest';
import { lookupAsset } from '../asset-registry.generated';
import { detectFileCategory } from '../file-types';

describe('file-types registry alignment', () => {
  // Verify registry shapes are correct (these tests depend on asset-registry.generated.ts).
  test('image detection derives from registry', () => {
    expect(!!lookupAsset('avif') && lookupAsset('avif')!.kind === 'Image').toBe(true);
  });

  test('ogg is Audio (intentional change: was Video in VIDEO_EXTS → tree icon flips Video→Audio)', () => {
    expect(lookupAsset('ogg')?.kind).toBe('Audio'); // matches ext_kind.rs:27, fixes the file-types divergence
  });

  // These tests verify that detectFileCategory itself routes through the registry.
  test('detectFileCategory returns audio for ogg (was video in old hand-maintained set)', () => {
    expect(detectFileCategory('test.ogg')).toBe('audio');
  });

  test('detectFileCategory returns image for avif', () => {
    expect(detectFileCategory('test.avif')).toBe('image');
  });

  test('detectFileCategory returns audio for aac', () => {
    expect(detectFileCategory('clip.aac')).toBe('audio');
  });

  test('detectFileCategory returns video for mp4 (still valid)', () => {
    expect(detectFileCategory('video.mp4')).toBe('video');
  });

  test('detectFileCategory returns image for png (still valid)', () => {
    expect(detectFileCategory('photo.png')).toBe('image');
  });

  test('detectFileCategory returns audio for mp3 (still valid)', () => {
    expect(detectFileCategory('music.mp3')).toBe('audio');
  });

  test('detectFileCategory text/font/pdf remain unchanged (viewer-only concerns)', () => {
    expect(detectFileCategory('notes.txt')).toBe('text');
    expect(detectFileCategory('font.ttf')).toBe('font');
    expect(detectFileCategory('doc.pdf')).toBe('pdf');
  });

  test('detectFileCategory binary for unknown extensions', () => {
    expect(detectFileCategory('archive.zip')).toBe('binary');
  });

  // Regression: bmp/ico/tiff were in old IMAGE_EXTS but absent from the initial registry,
  // causing them to fall through to 'binary' after the refactor. They must remain 'image'
  // for the file viewer (can_embed:false so no <img> is emitted in build).
  test('detectFileCategory returns image for bmp (viewer-only, was lost in registry refactor)', () => {
    expect(detectFileCategory('icon.bmp')).toBe('image');
    expect(lookupAsset('bmp')?.kind).toBe('Image');
    expect(lookupAsset('bmp')?.canEmbed).toBe(false);
    expect(lookupAsset('bmp')?.acceptOnDrop).toBe(false);
  });

  test('detectFileCategory returns image for ico (viewer-only, was lost in registry refactor)', () => {
    expect(detectFileCategory('favicon.ico')).toBe('image');
    expect(lookupAsset('ico')?.kind).toBe('Image');
    expect(lookupAsset('ico')?.canEmbed).toBe(false);
    expect(lookupAsset('ico')?.acceptOnDrop).toBe(false);
  });

  test('detectFileCategory returns image for tiff (viewer-only, was lost in registry refactor)', () => {
    expect(detectFileCategory('screenshot.tiff')).toBe('image');
    expect(lookupAsset('tiff')?.kind).toBe('Image');
    expect(lookupAsset('tiff')?.canEmbed).toBe(false);
    expect(lookupAsset('tiff')?.acceptOnDrop).toBe(false);
  });
});
