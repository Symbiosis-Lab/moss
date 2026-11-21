/**
 * The shell attaches to the server `moss edit` already runs: its "build" is
 * that server's preview, at the address the server names for this browser.
 */
import { afterEach, describe, it, expect, vi } from 'vitest';
import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { setLayout, shellHost } from '../shell-host';
import { mountFrames } from '../frames';

const carrier = vi.hoisted(() => {
  const handlers = new Map<string, Set<(p: unknown) => void>>();
  return { handlers, emit: (name: string, p: unknown) => handlers.get(name)?.forEach((h) => h(p)) };
});
vi.mock('../carrier', () => ({
  on: vi.fn(async (name: string, handler: (p: unknown) => void) => {
    const set = carrier.handlers.get(name) ?? new Set();
    carrier.handlers.set(name, set.add(handler));
    return () => set.delete(handler);
  }),
}));

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

  it("passes each rebuild's progress on, as the app's build channel does", async () => {
    const own = shellHost({ preview_url: 'http://127.0.0.1:4000', public_url: null }, { path: '/s', name: null });
    const updates: ProgressUpdate[] = [];
    await own.build!('/s', true, (u) => updates.push(u));
    const rebuilt = { step: 'build', message: 'Build complete', percentage: 100, completed: true, port: 4000, is_empty: false };
    carrier.emit('build-progress', rebuilt);
    expect(updates.at(-1)).toEqual(rebuilt);
  });

  it('gives a rebuild\'s progress only to the latest build, as each app build has its own channel', async () => {
    const own = shellHost({ preview_url: 'http://127.0.0.1:4000', public_url: null }, { path: '/s', name: null });
    const first: ProgressUpdate[] = [];
    const second: ProgressUpdate[] = [];
    await own.build!('/s', true, (u) => first.push(u));
    await own.build!('/s', true, (u) => second.push(u));
    const rebuilt = { step: 'build', message: 'Build complete', percentage: 100, completed: true, port: 4000, is_empty: false };
    carrier.emit('build-progress', rebuilt);
    expect(first).not.toContainEqual(rebuilt);
    expect(second.at(-1)).toEqual(rebuilt);
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
