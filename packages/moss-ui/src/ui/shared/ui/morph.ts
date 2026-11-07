/**
 * morph — height-morph + reveal/collapse helpers for surfaces that GROW
 * instead of swapping.
 *
 * The first-publish modal is one form that changes as little as possible
 * between phases: shared elements keep their DOM identity and their screen
 * position, and only the space below the growth point moves. That needs a
 * FLIP-style height transition around a synchronous DOM mutation, which is
 * what `morphHeight` is.
 *
 * Zero imports on purpose — this is a leaf primitive.
 */

/** Safety net if `transitionend` never fires (display:none, detached, …). */
const MORPH_FALLBACK_MS = 400;

/**
 * The most recent morph token per element. A second `morphHeight` on the same
 * element retargets (reading the CURRENT animated height as `from`, which is
 * correct), but the OUTGOING morph's `transitionend` must not then clear the
 * incoming one's inline styles — only the latest token may restore.
 */
const latestMorph = new WeakMap<HTMLElement, object>();

/**
 * Read the reduced-motion preference at CALL time, never cached — a
 * mid-session OS toggle must be respected.
 */
export function prefersReducedMotion(): boolean {
  return (
    typeof matchMedia === 'function' &&
    matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

/**
 * Animate `el`'s height across the DOM changes made by `mutate()`.
 *
 * `mutate()` runs exactly once, synchronously, between the before/after
 * measurements. The returned promise resolves once the transition settles (or
 * immediately when there is nothing to animate), so callers can sequence
 * focus moves after the box has stopped moving.
 *
 * The `transition: height …` rule must exist in CSS — this function writes
 * `height` and nothing else. Without the rule the height snaps and we sit in
 * `overflow: hidden` until the fallback timer.
 */
export async function morphHeight(
  el: HTMLElement,
  mutate: () => void,
): Promise<void> {
  const from = el.getBoundingClientRect().height;
  mutate();
  const to = el.getBoundingClientRect().height;

  if (from === to || prefersReducedMotion()) {
    // Nothing to animate — and, importantly, no style writes at all, so a
    // reduced-motion session never gets a stray inline height to leak.
    return;
  }

  // A clamped box cannot be measured: with `max-height` + `overflow-y: auto`
  // the rect returns the clamp, not the content height, so animating it would
  // be a lie (and would fight the scrollbar). Skip the animation; the mutation
  // has already been applied.
  const clamp = parseFloat(getComputedStyle(el).maxHeight);
  if (Number.isFinite(clamp) && (Math.abs(from - clamp) < 1 || Math.abs(to - clamp) < 1)) {
    return;
  }

  const token = {};
  latestMorph.set(el, token);
  el.style.overflow = 'hidden';
  el.style.height = `${from}px`;
  void el.offsetHeight; // force reflow so the next write is a transition start
  el.style.height = `${to}px`;

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      el.removeEventListener('transitionend', onEnd);
      clearTimeout(timer);
      // Only the LAST morph in a re-entrant chain restores the element: an
      // outgoing transitionend must not strip the incoming morph's styles.
      if (latestMorph.get(el) === token) {
        latestMorph.delete(el);
        el.style.height = '';
        el.style.overflow = '';
      }
      resolve();
    };
    const onEnd = (e: TransitionEvent) => {
      // `transitionend` bubbles: the modal transitions opacity/transform and
      // every revealed row transitions opacity. Any of those would otherwise
      // clear an in-flight height.
      if (e.propertyName !== 'height' || e.target !== el) return;
      finish();
    };
    el.addEventListener('transitionend', onEnd);
    const timer = setTimeout(finish, MORPH_FALLBACK_MS);
  });
}

/**
 * The row morph. Same duration and curve as the height transition in CSS
 * (`--fp-morph-dur`, `--fp-ease`) so a box that grows while its rows re-align
 * reads as ONE movement.
 */
const ROW_MORPH_MS = 240;
const ROW_MORPH_EASE = 'cubic-bezier(0.2, 0, 0, 1)';

/**
 * FLIP the given rows across the DOM changes made by `mutate()`.
 *
 * `morphHeight` animates the BOX; this animates what is inside it. A phase that
 * drops a row or re-aligns the surface moves everything below or beside it, and
 * without this those rows are simply somewhere else on the next frame — the
 * jump the whole morph exists to avoid.
 *
 * Rows that were not rendered before the mutation are left alone: that is an
 * entrance, and `revealRow` owns entrances. Web Animations rather than a CSS
 * transition, so nothing has to be written to (and cleaned out of) `style`.
 */
