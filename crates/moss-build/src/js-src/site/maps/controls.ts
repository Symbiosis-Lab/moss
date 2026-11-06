/**
 * The zoom controls both maps share: a glass capsule (zoom in over zoom out)
 * above a reset circle, in one labelled group. The look lives in
 * `map-controls.css`; each map positions the group and wires the buttons.
 */

export interface ControlLabels {
  /** The group's accessible name. */
  group: string;
  zoomIn: string;
  zoomOut: string;
  reset: string;
}

export interface Controls {
  element: HTMLElement;
  zoomIn: HTMLButtonElement;
  zoomOut: HTMLButtonElement;
  reset: HTMLButtonElement;
}

function button(control: "zoom-in" | "zoom-out" | "reset", label: string): HTMLButtonElement {
  const el = document.createElement("button");
  el.type = "button";
  el.className = "moss-places-control";
  el.dataset.control = control;
  el.setAttribute("aria-label", label);
  return el;
}

export function buildControls(labels: ControlLabels): Controls {
  const element = document.createElement("div");
  element.className = "moss-places-controls";
  element.setAttribute("role", "group");
  element.setAttribute("aria-label", labels.group);
  const capsule = document.createElement("div");
  capsule.className = "moss-places-capsule";
  capsule.setAttribute("role", "group");
  const zoomIn = button("zoom-in", labels.zoomIn);
  const zoomOut = button("zoom-out", labels.zoomOut);
  capsule.append(zoomIn, zoomOut);
  const reset = button("reset", labels.reset);
  // Both maps open on the whole view, where there is nothing to reset to.
  reset.hidden = true;
  element.append(capsule, reset);
  return { element, zoomIn, zoomOut, reset };
}
