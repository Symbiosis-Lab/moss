//#region src/element.d.ts
declare class BlueprintGridElement extends HTMLElement {
  private stop;
  connectedCallback(): void;
  disconnectedCallback(): void;
}
//#endregion
export { BlueprintGridElement };