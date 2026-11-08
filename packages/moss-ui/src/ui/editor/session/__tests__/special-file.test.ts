/**
 * The gate decision for a file with a special role: when the dialog is shown,
 * what each answer does to the per-kind memory. Pure logic; the dialog itself
 * and its wiring into the editor are covered in special-file-modal.test.ts and
 * editor-main.test.ts.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { SpecialFileKind as BindingKind } from '@symbiosis-lab/moss-ui';
import { isMuted, mayOpenSpecialFile, type SpecialFileKind } from '../special-file';

// The local union and moss's generated one must be the same set.
const sameKinds: [SpecialFileKind] extends [BindingKind] ? ([BindingKind] extends [SpecialFileKind] ? true : never) : never = true;

describe('mayOpenSpecialFile', () => {
  beforeEach(() => localStorage.clear());

  it('opens a file with no kind without asking', async () => {
    const ask = vi.fn();
    expect(await mayOpenSpecialFile(null, ask)).toBe(true);
    expect(await mayOpenSpecialFile(undefined, ask)).toBe(true);
    expect(ask).not.toHaveBeenCalled();
  });

  it('asks for a kind that is not muted', async () => {
    const ask = vi.fn().mockResolvedValue({ open: true, dontAskAgain: false });
    expect(await mayOpenSpecialFile('footer', ask)).toBe(true);
    expect(ask).toHaveBeenCalledWith('footer');
    expect(isMuted('footer')).toBe(false);
  });

  it('Cancel opens nothing and mutes nothing, even with the box ticked', async () => {
    const ask = vi.fn().mockResolvedValue({ open: false, dontAskAgain: true });
    expect(await mayOpenSpecialFile('not-found', ask)).toBe(false);
    expect(isMuted('not-found')).toBe(false);
  });

  it('Open with the box ticked mutes that kind only, and the next open skips the dialog', async () => {
    const ask = vi.fn().mockResolvedValue({ open: true, dontAskAgain: true });
    expect(await mayOpenSpecialFile('welcome-email', ask)).toBe(true);
    expect(isMuted('welcome-email')).toBe(true);
    expect(isMuted('footer')).toBe(false);

    ask.mockClear();
    expect(await mayOpenSpecialFile('welcome-email', ask)).toBe(true);
    expect(ask).not.toHaveBeenCalled();
    expect(await mayOpenSpecialFile('footer', ask)).toBe(true);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('keeps the kinds pinned to the bindings', () => {
    expect(sameKinds).toBe(true);
  });
});
