import { describe, test, expect, beforeEach } from 'vitest';
import { bumpAssetBust, assetBustQuery, __resetAssetBustForTests } from '../asset-bust';

describe('asset-bust', () => {
  beforeEach(() => __resetAssetBustForTests());

  test('no bust before any bump → empty query', () => {
    expect(assetBustQuery('/assets/x.jpg')).toBe('');
  });

  test('bump makes a matching request-url return a ?_t= query', () => {
    // AssetReady.path is site-root-relative WITHOUT a leading slash.
    bumpAssetBust('assets/x.jpg');
    expect(assetBustQuery('/assets/x.jpg')).toMatch(/^\?_t=\d+$/);
  });

  test('bump only affects the matching url', () => {
    bumpAssetBust('assets/x.jpg');
    expect(assetBustQuery('/assets/y.jpg')).toBe('');
  });

  test('re-bumping the same asset advances the token (forces a fresh fetch)', () => {
    bumpAssetBust('assets/x.jpg');
    const first = assetBustQuery('/assets/x.jpg');
    bumpAssetBust('assets/x.jpg');
    const second = assetBustQuery('/assets/x.jpg');
    expect(second).not.toBe(first);
  });

  test('assetBustQuery ignores an existing query and returns the registry token', () => {
    // Bump TWICE so the stored token is 2 — distinct from the `?_t=1` already on
    // the URL. This proves the lookup strips the input query AND returns the
    // registry's token, not an echo of the input.
    bumpAssetBust('assets/x.jpg');
    bumpAssetBust('assets/x.jpg');
    expect(assetBustQuery('/assets/x.jpg?_t=1')).toBe('?_t=2');
  });

  // ── SourceAssetChanged path ───────────────────────────────────────────────
  // SourceAssetChanged.path arrives WITH a leading slash (e.g. `/图片/摄影/X.jpeg`),
  // unlike AssetReady.path which has no leading slash. Verify bumpAssetBust
  // handles the leading-slash form correctly so the coordinator's handler works.

  test('SourceAssetChanged-style path (leading slash) bumps the bust token', () => {
    // path from SourceAssetChanged has a leading slash already
    bumpAssetBust('/图片/摄影/X.jpeg');
    const q = assetBustQuery('/图片/摄影/X.jpeg');
    expect(q).toMatch(/^\?_t=\d+$/);
  });

  test('SourceAssetChanged path matches assetBustQuery for the same request_url', () => {
    // Simulates: the watcher emits SourceAssetChanged { path: '/assets/photo.jpg' }
    // and the editor calls assetBustQuery('/assets/photo.jpg') on the resolved URL.
    bumpAssetBust('/assets/photo.jpg');
    const query = assetBustQuery('/assets/photo.jpg');
    expect(query).not.toBe('');
    expect(query).toMatch(/^\?_t=\d+$/);
  });
});
