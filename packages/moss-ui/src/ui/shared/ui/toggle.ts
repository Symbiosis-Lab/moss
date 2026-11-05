/**
 * Toggle — general-purpose boolean switch primitive.
 *
 * Creates a `.moss-toggle` row with label, optional description, and a
 * `.moss-toggle__switch` that is keyboard-accessible (role="switch").
 *
 * CSS lives in styles/shared/primitives.css.
 */

export interface ToggleOptions {
  /**
   * Primary label text. Optional: when neither `label` nor `description` is
   * given, `createToggle` renders no `.moss-toggle__body` at all — the switch
   * is the toggle's only child. A row grammar that draws the label in its own
   * grid column has
   * nothing for the body to hold.
   */
  label?: string;
  /** Optional secondary help text displayed below the label. */
  description?: string;
  /** Initial checked state. */
  checked: boolean;
  /** Called with the new boolean value after each toggle. */
  onChange: (checked: boolean) => void;
  /** When true the toggle is non-interactive (opacity + pointer-events). */
  disabled?: boolean;
  /**
   * Accessible name for the switch element, for the body-less case where no
   * `label`/`description` is given here — a sibling in the caller's own
   * markup (e.g. a row's `.moss-label`) carries the visible text instead.
   * Leaves the labelled path (`label`/`description` given) untouched.
   */
  ariaLabel?: string;
}

/**
 * Programmatic setters keyed by toggle root. A WeakMap (not a returned handle)
 * keeps `createToggle`'s public shape — callers hold a plain element — while
 * giving revert paths ONE place that updates the internal `isChecked` and the
 * DOM together. Repainting the DOM from outside desyncs the two: the next
 * click then computes its value from stale state and is a visual no-op.
 */
const setters = new WeakMap<HTMLElement, (checked: boolean) => void>();

/**
 * Set a toggle's checked state programmatically (e.g. reverting a failed
 * save). Updates internal state + DOM; does NOT fire `onChange` — a revert
 * must not look like a user action.
 */
export function setToggleChecked(root: HTMLElement, checked: boolean): void {
  setters.get(root)?.(checked);
}

/**
 * Factory that returns a fully-composed `.moss-toggle` element.
 *
 * The returned element is self-contained: clicking or pressing Space/Enter on
 * the switch element updates its visual state and fires `onChange`.
 * The caller owns the element's lifecycle — append it, remove it, done.
 */
export function createToggle(options: ToggleOptions): HTMLElement {
  const { label, description, checked, onChange, disabled = false, ariaLabel } = options;

  // --- Root row ---
  const root = document.createElement("div");
  root.className = "moss-toggle" + (disabled ? " moss-toggle--disabled" : "");

  // --- Left body (label + description) ---
  // Omitted entirely when the caller supplies neither: a row-grammar switch
  // draws its own label
  // in the grid's label column, and an empty `.moss-toggle__body` would still
  // take up flex space next to the switch.
  const body = label || description ? document.createElement("div") : null;
  if (body) {
    body.className = "moss-toggle__body";

    if (label) {
      const labelEl = document.createElement("span");
      labelEl.className = "moss-toggle__label";
      labelEl.textContent = label;
      body.appendChild(labelEl);
    }

    if (description) {
      const descEl = document.createElement("span");
      descEl.className = "moss-toggle__description";
      descEl.textContent = description;
      body.appendChild(descEl);
    }
  }

  // --- Switch track ---
  const switchEl = document.createElement("div");
  switchEl.className = "moss-toggle__switch" + (checked ? " active" : "");
  switchEl.setAttribute("role", "switch");
  switchEl.setAttribute("aria-checked", String(checked));
  switchEl.setAttribute("tabindex", disabled ? "-1" : "0");
  if (disabled) {
    switchEl.setAttribute("aria-disabled", "true");
  }
  if (ariaLabel) {
    switchEl.setAttribute("aria-label", ariaLabel);
  }

  // Internal mutable state
  let isChecked = checked;

  function setChecked(next: boolean) {
    isChecked = next;
    switchEl.classList.toggle("active", isChecked);
    switchEl.setAttribute("aria-checked", String(isChecked));
  }
  setters.set(root, setChecked);

  function toggle() {
    if (disabled) return;
    setChecked(!isChecked);
    onChange(isChecked);
  }

  // Click handler
  switchEl.addEventListener("click", toggle);

  // Keyboard: Space and Enter toggle
  switchEl.addEventListener("keydown", (e: KeyboardEvent) => {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      toggle();
    }
  });

  if (body) root.appendChild(body);
  root.appendChild(switchEl);

  return root;
}

/**
 * A labelled checkbox row for dialogs ("Don't show this again"); the whole label is
 * the hit area. CSS: `.moss-modal-check` in styles/shared/modal.css.
 */
export function checkboxRow(label: string): { row: HTMLLabelElement; input: HTMLInputElement } {
  const row = document.createElement('label');
  row.className = 'moss-modal-check';
  const input = document.createElement('input');
  input.type = 'checkbox';
  const text = document.createElement('span');
  text.textContent = label;
  row.append(input, text);
  return { row, input };
}
