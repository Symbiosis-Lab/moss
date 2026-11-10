import { describe, it, expect, afterEach, vi } from 'vitest';
import { uiPlatform } from '../platform';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('uiPlatform', () => {
  it("reads 'other' where there is no navigator, as in Node 20", () => {
    vi.stubGlobal('navigator', undefined);
    expect(uiPlatform()).toBe('other');
  });

  it('reads the platform the browser reports', () => {
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    expect(uiPlatform()).toBe('mac');
    vi.stubGlobal('navigator', { platform: 'Win32' });
    expect(uiPlatform()).toBe('win');
  });
});
