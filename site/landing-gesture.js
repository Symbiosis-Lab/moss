(() => {
  // The scroll gesture record, pulled out of site/landing.js so it can run
  // without a DOM: same record, same derivation, same release-edge
  // detection, same transition rules, byte for byte, unit-tested by
  // scripts/gesture-model.test.mjs under plain node:test. landing.js keeps
  // everything that has to touch the page -- the wheel/pointer/touch
  // listeners, scrollTo, requestAnimationFrame, and the settle spring --
  // and calls createLandingGesture() once to get this record back.
  function createLandingGesture() {
    // What the hand is doing, entirely DERIVED from the raw state directly below --
    // no caller sets `kind` itself, so two input channels can no longer overwrite
    // what the other means (M4: a wheel coast tick used to clear `via` and drop a
    // live touch hold). `contact.wheelUntil` is a deadline, not a flag: a push
    // extends it to now+HOLD_GAP, so a continuous wheel stream keeps `held` true
    // on its own -- a settle can never hijack an in-flight gesture between ticks
    // (the earlier one-line park fix overshot to scene 0 under load for exactly
    // this reason) -- and a real gap lets it lapse without anyone writing a demotion.
    const contact = { direct: false, wheelUntil: 0, wasHeld: false };
    let run = null, restOwed = false;
    const gestureHeld = (now) => contact.direct || now < contact.wheelUntil;
    // No cache: every reader, production and the harness alike, calls this with
    // its own `now` rather than trusting a value some earlier frame wrote. Unit 1
    // cached this once a frame and read the cache everywhere but state(), which
    // brought M1's own staleness bug back on the other side -- production and
    // state() could disagree about what the hand was doing right now (unit 1b
    // review). Two comparisons is cheap enough to just run again.
    const gestureKind = (now) => run ? 'settling' : gestureHeld(now) ? 'held' : restOwed ? 'coasting' : 'idle';
    // `reason` is the only thing still published as a single cached field --
    // written immediately by whichever setter changes something, nothing else
    // depends on its timing the way kind's callers did.
    const gesture = { reason: null };
    // Run unconditionally from the one watchScroll loop, before it dispatches to
    // any role, so a wheel hold expires the same way in watchScrollDesktop,
    // watchScrollNative or watchScrollReduced alike (M1 -- the old staleness
    // check lived in watchScrollDesktop only, so a held read stuck forever once
    // nativeScroll() routed elsewhere: the 4<->5 park at xf≈0.5). Also the one
    // place that notices a hold lapsing with nothing else arriving to arm a rest
    // for it -- a flick across a boundary with no further tick. contact.wasHeld
    // is the raw value tickGesture itself saw last time, not a read of any
    // derived field -- so the edge it detects can never be masked by something
    // else's idea of what kind was between two of its own calls.
    function tickGesture(now) {
      const held = gestureHeld(now);
      if (contact.wasHeld && !held) { restOwed = true; gesture.reason = 'release'; }
      contact.wasHeld = held;
    }
    // Cancels an outgoing settle itself, so no setter depends on running before
    // another to avoid orphaning a live rAF that keeps writing scrollY while direct
    // manipulation reads it back as the reader's own motion (M2 -- cancelSettle
    // used to be the only place this happened, correct only because it ran ahead
    // of holdOn/the wheel listener by listener order, an undocumented dependency).
    function cancelRun() { if (run) { run.cancel = true; run = null; } }
    // Arms a settle without disturbing a hold or a running one -- for a caller with
    // no gesture of its own to report (fiveOn, reduced motion) or a wheel tick read
    // as momentum. `coasting` is this, renamed and derived rather than set directly.
    function armSettle(reason) {
      const now = performance.now();
      if (!gestureHeld(now) && !run) { restOwed = true; gesture.reason = reason; }
    }
    // contact.direct is its own field, untouched by the wheel channel, so a coast
    // tick mid-hold (M4) has nothing shared to clobber -- holdOff needs no guard.
    function holdDirect() { cancelRun(); contact.direct = true; gesture.reason = 'hold'; }
    function holdOff() { contact.direct = false; }
    return {
      contact, gesture,
      // run/restOwed stay primitives, not object fields, in the moved code
      // itself (unchanged from before the move) -- exposed here as an
      // accessor pair so landing.js's settleTo/settleAtRest, which still own
      // starting and consuming them, read and write the same two bindings
      // this closure does rather than a stale copy taken at call time.
      get run() { return run; }, set run(value) { run = value; },
      get restOwed() { return restOwed; }, set restOwed(value) { restOwed = value; },
      gestureHeld, gestureKind, tickGesture, cancelRun, armSettle, holdDirect, holdOff,
    };
  }
  if (typeof module !== 'undefined') module.exports = { createLandingGesture };
  if (typeof window !== 'undefined') window.LandingGesture = createLandingGesture;
})();
