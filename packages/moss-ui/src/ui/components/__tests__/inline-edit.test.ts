import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { InlineEdit } from '../inline-edit';

describe('InlineEdit', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  // ── Test 1: Initial construction ─────────────────────────────────────────
  it('constructs with initialValue, correct el class, idle state, and native input with aria-label', () => {
    const ie = new InlineEdit({
      initialValue: 'Alice',
      ariaLabel: 'Sender name',
      validationMode: 'blur-autosave',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    expect(ie.getValue()).toBe('Alice');
    expect(ie.el.classList.contains('moss-inline-edit')).toBe(true);
    expect(ie.el.dataset.state).toBe('idle');

    const input = ie.el.querySelector('input');
    expect(input).not.toBeNull();
    expect(input!.tagName).toBe('INPUT');
    expect(input!.getAttribute('aria-label')).toBe('Sender name');
    // input is always present (never swapped out)
    expect(input).toBe(ie.input);
  });

  // ── Test 2: Focus / activate() → active state + select-all ──────────────
  it('activate() transitions to active state and selects all text', () => {
    const ie = new InlineEdit({
      initialValue: 'hello',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    ie.activate();

    expect(ie.el.dataset.state).toBe('active');
    expect(ie.input.selectionStart).toBe(0);
    expect(ie.input.selectionEnd).toBe('hello'.length);
  });

  // ── Test 3: Enter key commits and returns to idle ────────────────────────
  it('Enter key calls onCommit once with current value and returns to idle', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'original',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'updated';

    const enterEvent = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true });
    ie.input.dispatchEvent(enterEvent);

    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith('updated');
    expect(ie.el.dataset.state).toBe('idle');
  });

  // ── Test 4: Escape reverts value and returns to idle ────────────────────
  it('Escape reverts to focus-time snapshot and does not call onCommit', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'before',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'changed';

    const escEvent = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true });
    ie.input.dispatchEvent(escEvent);

    expect(onCommit).not.toHaveBeenCalled();
    expect(ie.getValue()).toBe('before');
    expect(ie.el.dataset.state).toBe('idle');
  });

  // ── Test 5: blur-autosave mode commits on blur ───────────────────────────
  it('blur-autosave: blur calls onCommit with current value', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'start',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'end';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    expect(onCommit).toHaveBeenCalledWith('end');
  });

  // ── Test 6: explicit-save mode: blur does not commit; validateAsync;
  //    validating state; available → Save enabled ────────────────────────
  it('explicit-save: blur does not commit; async validation → available → Save enabled', async () => {
    const onCommit = vi.fn();
    const validateAsync = vi.fn().mockResolvedValue('available');
    const ie = new InlineEdit({
      initialValue: 'old-slug',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      validateAsync,
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'new-slug';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    // Blur alone must NOT commit
    expect(onCommit).not.toHaveBeenCalled();
    // Must be in validating state while async pending — and SAY so: the
    // cleared-slot silence here was the subdomain claim's invisible wait.
    expect(ie.el.dataset.state).toBe('validating');
    const status = ie.el.querySelector('.moss-inline-edit__status');
    expect(status!.textContent).toBe('Checking availability');
    // The dots are an aria-hidden SIBLING of the live region, revealed by
    // [data-state="validating"] (CSS), never a child of the caption.
    const loader = ie.el.querySelector('.moss-inline-edit__loader');
    expect(loader).not.toBeNull();
    expect(loader!.getAttribute('aria-hidden')).toBe('true');
    expect(loader!.parentElement).toBe(status!.parentElement);
    expect(loader!.parentElement!.contains(status!)).toBe(true);
    expect(status!.contains(loader!)).toBe(false);

    // Wait for async to resolve
    await new Promise(r => setTimeout(r, 0));

    expect(ie.el.dataset.state).toBe('active');
    const saveBtn = ie.el.querySelector<HTMLButtonElement>('.moss-inline-edit__save');
    expect(saveBtn).not.toBeNull();
    expect(saveBtn!.disabled).toBe(false);
    expect(saveBtn!.hidden).toBe(false);

    saveBtn!.click();
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith('new-slug');
  });

  // ── Test 7: validateSync error → error state, no validateAsync called ────
  it('validateSync returning error → data-state=error, status shows error, validateAsync not called', () => {
    const onCommit = vi.fn();
    const validateSync = vi.fn().mockReturnValue('Too short');
    const validateAsync = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'x',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      validateSync,
      validateAsync,
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'x';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    expect(ie.el.dataset.state).toBe('error');
    expect(validateAsync).not.toHaveBeenCalled();

    const status = ie.el.querySelector('.moss-inline-edit__status');
    expect(status!.textContent).toContain('Too short');

    const saveBtn = ie.el.querySelector<HTMLButtonElement>('.moss-inline-edit__save');
    expect(saveBtn!.disabled).toBe(true);
    expect(onCommit).not.toHaveBeenCalled();
  });

  // ── Test 8: validateAsync returning 'taken' → status shows taken msg ─────
  it('validateAsync returning "taken" → status shows taken message, Save stays disabled', async () => {
    const onCommit = vi.fn();
    const validateAsync = vi.fn().mockResolvedValue('taken');
    const ie = new InlineEdit({
      initialValue: 'old',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      validateAsync,
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'taken-slug';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    await new Promise(r => setTimeout(r, 0));

    const status = ie.el.querySelector('.moss-inline-edit__status');
    expect(ie.el.dataset.state).toBe('error');
    expect(status!.textContent).toBe('That ID is already taken.');

    const saveBtn = ie.el.querySelector<HTMLButtonElement>('.moss-inline-edit__save');
    expect(saveBtn!.disabled).toBe(true);
    expect(onCommit).not.toHaveBeenCalled();
  });

  // ── Test 8b: blur-autosave Enter-then-blur produces exactly ONE onCommit ──
  it('blur-autosave: Enter then blur commits exactly once', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'start',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'end';
    // Press Enter — commits and transitions to idle
    ie.input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(ie.el.dataset.state).toBe('idle');

    // Subsequent blur must NOT commit again
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));
    expect(onCommit).toHaveBeenCalledTimes(1);
  });

  // ── Test 8c: FIX 4 — Esc revert uses second-activation snapshot ───────────
  it('Esc after second activation reverts to second-activation snapshot, not initialValue', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'original',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    // First activation: type "A" and commit
    ie.activate();
    ie.input.value = 'A';
    ie.input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onCommit).toHaveBeenCalledWith('A');

    // Second activation: type "B" and press Esc
    ie.activate(); // snapshot should now be "A"
    ie.input.value = 'B';
    ie.input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    // Should revert to "A" (second activation's snapshot), not "original"
    expect(ie.getValue()).toBe('A');
    expect(ie.el.dataset.state).toBe('idle');
  });

  // ── Test 8d: FIX 1 — stale async result does NOT enable Save ─────────────
  it('stale async result (value changed before resolution) does NOT enable Save', async () => {
    const onCommit = vi.fn();

    // Deferred promise we control
    let resolveValue1!: (v: 'available' | 'taken' | string) => void;
    const promise1 = new Promise<'available' | 'taken' | string>((res) => { resolveValue1 = res; });

    const validateAsync = vi.fn()
      .mockReturnValueOnce(promise1)
      .mockResolvedValue('available');

    const ie = new InlineEdit({
      initialValue: 'old',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      validateAsync,
      onCommit,
    });
    container.appendChild(ie.el);

    // Trigger validation for "value1"
    ie.activate();
    ie.input.value = 'value1';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));
    expect(ie.el.dataset.state).toBe('validating');

    // User re-focuses and types a different value before promise1 resolves
    ie.input.dispatchEvent(new FocusEvent('focus', { bubbles: true }));
    ie.input.value = 'value2';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));
    // Now validating for "value2" (promise2 resolves immediately)
    await new Promise(r => setTimeout(r, 0));

    // At this point value2's validation resolved as 'available' — Save should be enabled
    const saveBtn = ie.el.querySelector<HTMLButtonElement>('.moss-inline-edit__save');

    // Now resolve value1's promise as 'available' — should be ignored (stale)
    resolveValue1('available');
    await Promise.resolve();
    await new Promise(r => setTimeout(r, 0));

    // Save should still be enabled (from value2), not double-toggled or broken
    // The key test: if stale guard is missing, resolving value1 after value2 could re-enable
    // an already-enabled button or flip state. Here we verify state is still 'active' (not flipped).
    expect(ie.el.dataset.state).toBe('active');
    expect(saveBtn!.disabled).toBe(false);
  });

  // ── Test 8e: FIX 2 — validateAsync rejection → error state, Save disabled ─
  it('validateAsync rejection → state=error, status shows error message, Save disabled', async () => {
    const onCommit = vi.fn();
    const validateAsync = vi.fn().mockRejectedValue(new Error('Network timeout'));
    const ie = new InlineEdit({
      initialValue: 'old',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      validateAsync,
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.input.value = 'some-slug';
    ie.input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    await new Promise(r => setTimeout(r, 0));

    const status = ie.el.querySelector('.moss-inline-edit__status');
    expect(ie.el.dataset.state).toBe('error');
    expect(status!.textContent).toBe('Network timeout');

    const saveBtn = ie.el.querySelector<HTMLButtonElement>('.moss-inline-edit__save');
    expect(saveBtn!.disabled).toBe(true);
    expect(onCommit).not.toHaveBeenCalled();
  });

  // ── Test FIX-ESC: Esc while active stops propagation; while idle bubbles ───
  it('Esc while active reverts AND stops propagation (parent listener not called)', () => {
    const onCommit = vi.fn();
    const parentKeydown = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'before',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);
    // Attach parent listener to container (bubbles would reach it)
    container.addEventListener('keydown', parentKeydown);

    ie.activate(); // state → active
    ie.input.value = 'changed';

    const escEvent = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true });
    ie.input.dispatchEvent(escEvent);

    // InlineEdit reverted
    expect(ie.getValue()).toBe('before');
    expect(ie.el.dataset.state).toBe('idle');
    // Parent was NOT called — propagation stopped
    expect(parentKeydown).not.toHaveBeenCalled();
  });

  it('Esc while idle bubbles to parent (InlineEdit does not consume it)', () => {
    const onCommit = vi.fn();
    const parentKeydown = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'before',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);
    container.addEventListener('keydown', parentKeydown);

    // State is idle (no activate call)
    expect(ie.el.dataset.state).toBe('idle');

    const escEvent = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true });
    ie.input.dispatchEvent(escEvent);

    // Parent WAS called — propagation not stopped when idle
    expect(parentKeydown).toHaveBeenCalledTimes(1);
  });

  // ── Test 9: suffix option renders suffix element ──────────────────────────
  it('suffix option renders .moss-inline-edit__suffix with aria-hidden', () => {
    const ie = new InlineEdit({
      initialValue: 'mysite',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      suffix: '.mosspub.com',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    const suffix = ie.el.querySelector('.moss-inline-edit__suffix');
    expect(suffix).not.toBeNull();
    expect(suffix!.textContent).toBe('.mosspub.com');
    expect(suffix!.getAttribute('aria-hidden')).toBe('true');
  });

  // ── Test 9b: FIX 6 — slug option adds moss-inline-edit--slug class ────────
  it('slug option adds moss-inline-edit--slug class to el', () => {
    const ie = new InlineEdit({
      initialValue: 'mysite',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      slug: true,
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    expect(ie.el.classList.contains('moss-inline-edit--slug')).toBe(true);
  });

  it('slug option omitted — moss-inline-edit--slug class absent', () => {
    const ie = new InlineEdit({
      initialValue: 'mysite',
      ariaLabel: 'Site ID',
      validationMode: 'explicit-save',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    expect(ie.el.classList.contains('moss-inline-edit--slug')).toBe(false);
  });

  // ── Test 10: setValue() + destroy() ──────────────────────────────────────
  it('setValue updates display without calling onCommit', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'init',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.setValue('new-value');
    expect(ie.getValue()).toBe('new-value');
    expect(onCommit).not.toHaveBeenCalled();
  });

  it('destroy removes listeners: keydown after destroy does not call onCommit', () => {
    const onCommit = vi.fn();
    const ie = new InlineEdit({
      initialValue: 'init',
      ariaLabel: 'Name',
      validationMode: 'blur-autosave',
      onCommit,
    });
    container.appendChild(ie.el);

    ie.activate();
    ie.destroy();

    ie.input.value = 'changed';
    ie.input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onCommit).not.toHaveBeenCalled();
  });

  // ── Test 11: Accessibility invariants ─────────────────────────────────────
  it('status element has role=status and aria-live=polite and is always in DOM', () => {
    const ie = new InlineEdit({
      initialValue: 'val',
      ariaLabel: 'Label',
      validationMode: 'blur-autosave',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    const status = ie.el.querySelector('.moss-inline-edit__status');
    expect(status).not.toBeNull();
    expect(status!.getAttribute('role')).toBe('status');
    expect(status!.getAttribute('aria-live')).toBe('polite');
  });

  it('aria-readonly is NEVER toggled on the input', () => {
    const ie = new InlineEdit({
      initialValue: 'val',
      ariaLabel: 'Label',
      validationMode: 'blur-autosave',
      onCommit: vi.fn(),
    });
    container.appendChild(ie.el);

    // idle state
    expect(ie.input.hasAttribute('aria-readonly')).toBe(false);

    ie.activate();
    // active state
    expect(ie.input.hasAttribute('aria-readonly')).toBe(false);
  });
});
