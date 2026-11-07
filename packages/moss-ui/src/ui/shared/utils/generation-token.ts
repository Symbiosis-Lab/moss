/**
 * Generation token — the primitive behind every "stale completion" guard.
 *
 * The bug class this exists to kill: an async op (open a
 * document, resolve a URL, compute a heading) starts, something newer
 * supersedes it before the op finishes, and the late completion overwrites
 * the newer state. This one counter has been hand-rolled five separate
 * times under five different names; this is the shared shape. Read
 * current() before starting the async work, isStale(read) after any await
 * to decide whether to apply the result, bump() to mark "something newer
 * started now".
 *
 * One instance per independently-superseded concern — two owners that bump
 * on different events (e.g. "a load attempt began" vs. "the document
 * actually changed") need two tokens, even if a second consumer reads the
 * first owner's token because it needs that same notion of current.
 */
export function createGenerationToken() {
  let generation = 0;
  return {
    /** Start a new generation; every value read before this is now stale. Returns the new value. */
    bump: (): number => ++generation,
    /** The live generation — read it up front to snapshot before an async op, or later to stamp data isStale() will check. */
    current: (): number => generation,
    /** True once `read` no longer matches the live generation. */
    isStale: (read: number): boolean => read !== generation,
  };
}

export type GenerationToken = ReturnType<typeof createGenerationToken>;
