/**
 * Cold-start skeleton dismissers.
 *
 * editor.html ships STATIC skeleton markup inside #nav-grid and
 * #moss-editor-cm so the editor webview paints a shimmer before any JS
 * executes (the module script hasn't run yet). Removal is layered:
 *
 * - Content skeleton (#moss-editor-skeleton): removed STRUCTURALLY — every
 *   content mount path clears #moss-editor-cm's innerHTML (CM6 mount,
 *   non-text viewer, applyEditorSurface, email composer's renderComposer).
 *   No explicit call sites to rot as new surfaces are added.
 * - Nav skeleton (#moss-nav-skeleton): FileTree.mount APPENDS without
 *   clearing, so editor-main calls dismissNavSkeleton() immediately before
 *   the tree mount — mount appends the real (identically-sized, hairlined)
 *   breadcrumb strip synchronously in the same task, so the swap is seamless.
 * - Failsafe: init().finally(dismissEditorSkeleton) removes both on every
 *   terminal path that mounts nothing (no project + no source; bootstrap
 *   error; initial parse failure) — an eternal shimmer would be dishonest.
 *   On the no-project path this failsafe is the SOLE nav-skeleton remover.
 */

export function dismissNavSkeleton(): void {
  document.getElementById('moss-nav-skeleton')?.remove();
}

export function dismissContentSkeleton(): void {
  document.getElementById('moss-editor-skeleton')?.remove();
}

export function dismissEditorSkeleton(): void {
  dismissNavSkeleton();
  dismissContentSkeleton();
}
