import { describe, it, expect, afterEach } from 'vitest';
import { host } from "../../host";
import { sourceAssetUrl } from '../source-asset-url';

const kept = host.sourceAssetBase;
afterEach(() => {
  host.sourceAssetBase = kept;
});

describe('sourceAssetUrl', () => {
  it('appends the encoded path and bust to where the host serves sources', () => {
    host.sourceAssetBase = 'moss-source://localhost';
    expect(sourceAssetUrl('/assets/a.jpg')).toBe('moss-source://localhost/assets/a.jpg');
    expect(sourceAssetUrl('/图片/摄影/X.jpeg', '?_t=3'))
      .toBe('moss-source://localhost/%E5%9B%BE%E7%89%87/%E6%91%84%E5%BD%B1/X.jpeg?_t=3');
    expect(sourceAssetUrl('assets/no-slash.png')).toBe('moss-source://localhost/assets/no-slash.png');
  });

  it('follows a host that serves them under a path of its own page', () => {
    host.sourceAssetBase = '/__moss/source';
    expect(sourceAssetUrl('/图片/摄影/X.jpeg', '?_t=3'))
      .toBe('/__moss/source/%E5%9B%BE%E7%89%87/%E6%91%84%E5%BD%B1/X.jpeg?_t=3');
  });
});
