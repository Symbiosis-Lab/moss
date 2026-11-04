import { startBlueprintGrid } from "./index.mjs";

//#region src/element.ts
var BlueprintGridElement = class extends HTMLElement {
	stop = null;
	connectedCallback() {
		this.setAttribute("aria-hidden", "true");
		const root = this.shadowRoot ?? this.attachShadow({ mode: "open" });
		root.innerHTML = "<style>:host{display:block;pointer-events:none}canvas{display:block;width:100%;height:100%}</style><canvas></canvas>";
		const maxAlpha = parseFloat(this.getAttribute("max-alpha") ?? "");
		this.stop = startBlueprintGrid(this, {
			canvas: root.querySelector("canvas"),
			pointerTarget: window,
			color: () => this.getAttribute("color") || getComputedStyle(this).getPropertyValue("--blueprint-grid-color").trim(),
			maxAlpha: Number.isFinite(maxAlpha) ? maxAlpha : void 0
		});
	}
	disconnectedCallback() {
		this.stop?.();
		this.stop = null;
	}
};
if (!customElements.get("blueprint-grid")) customElements.define("blueprint-grid", BlueprintGridElement);

//#endregion
export { BlueprintGridElement };