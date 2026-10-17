/**
 * A mounted map arms debounced world rebakes and has no teardown of its
 * own, so a rebake can fire after a test file ends, against an environment
 * already torn down. Importing this module into a test file cancels every
 * timer still armed when the file finishes.
 */
import { afterAll } from "vitest";

const armed = new Set<ReturnType<typeof setTimeout>>();
const realSetTimeout = globalThis.setTimeout;

globalThis.setTimeout = ((handler: (...args: unknown[]) => void, ms?: number, ...args: unknown[]) => {
  const timer = realSetTimeout(() => {
    armed.delete(timer);
    handler(...args);
  }, ms);
  armed.add(timer);
  return timer;
}) as typeof setTimeout;

afterAll(() => {
  for (const timer of armed) clearTimeout(timer);
  globalThis.setTimeout = realSetTimeout;
});
