import { createSim, DEFAULT_PRESET } from '../dist/engine/index.mjs';
import { createSphere } from '../dist/sphere/index.mjs';
import { sphereLegAt, DEFAULT_SPHERE_BOUNDS as bounds } from '../dist/model/index.mjs';

const picture = document.querySelector('.picture'), canvas = document.querySelector('canvas');
const first = document.querySelector('#first'), second = document.querySelector('#second');
const caption = document.querySelector('#caption'), journey = document.querySelector('.journey');
const motion = matchMedia('(prefers-reduced-motion: reduce)');
let sim, sphere, sides = [], frame = 0;
function still(message) {
  cancelAnimationFrame(frame); sphere?.dispose(); sides.forEach(side => sim?.freeSide(side)); sim?.dispose();
  sim = sphere = null; sides = [];
  document.body.classList.add('still'); caption.textContent = message;
}
document.querySelector('#again').addEventListener('click', () => scrollTo({ top: 0, behavior: motion.matches ? 'instant' : 'smooth' }));
motion.addEventListener('change', () => { if (motion.matches) still('Hillside and shoreline, at rest.'); });

async function start() {
  if (motion.matches) { still('Hillside and shoreline, at rest.'); return; }
  await Promise.all([first.decode(), second.decode()]);
  const foot = document.createElement('canvas'); foot.width = 820; foot.height = 780;
  foot.getContext('2d').fillRect(0, 0, 820, 780);
  sim = createSim({ canvas, texW: 820, texH: 780, divisor: innerWidth <= 600 ? 4 : 2,
    ground: 'transparent', tint: '#f4efe6',
    rect: () => ({ x: 0, y: 0, w: picture.clientWidth, h: picture.clientHeight }) });
  if (!sim) { still('Hillside and shoreline, at rest.'); return; }
  sphere = createSphere(sim);
  sides = [sim.sceneSide(first, foot), sim.sceneSide(second, foot)];
  sim.onContextLost(() => still('Hillside and shoreline, at rest.'));
  let phase = 'ready', since = 0, p = 0;
  const enter = (next, now) => { phase = next; since = now; };
  function tick(now) {
    const pending = sides.find(side => !sim.sideRecorded(side));
    if (pending) {
      sim.recordSide(pending, DEFAULT_PRESET.stepsPerFrame);
      frame = requestAnimationFrame(tick); return;
    }
    const top = journey.getBoundingClientRect().top;
    const scroll = Math.min(1, Math.max(0, -top / (journey.offsetHeight - innerHeight)));
    if (phase === 'ready' && scroll > .02) enter('gather', now);
    if (phase === 'gather') {
      p = Math.min(1, (now - since) / 700) * bounds.holdStart;
      if (p >= bounds.holdStart) enter('hold', now);
    }
    if (phase === 'hold') {
      const progress = Math.min(1, Math.max(0, (scroll - .12) / .7));
      p = bounds.holdStart + progress * (bounds.holdEnd - bounds.holdStart);
      if (scroll >= .85) enter('release', now);
    }
    if (phase === 'release') {
      p = bounds.holdEnd + Math.min(1, (now - since) / 700) * (1 - bounds.holdEnd);
      if (p >= 1) enter('done', now);
    }
    if (scroll === 0 && phase !== 'gather') { phase = 'ready'; p = 0; }
    const pose = sphereLegAt(p), { ux, uy, k } = pose;
    // Map the fast phases to the wash's existing Out/In envelope.
    const washP = p < bounds.outEnd ? ux * DEFAULT_PRESET.playOut : p > bounds.inStart
      ? 1 - uy * (1 - DEFAULT_PRESET.playIn) : DEFAULT_PRESET.playOut + k * (DEFAULT_PRESET.playIn - DEFAULT_PRESET.playOut);
    sim.playSides(sides[0], ux, sides[1], uy, k, washP);
    sphere.draw(sim.pigmentAt(sides[0], ux), sim.pigmentAt(sides[1], uy), { ...pose, t: now / 1000 });
    picture.classList.add('active');
    caption.textContent = phase === 'hold' ? 'Scroll to change the colours. Pause to watch them breathe.'
      : phase === 'ready' ? 'Scroll to set the colours in motion.' : phase === 'done' ? 'The shoreline, settled.' : 'Pigment, carried by water.';
    frame = requestAnimationFrame(tick);
  }
  frame = requestAnimationFrame(tick);
}
start().catch(error => { console.error(error); still('Hillside and shoreline, at rest.'); });
addEventListener('pagehide', () => still('Hillside and shoreline, at rest.'), { once: true });
