/**
 * The shell attaches to the server `moss edit` already runs: its "build" is
 * that server's preview, at the address the server names for this browser.
 */
import { afterEach, describe, it, expect, vi } from 'vitest';
import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { setLayout, shellHost } from '../shell-host';
import { mountFrames } from '../frames';

describe("the shell's host members", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.getElementById('moss-edit-button')?.remove();
  });

  it('reports the running preview, at the address the server names', async () => {
    const own = shellHost(
      { preview_url: 'https://edit.example.test:444', public_url: 'https://edit.example.test' },
      { path: '/srv/site', name: 'site' },
    );
    expect(own.previewUrl!()).toBe('https://edit.example.test:444');
    expect(own.openedFolder!()).toMatchObject({ folder_path: '/srv/site', folder_name: 'site' });
    const updates: ProgressUpdate[] = [];
    await own.build!('/srv/site', true, (u) => updates.push(u));
    expect(updates).toEqual([expect.objectContaining({ completed: true, percentage: 100, port: 444, is_empty: false })]);
  });

  it('takes the default port of an address that names none', async () => {
    const own = shellHost({ preview_url: 'https://edit.example.test', public_url: null }, { path: '/s', name: null });
    const update = vi.fn();
    await own.build!('/s', true, update);
    expect(update.mock.calls[0][0].port).toBe(443);
  });

  it('marks the edit button pressed while the editor shows', () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })));
    mountFrames(document.createElement('div'));
    const button = document.createElement('button');
    button.id = 'moss-edit-button';
    document.body.append(button);
    setLayout('split');
    expect(button.getAttribute('aria-pressed')).toBe('true');
    setLayout('closed');
    expect(button.getAttribute('aria-pressed')).toBe('false');
  });
});
