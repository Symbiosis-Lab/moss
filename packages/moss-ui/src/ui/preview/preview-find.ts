import { createFindPill, type FindPill } from "../components/find-pill";
import type { IframeRPC } from './iframe-rpc';

export class PreviewFindController {
  private pill: FindPill | null = null;
  private currentQuery = '';
  private rpc: IframeRPC | null = null;

  open(rpc: IframeRPC, mountPoint: HTMLElement): void {
    if (this.pill) {
      this.pill.focusInput();
      return;
    }

    this.rpc = rpc;

    this.pill = createFindPill({
      mode: 'find-only',
      onQueryChange: (q) => {
        this.currentQuery = q;
      },
      onFindNext: () => this._find(false),
      onFindPrev: () => this._find(true),
      onClose: () => this.close(),
    });

    mountPoint.appendChild(this.pill.el);
    this.pill.focusInput();
  }

  close(): void {
    if (!this.pill) return;
    this.pill.destroy();
    this.pill = null;
    this.rpc?.call('find', '', false, false, false);
    this.rpc?.destroy();
    this.rpc = null;
    this.currentQuery = '';
  }

  isOpen(): boolean {
    return this.pill !== null;
  }

  focusInput(): void {
    this.pill?.focusInput();
  }

  private _find(backwards: boolean): void {
    if (!this.rpc || !this.currentQuery) return;
    // window.find(aString, caseSensitive, backwards, wrapAround)
    this.rpc.call('find', this.currentQuery, false, backwards, true);
  }
}
