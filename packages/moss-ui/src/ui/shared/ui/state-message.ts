/**
 * StateMessage — shared empty / loading / error presentation primitive.
 *
 * Creates a `.moss-state` element with optional icon, title, description,
 * and action button. Variant modifiers colour the icon for semantic meaning.
 *
 * CSS lives in styles/shared/primitives.css.
 */

export type StateVariant = "empty" | "loading" | "error";

export interface StateMessageOptions {
  /** Semantic variant — controls icon colour. Defaults to "empty". */
  variant?: StateVariant;
  /** Optional icon text or emoji rendered above the title. */
  icon?: string;
  /** Required title text. */
  title: string;
  /** Optional prose description below the title. */
  description?: string;
  /** Optional action button. */
  action?: {
    label: string;
    onClick: () => void;
  };
}

/**
 * Factory that returns a `.moss-state` element.
 *
 * The element is purely presentational — no internal state, no subscriptions.
 * Append it wherever needed; the caller owns its lifecycle.
 */
export function createStateMessage(options: StateMessageOptions): HTMLElement {
  const { variant = "empty", icon, title, description, action } = options;

  const root = document.createElement("div");
  root.className = `moss-state moss-state--${variant}`;

  if (icon) {
    const iconEl = document.createElement("div");
    iconEl.className = "moss-state__icon";
    iconEl.textContent = icon;
    iconEl.setAttribute("aria-hidden", "true");
    root.appendChild(iconEl);
  }

  const titleEl = document.createElement("p");
  titleEl.className = "moss-state__title";
  titleEl.textContent = title;
  root.appendChild(titleEl);

  if (description) {
    const descEl = document.createElement("p");
    descEl.className = "moss-state__description";
    descEl.textContent = description;
    root.appendChild(descEl);
  }

  if (action) {
    const actionWrapper = document.createElement("div");
    actionWrapper.className = "moss-state__action";

    const btn = document.createElement("button");
    btn.className = "moss-btn";
    btn.textContent = action.label;
    btn.addEventListener("click", action.onClick);
    actionWrapper.appendChild(btn);

    root.appendChild(actionWrapper);
  }

  return root;
}
