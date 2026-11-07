import { describe, test, expect, vi, beforeEach } from 'vitest';

import { host } from '../../host';
import { openExternal } from '../open-external';

const open = vi.fn().mockResolvedValue(undefined);
host.openExternal = open;

beforeEach(() => open.mockClear());

describe('openExternal', () => {
  test('hands an http(s) URL to the host', async () => {
    await openExternal('https://example.com/a');
    await openExternal('http://example.com/b');
    expect(open.mock.calls).toEqual([['https://example.com/a'], ['http://example.com/b']]);
  });

  test.each(['file:///etc/passwd', 'javascript:alert(1)', 'command:workbench.action.quit', 'ftp://host/x', 'moss-source://x', ''])('never hands %j to the host', async (url) => {
    await openExternal(url);
    expect(open).not.toHaveBeenCalled();
  });

  test('rejects when the host could not open it', async () => {
    open.mockRejectedValueOnce(new Error('blocked'));
    await expect(openExternal('https://example.com')).rejects.toThrow('blocked');
  });
});
