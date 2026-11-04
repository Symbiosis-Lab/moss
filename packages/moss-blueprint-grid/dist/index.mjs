//#region src/index.ts
const EPSILON = 1e-10;
const MIN_ALPHA = .01;
/**
* Pixel pitch of the grid — horizontal lines are stroked at y = k × this.
*
* Exported so a page can align its own marks to a real grid line. Two
* constants would drift, and the failure would be silent: a mark floating one
* pixel off a line still looks deliberate.
*/
const GRID_SPACING = 40;
function createParticles(count, speed) {
	return Array.from({ length: count }, () => ({
		x: Math.random(),
		y: Math.random(),
		vx: (Math.random() - .5) * speed * 2,
		vy: (Math.random() - .5) * speed * 2
	}));
}
function updatePhysics(particles, mouseX, mouseY, config) {
	const n = particles.length;
	for (let i = 0; i < n; i++) {
		const p = particles[i];
		for (let j = i + 1; j < n; j++) {
			const q = particles[j];
			const dx = q.x - p.x;
			const dy = q.y - p.y;
			const distSq = dx * dx + dy * dy + EPSILON;
			const force = config.repulsion / distSq;
			const fx = force * dx;
			const fy = force * dy;
			p.vx -= fx;
			p.vy -= fy;
			q.vx += fx;
			q.vy += fy;
			const dist = Math.sqrt(distSq);
			const tangent = 4e-7 / distSq;
			const tx = tangent * (-dy / dist);
			const ty = tangent * (dx / dist);
			p.vx += tx;
			p.vy += ty;
			q.vx -= tx;
			q.vy -= ty;
		}
		const mdx = mouseX - p.x;
		const mdy = mouseY - p.y;
		const mDistSq = mdx * mdx + mdy * mdy;
		const mDist = Math.sqrt(mDistSq + EPSILON);
		p.vx += config.attraction * mdx * mDistSq;
		p.vy += config.attraction * mdy * mDistSq;
		const mouseRepulse = 3e-5 * Math.exp(-mDist / .005);
		p.vx -= mouseRepulse * (mdx / mDist);
		p.vy -= mouseRepulse * (mdy / mDist);
		p.vx *= config.dampening;
		p.vy *= config.dampening;
		const speedSq = p.vx * p.vx + p.vy * p.vy;
		if (speedSq > config.speed * config.speed) {
			const scale = config.speed / Math.sqrt(speedSq);
			p.vx *= scale;
			p.vy *= scale;
		}
		p.x += p.vx;
		p.y += p.vy;
		if (p.x < 0) p.x += 1;
		else if (p.x > 1) p.x -= 1;
		if (p.y < 0) p.y += 1;
		else if (p.y > 1) p.y -= 1;
	}
}
const DEFAULT_CONFIG = {
	particleCount: 12,
	speed: .0025,
	repulsion: 2e-7,
	attraction: .006,
	dampening: .985,
	gridSpacing: GRID_SPACING,
	sigmaPerp: .018,
	sigmaParallel: .18,
	segmentLength: 8,
	minLineWidth: .1,
	maxLineWidth: .4
};
/**
* The events that move the point the particles swing around. Pointer events
* stop once a touch turns into a scroll, so touches are read as well: a finger
* leads the particles as a cursor does.
*/
const POINTER_EVENTS = [
	"pointermove",
	"touchstart",
	"touchmove"
];
/** The viewport position an event from `POINTER_EVENTS` reports, if any. */
function pointerPoint(e) {
	const touches = e.touches;
	if (touches) return touches[0] ?? null;
	const m = e;
	return typeof m.clientX === "number" ? m : null;
}
function startBlueprintGrid(container, options = {}) {
	const canvas = options.canvas ?? container.querySelector("#blueprint-canvas");
	if (!canvas) return () => {};
	let rawCtx;
	try {
		rawCtx = canvas.getContext("2d");
	} catch {
		return () => {};
	}
	if (!rawCtx) return () => {};
	const ctx = rawCtx;
	const config = DEFAULT_CONFIG;
	const maxAlpha = options.maxAlpha ?? .25;
	const particles = createParticles(config.particleCount, config.speed);
	let mouseX = .5;
	let mouseY = .5;
	let rafId = 0;
	let width = 0;
	let height = 0;
	const twoSigmaPerpSq = 2 * config.sigmaPerp * config.sigmaPerp;
	const twoSigmaParallelSq = 2 * config.sigmaParallel * config.sigmaParallel;
	const crossings = [];
	let isDark = document.documentElement.getAttribute("data-theme") === "dark";
	const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches === true;
	const themeObserver = new MutationObserver(() => {
		isDark = document.documentElement.getAttribute("data-theme") === "dark";
		if (reduceMotion) paint();
	});
	themeObserver.observe(document.documentElement, {
		attributes: true,
		attributeFilter: ["data-theme"]
	});
	const observer = new ResizeObserver((entries) => {
		for (const entry of entries) {
			width = entry.contentRect.width;
			height = entry.contentRect.height;
			canvas.width = width * devicePixelRatio;
			canvas.height = height * devicePixelRatio;
			ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
		}
		if (reduceMotion) paint();
	});
	observer.observe(container);
	const pointerTarget = options.pointerTarget ?? container;
	const onPointer = (e) => {
		const p = pointerPoint(e);
		if (!p) return;
		const rect = container.getBoundingClientRect();
		mouseX = (p.clientX - rect.left) / rect.width;
		mouseY = (p.clientY - rect.top) / rect.height;
	};
	for (const type of POINTER_EVENTS) pointerTarget.addEventListener(type, onPointer, { passive: true });
	function paint() {
		options.onFrame?.();
		ctx.clearRect(0, 0, width, height);
		if (width === 0 || height === 0) return;
		ctx.strokeStyle = options.color?.() || (isDark ? "rgb(90,155,255)" : "rgb(20,60,130)");
		paintLines(true);
		paintLines(false);
	}
	function paintLines(horizontal) {
		const along = horizontal ? width : height;
		const across = horizontal ? height : width;
		const step = config.gridSpacing / across;
		const segLen = config.segmentLength / along;
		for (let line = step; line < 1; line += step) {
			crossings.length = 0;
			for (const p of particles) {
				const dPerp = (horizontal ? p.y : p.x) - line;
				const perpAlpha = Math.exp(-(dPerp * dPerp) / twoSigmaPerpSq);
				if (perpAlpha >= MIN_ALPHA) crossings.push({
					at: horizontal ? p.x : p.y,
					perpAlpha
				});
			}
			if (crossings.length === 0) continue;
			const c = line * across;
			for (let s = 0; s < 1; s += segLen) {
				const mid = s + segLen / 2;
				let sum = 0;
				for (const x of crossings) {
					const d = mid - x.at;
					sum += x.perpAlpha * Math.exp(-(d * d) / twoSigmaParallelSq);
				}
				const k = Math.min(sum, 1);
				if (k < MIN_ALPHA) continue;
				const a0 = s * along;
				const a1 = Math.min((s + segLen) * along, along);
				ctx.globalAlpha = k * maxAlpha;
				ctx.lineWidth = config.minLineWidth + k * (config.maxLineWidth - config.minLineWidth);
				ctx.beginPath();
				if (horizontal) {
					ctx.moveTo(a0, c);
					ctx.lineTo(a1, c);
				} else {
					ctx.moveTo(c, a0);
					ctx.lineTo(c, a1);
				}
				ctx.stroke();
			}
		}
	}
	function frame() {
		updatePhysics(particles, mouseX, mouseY, config);
		paint();
		rafId = requestAnimationFrame(frame);
	}
	if (!reduceMotion) rafId = requestAnimationFrame(frame);
	return () => {
		cancelAnimationFrame(rafId);
		observer.disconnect();
		themeObserver.disconnect();
		for (const type of POINTER_EVENTS) pointerTarget.removeEventListener(type, onPointer);
	};
}

//#endregion
export { GRID_SPACING, createParticles, pointerPoint, startBlueprintGrid, updatePhysics };