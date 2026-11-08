/**
 * outcome-modal.ts — a modal whose caller is waiting for an answer.
 *
 * Most modals end when they close. These two — the credential prompt and the
 * setup probe — suspend a publish and hand it back, so closing is not the
 * event; answering is. Both keep Cancel live across an await (a store write, a
 * plugin re-probe), which is exactly the race a plain callback loses: the user
 * cancels, the await then succeeds, and the publish the user dropped restarts.
 *
 * One exit, taken once. Kept apart from `BaseModal` deliberately: the seven
 * other modals have no outcome to report, and a callback on the base class
 * would hand them a concept they do not have.
 */

import { BaseModal } from "./base-modal";

export abstract class OutcomeModal extends BaseModal {
  private settled = false;

  constructor(private readonly onDone: (result: boolean) => void) {
    super();
  }

  /**
   * Answer, once. Unmounts BEFORE telling the caller: `onDone` re-enters the
   * publish handler synchronously, and a still-mounted modal would still hold
   * `BaseModal`'s per-class slot when the next one tries to open.
   */
  protected finish(result: boolean): void {
    if (this.settled) return;
    this.settled = true;
    super.close();
    this.onDone(result);
  }

  /** Any other close — Cancel, Escape, the backdrop — is a no. */
  close(): void {
    if (this._isOpen) {
      this.finish(false);
      return;
    }
    super.close();
  }
}
