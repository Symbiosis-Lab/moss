/**
 * InlineEdit — shared inline-edit primitive.
 *
 * Native <input type="text"> always in the DOM; CSS-only idle/active
 * appearance via data-state. Select-all on activation. Enter commits,
 * Esc reverts to the focus-time snapshot. Blur behaviour depends on
 * validationMode: 'blur-autosave' commits immediately; 'explicit-save'
 * runs validateSync → validateAsync, then gates Save.
 */

import { t } from '../i18n';

export interface InlineEditOptions {
  initialValue: string;
  ariaLabel: string;
  validationMode: 'blur-autosave' | 'explicit-save';
  validateSync?: (value: string) => string | null;
  validateAsync?: (value: string) => Promise<'available' | 'taken' | string>;
  onCommit: (value: string) => void;
  suffix?: string;
  placeholder?: string;
  slug?: boolean;
}

type State = 'idle' | 'active' | 'validating' | 'error';

export class InlineEdit {
  readonly el: HTMLElement;
  readonly input: HTMLInputElement;

  private readonly _status: HTMLElement;
  private readonly _saveBtn: HTMLButtonElement | null;
  private readonly _options: InlineEditOptions;

  /** Value captured at focus time — used for Esc revert. */
  private _snapshot: string = '';

  /** Whether listeners have been torn down. */
  private _destroyed = false;

  // Bound handler references kept so we can removeEventListener exactly.
  private _onKeydown: (e: KeyboardEvent) => void;
  private _onBlur: (e: FocusEvent) => void;
  private _onFocus: (e: FocusEvent) => void;

  constructor(options: InlineEditOptions) {
    this._options = options;

    // ── Root container ────────────────────────────────────────────────────
    this.el = document.createElement('div');
    this.el.className = 'moss-inline-edit';
    if (options.slug) {
      this.el.classList.add('moss-inline-edit--slug');
    }
    this._setState('idle');

    // ── Input ─────────────────────────────────────────────────────────────
    this.input = document.createElement('input');
    this.input.type = 'text';
    this.input.className = 'moss-inline-edit__input';
    this.input.setAttribute('aria-label', options.ariaLabel);
    this.input.setAttribute('autocorrect', 'off');
    this.input.setAttribute('spellcheck', 'false');
    this.input.value = options.initialValue;
    if (options.placeholder) {
      this.input.placeholder = options.placeholder;
    }
    this.el.appendChild(this.input);

    // ── Suffix (slug only) ────────────────────────────────────────────────
    if (options.suffix) {
      const suffix = document.createElement('span');
      suffix.className = 'moss-inline-edit__suffix';
      suffix.setAttribute('aria-hidden', 'true');
      suffix.textContent = options.suffix;
      this.el.appendChild(suffix);
    }

    // ── Status (always present) ───────────────────────────────────────────
    // Row = caption (live region) + trailing checking dots, so `validating`
    // can SAY it is checking instead of clearing the slot and going silent.
    // The dots are a SIBLING of the caption, never a child: status writes set
    // `textContent` (which would delete them), and a decorative node changing
    // inside a role=status region re-announces the sentence (the publish-form
    // loader documents the same two rules).
    const statusRow = document.createElement('div');
    statusRow.className = 'moss-inline-edit__status-row';
    // Only a field that can ever validate needs the reserved line height —
    // a blur-autosave field with neither validator (e.g. the email
    // composer's sender-name field) never writes to this row, so reserving
    // space for it left a permanent blank gap under the input.
    if (options.validateSync || options.validateAsync) {
      statusRow.classList.add('moss-inline-edit__status-row--reserved');
    }
    this._status = document.createElement('div');
    this._status.className = 'moss-inline-edit__status';
    this._status.setAttribute('role', 'status');
    this._status.setAttribute('aria-live', 'polite');
    this._status.setAttribute('aria-atomic', 'true');
    statusRow.appendChild(this._status);
    const loader = document.createElement('span');
    loader.className = 'moss-inline-edit__loader moss-checking-dots';
    loader.setAttribute('aria-hidden', 'true');
    loader.append(
      document.createElement('span'),
      document.createElement('span'),
      document.createElement('span'),
    );
    statusRow.appendChild(loader);
    this.el.appendChild(statusRow);

    // ── Save button (explicit-save only) ──────────────────────────────────
    if (options.validationMode === 'explicit-save') {
      this._saveBtn = document.createElement('button');
      this._saveBtn.className = 'moss-btn moss-btn--small moss-inline-edit__save';
      this._saveBtn.disabled = true;
      this._saveBtn.hidden = true;
      this._saveBtn.textContent = t('inline.save');
      this._saveBtn.addEventListener('click', () => this._commit());
      this.el.appendChild(this._saveBtn);
    } else {
      this._saveBtn = null;
    }

    // ── Event handlers ────────────────────────────────────────────────────
    this._onFocus = () => this._handleFocus();
    this._onKeydown = (e) => this._handleKeydown(e);
    this._onBlur = () => this._handleBlur();

    this.input.addEventListener('focus', this._onFocus);
    this.input.addEventListener('keydown', this._onKeydown);
    this.input.addEventListener('blur', this._onBlur);
  }

