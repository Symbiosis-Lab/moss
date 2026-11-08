import { describe, test, expect } from 'vitest';
import { resolveCachedImage } from '../cm-editor';
import type { EditorReferenceResolution } from '@symbiosis-lab/moss-ui';

function imageEnv(requestUrl: string): EditorReferenceResolution {
  return {
    target: 'x.jpg', is_embed: true,
    kind: { kind: 'image' } as any,
    resolved: {
      request_url: requestUrl,
      mime_type: 'image/jpeg',
      width: 800, height: 600,
    } as any,
    url: null, anchor: null, resolved_path: null,
    message: null, candidates: [],
    folder: null,
  };
}

describe('resolveCachedImage', () => {
  test('builds moss-source:// URL from request_url (no bust by default)', () => {
    const img = resolveCachedImage(imageEnv('/assets/x.jpg'));
    expect(img?.url).toBe('moss-source://localhost/assets/x.jpg');
  });

  test('appends a supplied bust query so the URL identity changes', () => {
    const img = resolveCachedImage(imageEnv('/assets/x.jpg'), '?_t=7');
    expect(img?.url).toBe('moss-source://localhost/assets/x.jpg?_t=7');
  });

  test('percent-encodes non-ASCII path segments', () => {
    const img = resolveCachedImage(imageEnv('/图片/摄影/X.jpeg'));
    expect(img?.url).toBe('moss-source://localhost/%E5%9B%BE%E7%89%87/%E6%91%84%E5%BD%B1/X.jpeg');
  });

  test('still returns undefined for a non-image mime', () => {
    const env = imageEnv('/assets/x.jpg');
    (env.resolved as any).mime_type = 'application/pdf';
    expect(resolveCachedImage(env, '?_t=7')).toBeUndefined();
  });

  test('returns undefined when env is undefined', () => {
    expect(resolveCachedImage(undefined)).toBeUndefined();
  });

  test('returns undefined — never throws — when the envelope has no resolved asset', () => {
    // Live-preview crash guard: an unresolved / non-asset envelope reaches here
    // on every keystroke while the async resolve batch is in flight.
    expect(() => resolveCachedImage({ resolved: null } as any)).not.toThrow();
    expect(resolveCachedImage({ resolved: null } as any)).toBeUndefined();
  });
});
