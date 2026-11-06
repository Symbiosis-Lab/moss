/**
 * The cleanups a script's wiring has accumulated, run together and in the
 * order they were recorded. `release()` empties the stack, so releasing twice
 * is harmless and the stack can be reused for the next wiring.
 */
export function teardownStack() {
  let cleanups: Array<() => void> = [];
  return {
    on<K extends string>(
      target: Window | Document | Element,
      type: K,
      handler: (event: K extends keyof HTMLElementEventMap ? HTMLElementEventMap[K] : Event) => void,
      options?: AddEventListenerOptions,
    ): void {
      const listener = handler as EventListener;
      target.addEventListener(type, listener, options);
      cleanups.push(() => target.removeEventListener(type, listener, options));
    },
    add(cleanup: () => void): void {
      cleanups.push(cleanup);
    },
    release(): void {
      const run = cleanups;
      cleanups = [];
      run.forEach((cleanup) => cleanup());
    },
  };
}
