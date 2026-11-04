// <blueprint-grid>: the grid as one custom element, for a page with no script
// of its own to start it. Attributes and styling: README.md.
import { startBlueprintGrid } from './index.js';

export class BlueprintGridElement extends HTMLElement {
  private stop: (() => void) | null = null;

  connectedCallback(): void {
    this.setAttribute('aria-hidden', 'true');
    const root = this.shadowRoot ?? this.attachShadow({ mode: 'open' });
    root.innerHTML = '<style>:host{display:block;pointer-events:none}canvas{display:block;width:100%;height:100%}</style><canvas></canvas>';
    const maxAlpha = parseFloat(this.getAttribute('max-alpha') ?? '');
    this.stop = startBlueprintGrid(this, {
      canvas: root.querySelector('canvas')!,
      // The window, not the element: a background behind the content never
      // receives the pointer itself.
      pointerTarget: window,
      // Read at every paint, so a page that restyles the element is followed.
      color: () => this.getAttribute('color')
        || getComputedStyle(this).getPropertyValue('--blueprint-grid-color').trim(),
      maxAlpha: Number.isFinite(maxAlpha) ? maxAlpha : undefined,
    });
  }

  disconnectedCallback(): void {
    this.stop?.();
    this.stop = null;
  }
}

if (!customElements.get('blueprint-grid')) customElements.define('blueprint-grid', BlueprintGridElement);
