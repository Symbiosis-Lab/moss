import { describe, it, test, expect } from 'vitest';
import { stripWikilinkBrackets, wikilinkTarget, wrapWikilink, wrapEmbedWikilink } from '../wikilink-syntax.js';

describe('stripWikilinkBrackets', () => {
  it('removes [[ ]] delimiters, keeping the inner text', () => {
    expect(stripWikilinkBrackets('[[DSCF4053.jpeg]]')).toBe('DSCF4053.jpeg');
  });
  it('removes the embed ![[ ]] delimiters too', () => {
    expect(stripWikilinkBrackets('![[DSCF4053.jpeg]]')).toBe('DSCF4053.jpeg');
  });
  it('keeps an alias/pothole inside the brackets (display round-trip)', () => {
    expect(stripWikilinkBrackets('[[News|Latest]]')).toBe('News|Latest');
  });
  it('passes a bare path through unchanged', () => {
    expect(stripWikilinkBrackets('assets/photo.png')).toBe('assets/photo.png');
  });
  it('trims surrounding whitespace', () => {
    expect(stripWikilinkBrackets('  [[a]] ')).toBe('a');
  });
});

describe('wikilinkTarget', () => {
  it('unwraps an Obsidian cover ref to the bare asset path', () => {
    expect(wikilinkTarget('[[DSCF4053.jpeg]]')).toBe('DSCF4053.jpeg');
  });
  it('drops the |alias / pothole, returning the path part', () => {
    expect(wikilinkTarget('[[DSCF4053.jpeg|600x400]]')).toBe('DSCF4053.jpeg');
    expect(wikilinkTarget('[[文明的代价|悲伤的石狮子]]')).toBe('文明的代价');
  });
  it('drops a #anchor', () => {
    expect(wikilinkTarget('[[note#section]]')).toBe('note');
  });
  it('handles the embed prefix', () => {
    expect(wikilinkTarget('![[clip.mp4]]')).toBe('clip.mp4');
  });
  it('passes a bare path through unchanged', () => {
    expect(wikilinkTarget('assets/photo.png')).toBe('assets/photo.png');
  });
});

describe('wrapWikilink', () => {
  it('wraps a bare reference', () => {
    expect(wrapWikilink('News')).toBe('[[News]]');
  });
  it('is idempotent for an already-wrapped reference', () => {
    expect(wrapWikilink('[[News]]')).toBe('[[News]]');
  });
  it('trims before wrapping', () => {
    expect(wrapWikilink('  News ')).toBe('[[News]]');
  });
});

describe('wrapEmbedWikilink', () => {
  test('bare wikilink, no encoding', () => {
    expect(wrapEmbedWikilink('My Photo.png')).toBe('![[My Photo.png]]');
  });
  test('folder embed keeps trailing slash', () => {
    expect(wrapEmbedWikilink('webapp/')).toBe('![[webapp/]]');
  });
});
