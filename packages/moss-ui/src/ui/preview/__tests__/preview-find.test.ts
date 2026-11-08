import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { PreviewFindController } from '../preview-find';

function makeFakeRpc() {
  return { call: vi.fn().mockResolvedValue(true), destroy: vi.fn() };
}

describe('PreviewFindController', () => {
  let mountPoint: HTMLElement;
  let ctrl: PreviewFindController;
  let rpc: ReturnType<typeof makeFakeRpc>;

  beforeEach(() => {
    mountPoint = document.createElement('div');
    mountPoint.id = 'moss-preview-find';
    document.body.appendChild(mountPoint);
    rpc = makeFakeRpc();
    ctrl = new PreviewFindController();
  });

  afterEach(() => {
    ctrl.close();
    document.body.innerHTML = '';
  });

  it('open() appends pill to the mount point', () => {
    ctrl.open(rpc as any, mountPoint);
    expect(mountPoint.querySelector('.find-pill')).not.toBeNull();
  });

  it('open() is idempotent', () => {
    ctrl.open(rpc as any, mountPoint);
    ctrl.open(rpc as any, mountPoint);
    expect(mountPoint.querySelectorAll('.find-pill').length).toBe(1);
  });

  it('close() removes pill from DOM', () => {
    ctrl.open(rpc as any, mountPoint);
    ctrl.close();
    expect(mountPoint.querySelector('.find-pill')).toBeNull();
  });

  it('close() calls rpc.call("find") with empty string to clear highlight', () => {
    ctrl.open(rpc as any, mountPoint);
    ctrl.close();
    expect(rpc.call).toHaveBeenCalledWith('find', '', false, false, false);
  });

  it('close() calls rpc.destroy() to remove the message listener', () => {
    ctrl.open(rpc as any, mountPoint);
    ctrl.close();
    expect(rpc.destroy).toHaveBeenCalled();
  });

  it('isOpen() reflects state', () => {
    expect(ctrl.isOpen()).toBe(false);
    ctrl.open(rpc as any, mountPoint);
    expect(ctrl.isOpen()).toBe(true);
    ctrl.close();
    expect(ctrl.isOpen()).toBe(false);
  });

  it('findNext calls rpc.call with query forwards', () => {
    ctrl.open(rpc as any, mountPoint);
    const input = mountPoint.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.value = 'hello';
    input.dispatchEvent(new Event('input'));
    const nextBtn = mountPoint.querySelector('.find-pill__btn--next') as HTMLButtonElement;
    nextBtn.click();
    expect(rpc.call).toHaveBeenCalledWith('find', 'hello', false, false, true);
  });

  it('findPrev calls rpc.call with query backwards', () => {
    ctrl.open(rpc as any, mountPoint);
    const input = mountPoint.querySelector('.find-pill__input--find') as HTMLInputElement;
    input.value = 'hello';
    input.dispatchEvent(new Event('input'));
    const prevBtn = mountPoint.querySelector('.find-pill__btn--prev') as HTMLButtonElement;
    prevBtn.click();
    expect(rpc.call).toHaveBeenCalledWith('find', 'hello', false, true, true);
  });

  it('focusInput() focuses the find input when open', () => {
    ctrl.open(rpc as any, mountPoint);
    const input = mountPoint.querySelector('.find-pill__input--find') as HTMLInputElement;
    const focusSpy = vi.spyOn(input, 'focus');
    ctrl.focusInput();
    expect(focusSpy).toHaveBeenCalled();
  });
});
