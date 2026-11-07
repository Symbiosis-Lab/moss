/**
 * Pill select — the ONE place that builds a `<select class="moss-select
 * moss-select--pill">`, moss's glass-pill choice control for a setting row
 * that sits beside its label.
 *
 * Zero-import leaf by design: shared/ui primitives take nothing from the app
 * graph, so callers in lean surfaces (the launcher) can use this without
 * pulling anything in behind it.
 *
 * Also home to `createPillInput`, the ONE place that builds an
 * `<input class="moss-pill-input">` — the same glass-pill look for a text
 * control. autocomplete/autocorrect/autocapitalize/spellcheck are always off: a
 * plugin key, a hostname or a pin name is an identifier, not prose, and platform
 * autocorrect silently mangling it is the failure this avoids.
 *
 * CSS lives in styles/shared/primitives.css (.moss-select,
 * .moss-select--pill, .moss-pill-input).
 */

export interface PillSelectOption {
  value: string;
  label: string;
}

export interface PillSelectOptions {
  options: PillSelectOption[];
  /** Initial selected value. Left to the browser default (first option) when omitted. */
  value?: string;
  onChange?: (value: string) => void;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  /** Extra class(es) appended after the two base pill classes. */
  extraClass?: string;
}

export function createPillSelect(opts: PillSelectOptions): HTMLSelectElement {
  const select = document.createElement("select");
  select.className = opts.extraClass
    ? `moss-select moss-select--pill ${opts.extraClass}`
    : "moss-select moss-select--pill";

  for (const option of opts.options) {
    const el = document.createElement("option");
    el.value = option.value;
    el.textContent = option.label;
    select.appendChild(el);
  }
  if (opts.value !== undefined) select.value = opts.value;

  if (opts.ariaLabel) select.setAttribute("aria-label", opts.ariaLabel);
  if (opts.ariaLabelledBy) select.setAttribute("aria-labelledby", opts.ariaLabelledBy);

  if (opts.onChange) {
    const onChange = opts.onChange;
    select.addEventListener("change", () => onChange(select.value));
  }

  return select;
}

export interface PillInputOptions {
  type?: "text" | "password" | "url" | "number";
  value?: string;
  placeholder?: string;
  ariaLabel?: string;
  /** Extra class(es) appended after the base pill class. */
  extraClass?: string;
}

export function createPillInput(opts: PillInputOptions): HTMLInputElement {
  const input = document.createElement("input");
  input.type = opts.type ?? "text";
  input.className = opts.extraClass ? `moss-pill-input ${opts.extraClass}` : "moss-pill-input";

  if (opts.value !== undefined) input.value = opts.value;
  if (opts.placeholder !== undefined) input.placeholder = opts.placeholder;
  if (opts.ariaLabel) input.setAttribute("aria-label", opts.ariaLabel);

  input.autocomplete = "off";
  input.autocapitalize = "off";
  input.spellcheck = false;
  input.setAttribute("autocorrect", "off");

  return input;
}
