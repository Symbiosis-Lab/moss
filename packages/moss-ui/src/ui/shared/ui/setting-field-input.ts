/**
 * setting-field-input.ts — one drawn field of the setup contract's vocabulary.
 *
 * The contract's promise is "same Field vocabulary, same renderer": a field
 * declared in `settings[]` and a field inside a `check_setup` blocker's form
 * are the same shape and must look the same. This module is that renderer —
 * the settings page wires save-on-commit around it, the probe modal collects
 * values on submit — so neither surface grows its own drawing code.
 *
 * What it does NOT draw: the `secret` custody UX of the settings page
 * (stored-state placeholder, Remove button, keystore round trips). That is
 * the page's own concern; inside a form a secret is just a password input
 * whose submitted value moss routes to the keystore.
 */

import { createToggle } from "./toggle";
import { createPillSelect, createPillInput } from "./pill-select";

/**
 * Structural on purpose: shared/ imports nothing from the bindings seam, so
 * this module declares the vocabulary it consumes. The generated
 * `bindings.SettingField` is assignable to it, and the compiler checks that
 * at every call site a bindings value flows through — no sync comment needed.
 */
export type SettingValue =
  | null
  | boolean
  | number
  | string
  | SettingValue[]
  | { [key in string]: SettingValue };

export interface SettingFieldOption {
  value: string;
  label: string;
  description?: string | null;
}

export interface SettingField {
  key: string;
  type: "string" | "number" | "boolean" | "secret";
  label?: string;
  description?: string | null;
  /** A heading the form draws above the first field of a run sharing it. */
  group?: string | null;
  default?: SettingValue | null;
  options?: SettingFieldOption[];
  placeholder?: string | null;
  pattern?: string | null;
  pattern_message?: string | null;
  when?: { [key in string]: string };
}

export interface FieldInput {
  /** The `.moss-field` wrapper: label, widget, description, feedback slot. */
  el: HTMLElement;
  key: string;
  /**
   * The current value in the field's wire type — `number` fields answer a
   * number, `boolean` a boolean, everything else a string. `null` is a blank
   * input: nothing to submit, never "clear the stored value".
   */
  value(): SettingValue | null;
  /**
   * The `pattern` verdict, surfaced in the feedback slot as a side effect.
   * A blank value is valid — `pattern` narrows what may be typed, `required`
   * (the publish gate's question) decides whether blank is acceptable.
   */
  valid(): boolean;
  /** Show a message under the field (`field_errors`, save feedback), or clear it. */
  setError(message: string | null): void;
}

export interface FieldInputOptions {
  /** Called when the user commits a value: blur on text, change on a select or toggle. */
  onCommit?: () => void;
}

/** The runtime mirror of `Field::when_matches` — same fail-closed answer. */
export function whenMatches(
  field: SettingField,
  values: Record<string, SettingValue | undefined>
): boolean {
  return Object.entries(field.when ?? {}).every(([key, want]) => {
    const got = values[key];
    if (got === undefined || got === null) return true;
    return typeof got === "string" ? got === want : JSON.stringify(got) === want;
  });
}

export interface FieldRowOptions {
  /** Text for the row's `.moss-label`. */
  label: string;
  /** Adds `moss-field--switch` alongside the base classes (boolean rows). */
  switch?: boolean;
  /** `data-${key}="value"` attributes set on the row, e.g. `{ field: field.key }`. */
  dataset?: Record<string, string>;
}

/**
 * The `.moss-field.moss-field--inline` row shell: a `.moss-label`, then a
 * `.moss-field-body` for the caller to fill with the control, an optional
 * caption, and the feedback slot. One factory because the settings page and
 * the shared field renderer were building this by hand, byte-for-byte
 * identically, in two places.
 */
