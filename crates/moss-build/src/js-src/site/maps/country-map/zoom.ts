/** Zoom for the country map: the shared controls and gestures, driving the viewBox camera. */
import { buildControls, type ControlLabels } from "../controls";
import { attachGestures } from "../gestures";
import { reducedMotion } from "../media";
import { clampView, MAX_ZOOM, pannedBy, sameView, zoomedAt, type Camera, type ViewBox } from "./frame";

/** One press of + or −, and one key press. */
const STEP = 1.5;

interface ZoomParts {
  stage: HTMLElement;
  svg: SVGSVGElement;
  camera: Camera;
  labels: ControlLabels & { hint: string };
  /** The reader moved or zoomed the map themselves. */
  onUserMove(): void;
}

/** Mounts the controls, the gestures and the keyboard path; returns the function that brings the buttons in step with the camera. */
export function mountZoom({ stage, svg, camera, labels, onUserMove }: ZoomParts): () => void {
  const controls = buildControls(labels);
  stage.append(controls.element);

  const hint = document.createElement("p");
  hint.id = `moss-country-hint-${stage.closest<HTMLElement>("[data-moss-country-map]")?.dataset.mossCountryMap || "map"}`;
  hint.className = "moss-country-key-hint";
  hint.textContent = labels.hint;
  stage.append(hint);
  stage.tabIndex = 0;
  stage.setAttribute("role", "region");
  stage.setAttribute("aria-label", svg.getAttribute("aria-label") || "");
  stage.setAttribute("aria-describedby", hint.id);

  const sync = (): void => {
    const view = camera.goal();
    const atWhole = view[2] >= camera.whole[2] - 0.01;
    controls.zoomOut.disabled = atWhole;
    controls.zoomIn.disabled = view[2] <= camera.whole[2] / MAX_ZOOM + 0.01;
    controls.reset.hidden = sameView(view, camera.whole);
    stage.toggleAttribute("data-zoomed", !atWhole);
  };

  const go = (view: ViewBox, animate: boolean): void => {
    onUserMove();
    camera.to(clampView(view, camera.world, camera.whole), animate && !reducedMotion());
  };
  const stepBy = (factor: number): void => go(zoomedAt(camera.goal(), factor, 0.5, 0.5, camera.world, camera.whole), true);
  controls.zoomIn.addEventListener("click", () => stepBy(STEP));
  controls.zoomOut.addEventListener("click", () => stepBy(1 / STEP));
  controls.reset.addEventListener("click", () => go(camera.whole, true));

  let from = camera.goal();
  attachGestures(
    stage,
    {
      panStart: () => {
        from = camera.goal();
      },
      panTo(dx, dy) {
        const size = svg.getBoundingClientRect();
        if (size.width && size.height) go(pannedBy(from, dx, dy, size, camera.world, camera.whole), false);
      },
      zoomBy(factor, at) {
        const rect = svg.getBoundingClientRect();
        // A box with no size yet zooms about its middle.
        go(zoomedAt(camera.goal(), factor, rect.width ? at.x / rect.width : 0.5, rect.height ? at.y / rect.height : 0.5, camera.world, camera.whole), false);
      },
      reset: () => go(camera.whole, true),
      onGestureStart() {
        camera.stop();
        stage.querySelector<HTMLElement>(".moss-country-tip")?.setAttribute("hidden", "");
      },
    },
    { ownJob: "button, a, .moss-country-panel, .moss-places-controls", keyZoomStep: STEP, wheelNeedsModifier: () => true },
  );
  return sync;
}