export function flipRows(rows: HTMLElement[], mutate: () => void): void {
  // No Web Animations API (jsdom under the unit-test suite; a hypothetically
  // very old WebKit): behave like reduced motion rather than throw.
  if (prefersReducedMotion() || typeof rows[0]?.animate !== 'function') {
    mutate();
    return;
  }
  const before = rows.map((el) => ({ el, rect: el.getBoundingClientRect() }));
  mutate();
  for (const { el, rect } of before) {
    // Absent before or absent after: not a move.
    if (!el.isConnected || (!rect.width && !rect.height)) continue;
    const now = el.getBoundingClientRect();
    if (!now.width && !now.height) continue;
    const dx = rect.left - now.left;
    const dy = rect.top - now.top;
    if (!dx && !dy) continue;
    el.animate(
      [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }],
      { duration: ROW_MORPH_MS, easing: ROW_MORPH_EASE },
    );
  }
}

/**
 * Dim a row out before the mutation that removes it, so it leaves rather than
 * vanishing between two frames. Resolves immediately under reduced motion.
 */
export async function fadeOutRow(el: HTMLElement): Promise<void> {
  if (prefersReducedMotion() || !el.isConnected || typeof el.animate !== 'function') return;
  await el
    .animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: 140,
      easing: 'ease-out',
      // Without this the effect stops applying the instant `finished`
      // resolves and the row snaps back to full opacity for one frame before
      // the caller hides it — fade out, flash back, vanish, which is the
      // exact "vanishing between two frames" this helper exists to prevent.
      fill: 'forwards',
    })
    .finished.catch(() => {
      /* cancelled by a later phase change — the mutation still applies */
    });
}

/**
 * Mark `el` as freshly revealed: it fades and rises into the space the height
 * morph just made. The delay lives in CSS so content lands AFTER the space
 * exists.
 */
export function revealRow(el: HTMLElement): void {
  if (prefersReducedMotion()) return;
  // Restart the animation even if the class is already present.
  el.classList.remove('fp-reveal');
  void el.offsetHeight;
  el.classList.add('fp-reveal');
}

/**
 * The most recent crossfade token per element (mirrors `latestMorph` above) —
 * a superseded call's continuation checks it before writing a stale swap, so
 * a caller never needs its own per-element bookkeeping.
 */
const latestCrossfade = new WeakMap<HTMLElement, object>();

/**
 * Crossfade `el` across an arbitrary DOM mutation on the SAME node — the
 * honest fallback where no continuous-shape morph exists for the changing
 * part, so the layout does not shift. Fades out
 * (140ms), runs `mutate()`, then fades back in (160ms, no `fill`) so the node
 * ends on its own stylesheet opacity rather than an inline value pinned by
 * the fade-out's `fill: 'forwards'`. Under reduced motion or without
 * `el.animate` (jsdom, or a hypothetically very old WebKit) `mutate()` runs
 * synchronously with no animation — same contract as `flipRows`.
 *
 * `crossfadeText` and `crossfadeSwap` are this one shape with two different
 * mutations — a text swap, or an arbitrary subtree replacement (the title-bar
 * layer's content).
 */
async function crossfadeMutate(el: HTMLElement, mutate: () => void): Promise<void> {
  if (prefersReducedMotion() || typeof el.animate !== 'function') {
    mutate();
    return;
  }
  const token = {};
  latestCrossfade.set(el, token);
  const fadeOut = el.animate([{ opacity: 1 }, { opacity: 0 }], {
    duration: 140,
    easing: 'ease-out',
    fill: 'forwards',
  });
  await fadeOut.finished.catch(() => { /* cancelled by a later crossfade */ });
  if (latestCrossfade.get(el) !== token) return; // superseded by a later call
  mutate();
  // Release the fade-out's pinned opacity:0 before starting the fade-in, so
  // the fade-in's own (fill-less) end state is what the stylesheet says.
  fadeOut.cancel();
  el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 160, easing: 'ease-out' });
}

/** Crossfade `el`'s text content to `next`. See `crossfadeMutate`. */
export function crossfadeText(el: HTMLElement, next: string): Promise<void> {
  return crossfadeMutate(el, () => { el.textContent = next; });
}

/** Crossfade `el` across an arbitrary subtree replacement (e.g. swapping the
 *  title-bar layer's content). See `crossfadeMutate`. */
export function crossfadeSwap(el: HTMLElement, mutate: () => void): Promise<void> {
  return crossfadeMutate(el, mutate);
}
