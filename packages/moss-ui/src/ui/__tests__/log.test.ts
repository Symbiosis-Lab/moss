import { describe, it, expect, vi, afterEach } from 'vitest';
import type { LogLevel } from '../host';

type HostLog = (level: LogLevel, message: string) => Promise<void>;

// log.ts binds the console at load, so each test spies first, then loads the
// log and the host it reads from one fresh module graph. test-setup mocks the
// log globally; unmocked here so the console bridge forwards to the real one.
async function load(hostLog?: HostLog) {
  vi.resetModules();
  vi.doUnmock('../log');
  const { host } = await import('../host');
  if (hostLog) host.log = hostLog;
  const log = await import('../log');
  return { host, ...log };
}

describe('the UI log', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('writes to the console in a host that keeps no log', async () => {
    const console_ = vi.spyOn(console, 'info').mockImplementation(() => {});
    const { info } = await load();
    await info('hello');
    expect(console_).toHaveBeenCalledWith('hello');
  });

  it("writes to the host's log where it keeps one", async () => {
    const hostLog = vi.fn().mockResolvedValue(undefined);
    const console_ = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { error } = await load(hostLog);
    await error('boom');
    expect(hostLog).toHaveBeenCalledWith('error', 'boom');
    expect(console_).not.toHaveBeenCalled();
  });

  it("falls back to the console when the host's log fails, and never throws", async () => {
    const console_ = vi.spyOn(console, 'info').mockImplementation(() => {});
    const { info } = await load(vi.fn().mockRejectedValue(new Error('no runtime')));
    await expect(info('kept')).resolves.toBeUndefined();
    expect(console_).toHaveBeenCalledWith('kept');
  });

  it('a failing host log does not loop through the console bridge', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const original = console.error;
    const hostLog = vi.fn().mockRejectedValue(new Error('no runtime'));
    const { error } = await load(hostLog);
    const bridge = await import('../lib/install-log-bridge');
    bridge.installLogBridge('test');
    try {
      await error('once');
      for (let i = 0; i < 20; i++) await Promise.resolve();
      expect(hostLog).toHaveBeenCalledTimes(1);
    } finally {
      bridge.resetForTests();
      console.error = original;
    }
  });
});