  // ── Public API ────────────────────────────────────────────────────────────

  getValue(): string {
    return this.input.value;
  }

  setValue(value: string): void {
    this.input.value = value;
    // Does NOT call onCommit — caller owns persistence.
  }

  activate(): void {
    this._snapshot = this.input.value;
    this._setState('active');
    this.input.focus();
    this.input.select();
  }

  destroy(): void {
    this._destroyed = true;
    this.input.removeEventListener('focus', this._onFocus);
    this.input.removeEventListener('keydown', this._onKeydown);
    this.input.removeEventListener('blur', this._onBlur);
  }

  // ── Private helpers ───────────────────────────────────────────────────────

  private _setState(state: State): void {
    this.el.dataset.state = state;
  }

  private _getState(): State {
    return (this.el.dataset.state as State) ?? 'idle';
  }

  private _clearStatus(): void {
    this._status.textContent = '';
    this._status.className = 'moss-inline-edit__status';
  }

  private _setStatus(text: string, modifier?: 'error' | 'success'): void {
    this._status.textContent = text;
    this._status.className = 'moss-inline-edit__status';
    if (modifier) {
      this._status.classList.add(`--${modifier}`);
    }
  }

  private _enableSave(enabled: boolean): void {
    if (!this._saveBtn) return;
    this._saveBtn.disabled = !enabled;
    this._saveBtn.hidden = !enabled;
  }

  private _commit(): void {
    if (this._destroyed) return;
    const value = this.input.value;
    this._snapshot = value;
    this._setState('idle');
    this._clearStatus();
    this._enableSave(false);
    this._options.onCommit(value);
  }

  private _revert(): void {
    this.input.value = this._snapshot;
    this._setState('idle');
    this._clearStatus();
    this._enableSave(false);
  }

  private _handleFocus(): void {
    if (this._destroyed) return;
    this._snapshot = this.input.value;
    this._setState('active');
    // select-all on focus
    this.input.select();
  }

  private _handleKeydown(e: KeyboardEvent): void {
    if (this._destroyed) return;
    if (e.key === 'Enter') {
      // IME guard: an Enter that confirms a CJK composition (e.g. choosing a
      // candidate in the email-composer sender-name field) must not commit.
      if (e.isComposing) return;
      e.preventDefault();
      if (this._options.validationMode === 'blur-autosave') {
        this._commit();
      } else {
        // explicit-save: Enter commits only if save is enabled
        if (this._saveBtn && !this._saveBtn.disabled) {
          this._commit();
        } else if (this._getState() !== 'validating') {
          // Treat like a blur to trigger validation
          this._runValidation();
        }
      }
    } else if (e.key === 'Escape') {
      if (this._getState() === 'active' || this._getState() === 'validating' || this._getState() === 'error') {
        // InlineEdit consumes this Escape — stop propagation so parent
        // keydown handlers (e.g. escHierarchy closing the composer) do not fire.
        e.preventDefault();
        e.stopPropagation();
        this._revert();
      }
      // When idle, let Esc bubble normally.
    }
  }

  private _handleBlur(): void {
    if (this._destroyed) return;
    if (this._options.validationMode === 'blur-autosave') {
      // Guard: if already committed via Enter (state is idle), skip the blur commit.
      if (this._getState() !== 'active') return;
      this._commit();
    } else {
      // explicit-save: run validation on blur
      this._runValidation();
    }
  }

  private _runValidation(): void {
    const value = this.input.value;
    this._clearStatus();
    this._enableSave(false);

    // 1. Sync validation
    if (this._options.validateSync) {
      const syncError = this._options.validateSync(value);
      if (syncError !== null) {
        this._setState('error');
        this._setStatus(syncError, 'error');
        // validateAsync NOT called on sync error
        return;
      }
    }

    // 2. Async validation
    if (this._options.validateAsync) {
      this._setState('validating');
      // Say so — a cleared slot here left the wait visually silent (the
      // subdomain claim's dead air). The dots trail this sentence via the
      // `[data-state="validating"]` reveal; every exit path below writes or
      // clears the status.
      this._setStatus(t('inline.checking'));
      this._options.validateAsync(value).then((result) => {
        // FIX 1: Ignore stale results — destroyed or user typed a new value.
        if (this._destroyed) return;
        if (this.input.value !== value) return;
        if (result === 'available') {
          this._setState('active');
          this._clearStatus();
          this._enableSave(true);
        } else if (result === 'taken') {
          this._setState('error');
          this._setStatus(t('inline.id_taken'), 'error');
          this._enableSave(false);
        } else {
          // Other error string
          this._setState('error');
          this._setStatus(result, 'error');
          this._enableSave(false);
        }
      }).catch((err) => {
        // FIX 2: Handle validateAsync rejection — leave user in recoverable error state.
        if (this._destroyed) return;
        if (this.input.value !== value) return;
        this._setState('error');
        this._setStatus(err instanceof Error ? err.message : t('inline.validation_failed'), 'error');
        this._enableSave(false);
      });
    } else {
      // No async validator — value is valid; show Save
      this._setState('active');
      this._enableSave(true);
    }
  }
}
