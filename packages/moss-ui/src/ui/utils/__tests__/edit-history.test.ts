import { describe, it, expect, vi, beforeEach } from 'vitest';
import { onMossSignal } from '../moss-events';
import { initEditHistoryMenu } from '../edit-history';

vi.mock('../moss-events', () => ({ onMossSignal: vi.fn() }));

// The menu broadcasts to EVERY webview, so the contract under test is: answer
// in the focused one, with the surface that owns the change, and nowhere else.

type Handlers = Record<string, () => void>;

function wire(): Handlers {
  const handlers: Handlers = {};
  vi.mocked(onMossSignal).mockImplementation((name: string, cb: () => void) => {
    handlers[name] = cb;
    return Promise.resolve(() => {});
  });
  return handlers;
}

describe('edit-history menu routing', () => {
  let execCommand: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    execCommand = vi.fn();
    document.execCommand = execCommand as unknown as typeof document.execCommand;
    vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  });

  it('gives undo to the surface that claims it', () => {
    const handlers = wire();
    const surface = vi.fn().mockReturnValue(true);
    initEditHistoryMenu(surface);

    handlers['menu:edit-undo']();
    handlers['menu:edit-redo']();

    expect(surface.mock.calls).toEqual([['undo'], ['redo']]);
    // The document's own history answered — no second, native attempt.
    expect(execCommand).not.toHaveBeenCalled();
  });

  it('falls through to the focused field when no surface claims it', () => {
    const handlers = wire();
    initEditHistoryMenu(vi.fn().mockReturnValue(false));

    handlers['menu:edit-undo']();

    expect(execCommand).toHaveBeenCalledWith('undo');
  });

  it('works with no surface at all (shell, launcher)', () => {
    const handlers = wire();
    initEditHistoryMenu();

    handlers['menu:edit-redo']();

    expect(execCommand).toHaveBeenCalledWith('redo');
  });

  it('ignores the broadcast in an unfocused webview', () => {
    const handlers = wire();
    vi.mocked(document.hasFocus).mockReturnValue(false);
    const surface = vi.fn().mockReturnValue(true);
    initEditHistoryMenu(surface);

    handlers['menu:edit-undo']();

    // A background editor rewinding its own document would be data loss the
    // author never asked for and cannot see happen.
    expect(surface).not.toHaveBeenCalled();
    expect(execCommand).not.toHaveBeenCalled();
  });
});
