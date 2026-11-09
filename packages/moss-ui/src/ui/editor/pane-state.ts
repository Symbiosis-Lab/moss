// packages/moss-ui/src/ui/editor/pane-state.ts
//
// Reusable editor pane-state: heading + description + optional CTA, mounted
// into the CM slot. The one surface for every non-content node-class states.
import { escapeHtml } from './folder-mode/folder-mode';
import { startBlueprintGrid } from '@symbiosis-lab/moss-blueprint-grid';

export interface PaneStateOptions {
  title: string;
  description: string;
  cta?: { label: string; onClick: () => void };
}

const ROOT_ATTR = 'data-pane-state';

// The animated blueprint-grid runs a requestAnimationFrame loop; its stop fn
// must be called on unmount/re-mount or it leaks a RAF + canvas. Only one
// pane-state lives in the single CM slot at a time, so a module-level handle is
// sufficient (mountPaneState always unmounts the prior one first).
let gridCleanup: (() => void) | null = null;

export function mountPaneState(host: HTMLElement, opts: PaneStateOptions): void {
  unmountPaneState(host);
  const root = document.createElement('div');
  root.setAttribute(ROOT_ATTR, '');
  root.className = 'moss-pane-state';
  const ctaHtml = opts.cta
    ? `<button type="button" class="moss-editor-cta moss-pane-state__cta" data-pane-state-cta>${escapeHtml(opts.cta.label)}</button>`
    : '';
  // The blueprint-grid canvas sits BEHIND the content (CSS z-index), carrying
  // the same particle-physics background as the empty-folder onboarding and the
  // launcher — so every "nothing to edit here yet" surface shares one visual.
  root.innerHTML = `
    <canvas id="blueprint-canvas" aria-hidden="true"></canvas>
    <div class="moss-pane-state__inner">
      <h2 class="moss-pane-state__title">${escapeHtml(opts.title)}</h2>
      <p class="moss-pane-state__desc">${escapeHtml(opts.description)}</p>
      ${ctaHtml}
    </div>
  `;
  if (opts.cta) {
    const btn = root.querySelector('[data-pane-state-cta]') as HTMLButtonElement;
    btn.addEventListener('click', () => opts.cta!.onClick());
  }
  host.appendChild(root);
  // Start the animated grid (no-ops under jsdom, where getContext throws).
  gridCleanup = startBlueprintGrid(root);
}

export function unmountPaneState(host: HTMLElement): void {
  if (gridCleanup) {
    gridCleanup();
    gridCleanup = null;
  }
  const existing = host.querySelector(`[${ROOT_ATTR}]`);
  if (existing) existing.remove();
}