export function createFieldRow(
  opts: FieldRowOptions
): { row: HTMLDivElement; body: HTMLDivElement; label: HTMLSpanElement } {
  const row = document.createElement("div");
  row.className = opts.switch
    ? "moss-field moss-field--inline moss-field--switch"
    : "moss-field moss-field--inline";
  for (const [name, value] of Object.entries(opts.dataset ?? {})) {
    row.setAttribute(`data-${name}`, value);
  }

  const label = document.createElement("span");
  label.className = "moss-label";
  label.textContent = opts.label;
  row.appendChild(label);

  const body = document.createElement("div");
  body.className = "moss-field-body";
  row.appendChild(body);

  return { row, body, label };
}

export function drawFieldInput(
  field: SettingField,
  initial: SettingValue | undefined,
  options: FieldInputOptions = {}
): FieldInput {
  const { row: el, body } = createFieldRow({
    label: field.label ?? field.key,
    switch: field.type === "boolean",
    dataset: { field: field.key },
  });

  const widget = buildWidget(field, initial, options);
  body.appendChild(widget.el);

  if (field.description) {
    const caption = document.createElement("p");
    caption.className = "moss-field-caption";
    caption.textContent = field.description;
    body.appendChild(caption);
  }

  const feedback = document.createElement("span");
  feedback.className = "moss-input-feedback";
  feedback.setAttribute("data-feedback-for", field.key);
  body.appendChild(feedback);

  const setError = (message: string | null) => {
    feedback.textContent = message ?? "";
    feedback.className = message
      ? "moss-input-feedback moss-input-feedback--error"
      : "moss-input-feedback";
  };

  const valid = () => {
    if (!field.pattern || widget.raw() === "") {
      return true;
    }
    const ok = new RegExp(field.pattern).test(widget.raw());
    setError(ok ? null : field.pattern_message ?? null);
    return ok;
  };

  if (field.pattern) {
    widget.el.addEventListener("input", () => void valid());
  }

  return { el, key: field.key, value: widget.value, valid, setError };
}

interface Widget {
  el: HTMLElement;
  value(): SettingValue | null;
  /** The text as typed, for `pattern` — before any numeric parse. */
  raw(): string;
}

function buildWidget(
  field: SettingField,
  initial: SettingValue | undefined,
  options: FieldInputOptions
): Widget {
  if (field.type === "boolean") {
    let checked = typeof initial === "boolean" ? initial : !!initial;
    // No visible label here: the row's own .moss-label (createFieldRow)
    // carries it. ariaLabel gives the switch itself an accessible name.
    const toggle = createToggle({
      checked,
      ariaLabel: field.label ?? field.key,
      onChange: (next) => {
        checked = next;
        options.onCommit?.();
      },
    });
    return { el: toggle, value: () => checked, raw: () => String(checked) };
  }

  const declaredOptions = field.options ?? [];
  if (field.type === "string" && declaredOptions.length > 0) {
    const current = initial === undefined || initial === null ? "" : String(initial);
    // A stored value the plugin no longer offers is kept as a choice rather
    // than dropped: without it the dropdown would read as the first option
    // while the file still says something else, and the next unrelated save
    // would quietly rewrite the user's setting.
    const known = declaredOptions.some((o) => o.value === current);
    const choices = current && !known
      ? [{ value: current, label: current }, ...declaredOptions]
      : declaredOptions;
    const select = createPillSelect({
      options: choices.map(({ value, label }) => ({ value, label })),
      value: current,
      onChange: () => options.onCommit?.(),
    });
    return { el: select, value: () => select.value, raw: () => select.value };
  }

  const input = createPillInput({
    type: field.type === "secret" ? "password" : field.type === "number" ? "number" : "text",
    value: initial === undefined || initial === null ? "" : String(initial),
    placeholder: field.placeholder ?? undefined,
  });
  input.addEventListener("blur", () => options.onCommit?.());
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      input.blur();
    }
  });
  const value = (): SettingValue | null => {
    if (input.value === "") return null;
    if (field.type === "number") {
      const parsed = Number(input.value);
      return Number.isFinite(parsed) ? parsed : null;
    }
    return input.value;
  };
  return { el: input, value, raw: () => input.value };
}
