import { a as vonMisesAxial, i as seededRandom, n as lognormal, r as normal } from "./default-CV2Smisg.mjs";

//#region src/paper/recipe.ts
const CELLULOSE_DENSITY = 1500;
const coarseness = (f) => 2 * f.width * f.wall * CELLULOSE_DENSITY;
const fibreThickness = (f) => 2 * f.wall;
const minFibreWidth = (r) => Math.min(...r.furnish.map((f) => f.width));
const latticeCell = (recipe) => minFibreWidth(recipe) / 2;
/** Two cell sizes within this share of each other are the same cell. */
const CELL_MATCH = .05;
const isFine = (recipe, cell) => Math.abs(cell - latticeCell(recipe)) <= CELL_MATCH * latticeCell(recipe);
/** A furnish property averaged by mass fraction. */
const furnishMean = (r, of) => r.furnish.reduce((s, f) => s + f.massFraction * of(f), 0);
const meanFibreLength = (r) => furnishMean(r, (f) => f.lengthMean);
const meanFibreRadius = (r) => furnishMean(r, (f) => f.width / 2);
const deg = (d) => d * Math.PI / 180;
const COTTON = {
	name: "cotton",
	lengthMean: .0015,
	lengthLogSd: .4,
	width: 2e-5,
	wall: 4e-6,
	massFraction: 1
};
const PTEROCELTIS = {
	name: "pteroceltis",
	lengthMean: .00245,
	lengthLogSd: .25,
	width: 11e-6,
	wall: 25e-7,
	massFraction: .7
};
const RICE_STRAW = {
	name: "rice-straw",
	lengthMean: .0012,
	lengthLogSd: .5,
	width: 1e-5,
	wall: 2e-6,
	massFraction: .3
};
/** Felt batt fibre, solid round: wall = width / 2 makes thickness equal width. */
const FELT_FIBRE = {
	name: "felt",
	lengthMean: .005,
	lengthLogSd: .3,
	width: 3e-5,
	wall: 15e-6,
	massFraction: 1
};
const COTTON_CLOSURE = {
	cell: .0005291666666666666,
	mean: 421.29,
	slope: 170.82,
	residual: .0287,
	residualCorrelation: .406
};
const XUAN_CLOSURES = {
	unsized: {
		cell: .0005291666666666666,
		mean: 365.81,
		slope: -99.31,
		residual: .0446,
		residualCorrelation: .183
	},
	sized: {
		cell: .0005291666666666666,
		mean: 359.16,
		slope: -85.39,
		residual: .0441,
		residualCorrelation: .183
	}
};
const cottonFelt = (imprintDepth) => ({
	fibre: FELT_FIBRE,
	grammage: .2,
	conformity: 387e-7,
	imprintDepth
});
const cotton = (felt, press) => ({
	furnish: [COTTON],
	grammage: .3,
	flocculation: .6,
	flexibility: .8235,
	machineBias: 0,
	felt,
	press,
	contactAngle: deg(85),
	densityClosures: [COTTON_CLOSURE]
});
const xuan = (grammage, compaction, contactAngle, closure) => ({
	furnish: [PTEROCELTIS, RICE_STRAW],
	grammage,
	flocculation: .6,
	flexibility: .4197,
	machineBias: 0,
	felt: null,
	press: {
		compaction,
		polishLength: 0,
		polishStrength: 0
	},
	contactAngle,
	densityClosures: [closure]
});
const PRESETS = {
	"cotton-rough": cotton(cottonFelt(6e-5), {
		compaction: .875,
		polishLength: 0,
		polishStrength: 0
	}),
	"cotton-cold-press": cotton(cottonFelt(3e-5), {
		compaction: .8,
		polishLength: 2e-4,
		polishStrength: .3
	}),
	"cotton-hot-press": cotton(cottonFelt(8e-6), {
		compaction: .7,
		polishLength: .001,
		polishStrength: .9
	}),
	"xuan-unsized": xuan(.0323, 1, 0, XUAN_CLOSURES.unsized),
	"xuan-sized": xuan(.0343, .7163, deg(85.2), XUAN_CLOSURES.sized)
};
const PRESET_TARGETS = {
	"cotton-rough": {
		porosity: .68,
		source: "est.: looser than cold-press, which is unpressed felt-dried"
	},
	"cotton-cold-press": {
		porosity: .65,
		source: "mould-made cotton rag at 0.52 g/cm³: 1 − 0.52/1.5"
	},
	"cotton-hot-press": {
		porosity: .6,
		source: "est.: denser than cold-press"
	},
	"xuan-unsized": {
		porosity: .755,
		source: "Shao et al. 2019, RSC Advances 9(69)"
	},
	"xuan-sized": {
		porosity: .664,
		source: "Shao et al. 2019, RSC Advances 9(69)"
	}
};

//#endregion
//#region src/paper/deposit.ts
const BARE_FRACTION = .5;
const wrap = (i, n) => (i % n + n) % n;
const grow = (a) => {
	const b = new Int32Array(a.length * 2);
	b.set(a);
	return b;
};
/** Visits each cell a segment crosses with the share of its length inside that cell (Amanatides & Woo 1987). */
function crossCells(x0, y0, x1, y1, visit) {
	const dx = x1 - x0, dy = y1 - y0, sx = Math.sign(dx), sy = Math.sign(dy);
	let x = Math.floor(x0), y = Math.floor(y0), t = 0;
	let tx = dx > 0 ? (x + 1 - x0) / dx : dx < 0 ? (x - x0) / dx : Infinity;
	let ty = dy > 0 ? (y + 1 - y0) / dy : dy < 0 ? (y - y0) / dy : Infinity;
	const stepX = dx !== 0 ? Math.abs(1 / dx) : Infinity, stepY = dy !== 0 ? Math.abs(1 / dy) : Infinity;
	for (;;) {
		const next = Math.min(tx, ty, 1);
		if (next > t) visit(x, y, next - t);
		if (next >= 1) return;
		t = next;
		if (tx <= ty) {
			x += sx;
			tx += stepX;
		} else {
			y += sy;
			ty += stepY;
		}
	}
}
/** Lowest profile at or above `floor` whose neighbouring samples differ by at most `drop`; in place. */
function drapeProfile(floor, n, drop) {
	for (let i = 1; i < n; i++) floor[i] = Math.max(floor[i], floor[i - 1] - drop);
	for (let i = n - 2; i >= 0; i--) floor[i] = Math.max(floor[i], floor[i + 1] - drop);
}
function depositFibres(p, grid, rng) {
	const { width: W, height: H, cell: c } = grid;
	const n = W * H, cellArea = c * c, area = n * cellArea;
	const mass = new Float32Array(n), surface = new Float32Array(n);
	const oxx = new Float32Array(n), oxy = new Float32Array(n), oyy = new Float32Array(n), radiusMass = new Float32Array(n);
	const weights = p.furnish.map((f) => f.massFraction / (f.lengthMean * coarseness(f)));
	const total = weights.reduce((a, b) => a + b, 0);
	const cumulative = [];
	let acc = 0;
	for (const w of weights) {
		acc += w / total;
		cumulative.push(acc);
	}
	const target = p.grammage * area, maxLength = Math.min(W, H) * c / 2;
	let idx = new Int32Array(4096), bin = new Int32Array(4096), floor = new Float64Array(1024), deposited = 0;
	while (deposited < target) {
		const u = rng();
		let k = 0;
		while (k < cumulative.length - 1 && u > cumulative[k]) k++;
		const f = p.furnish[k];
		const length = Math.max(c, Math.min(lognormal(rng, f.lengthMean, f.lengthLogSd), maxLength));
		const theta = vonMisesAxial(rng, p.machineBias);
		const cx = rng() * W, cy = rng() * H, dx = Math.cos(theta), dy = Math.sin(theta);
		const steps = Math.max(1, Math.ceil(length / c)), across = Math.max(1, Math.round(f.width / c));
		if (p.flocculation < 1) {
			let local = 0;
			const back = Math.floor((across - 1) / 2), forward = across - 1 - back;
			for (let yy = -back; yy <= forward; yy++) for (let xx = -back; xx <= forward; xx++) local += mass[wrap(Math.floor(cy) + yy, H) * W + wrap(Math.floor(cx) + xx, W)];
			if (local / (across * across) < BARE_FRACTION * deposited / area && rng() >= p.flocculation) continue;
		}
		const fibreMass = coarseness(f) * length, r = f.width / 2;
		const add = (q, dm) => {
			mass[q] += dm;
			oxx[q] += dm * dx * dx;
			oxy[q] += dm * dx * dy;
			oyy[q] += dm * dy * dy;
			radiusMass[q] += dm * r;
		};
		if (p.drape) {
			const hl = Math.max(length / c, 1) / 2, hw = Math.max(f.width / c, 1) / 2;
			const ey = Math.abs(dy) * hl + Math.abs(dx) * hw;
			let count = 0;
			for (let y = Math.ceil(cy - ey - .5); y <= Math.floor(cy + ey - .5); y++) {
				const py = y + .5 - cy;
				let lo = -Infinity, hi = Infinity;
				if (Math.abs(dx) > 1e-9) {
					const a = (-hl - py * dy) / dx, b = (hl - py * dy) / dx;
					lo = Math.max(lo, Math.min(a, b));
					hi = Math.min(hi, Math.max(a, b));
				} else if (Math.abs(py * dy) > hl) continue;
				if (Math.abs(dy) > 1e-9) {
					const a = (py * dx - hw) / dy, b = (py * dx + hw) / dy;
					lo = Math.max(lo, Math.min(a, b));
					hi = Math.min(hi, Math.max(a, b));
				} else if (Math.abs(py * dx) > hw) continue;
				for (let x = Math.ceil(cx + lo - .5); x <= Math.floor(cx + hi - .5); x++) {
					if (count === idx.length) {
						idx = grow(idx);
						bin = grow(bin);
					}
					const along = (x + .5 - cx) * dx + py * dy;
					idx[count] = wrap(y, H) * W + wrap(x, W);
					bin[count++] = Math.min(steps - 1, Math.max(0, Math.floor((along + hl) / (2 * hl) * steps)));
				}
			}
			if (count === 0) {
				idx[0] = wrap(Math.floor(cy), H) * W + wrap(Math.floor(cx), W);
				bin[0] = 0;
				count = 1;
			}
			const t = fibreThickness(f);
			if (floor.length < steps) floor = new Float64Array(steps * 2);
			floor.fill(0, 0, steps);
			for (let k$1 = 0; k$1 < count; k$1++) floor[bin[k$1]] = Math.max(floor[bin[k$1]], surface[idx[k$1]]);
			drapeProfile(floor, steps, p.flexibility * t * (length / steps) / f.width);
			for (let k$1 = 0; k$1 < count; k$1++) {
				const q = idx[k$1], top = floor[bin[k$1]] + t;
				if (top > surface[q]) surface[q] = top;
			}
			const dm = fibreMass / count / cellArea;
			for (let k$1 = 0; k$1 < count; k$1++) add(idx[k$1], dm);
		} else {
			const half = length / c / 2;
			crossCells(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half, (x, y, share) => add(wrap(y, H) * W + wrap(x, W), fibreMass * share / cellArea));
		}
		deposited += fibreMass;
	}
	return {
		grid,
		mass,
		surface,
		oxx,
		oxy,
		oyy,
		radiusMass
	};
}

//#endregion
//#region src/paper/filter.ts
function boxPass(src, dst, W, H, r, alongX) {
	const n = alongX ? W : H, lines = alongX ? H : W;
	const rr = Math.min(r, Math.floor((n - 1) / 2));
	const span = 2 * rr + 1;
	for (let l = 0; l < lines; l++) {
		const at = (i) => {
			const k = (i % n + n) % n;
			return alongX ? l * W + k : k * W + l;
		};
		let acc = 0;
		for (let i = -rr; i <= rr; i++) acc += src[at(i)];
		for (let i = 0; i < n; i++) {
			dst[at(i)] = acc / span;
			acc += src[at(i + rr + 1)] - src[at(i - rr)];
		}
	}
}
function boxBlurWrap(src, width, height, radius) {
	const r = Math.max(0, Math.round(radius));
	if (r === 0) return src.slice();
	const tmp = new Float32Array(src.length), out = new Float32Array(src.length);
	boxPass(src, tmp, width, height, r, true);
	boxPass(tmp, out, width, height, r, false);
	return out;
}
/** Three box passes approximate a Gaussian of standard deviation sigma cells (Wells 1986). */
function gaussianBlurWrap(src, width, height, sigma) {
	const r = sigma > 0 ? Math.round((Math.sqrt(4 * sigma * sigma + 1) - 1) / 2) : 0;
	if (r === 0) return src.slice();
	return boxBlurWrap(boxBlurWrap(boxBlurWrap(src, width, height, r), width, height, r), width, height, r);
}

//#endregion
//#region src/paper/fields.ts
const WATER_SURFACE_TENSION = .0726;
const WATER_VISCOSITY = .001002;
const HEX = {
	c: 57,
	C: 16 / (9 * Math.PI * Math.sqrt(6)),
	vMax: Math.PI / (2 * Math.sqrt(3))
};
const POROSITY_RANGE = [.02, .98];
/** Sheet porosity: 1 − total mass / (cellulose density · total thickness). */
function bulkPorosity(s) {
	let m = 0, t = 0;
	for (let i = 0; i < s.grammage.length; i++) {
		m += s.grammage[i];
		t += s.thickness[i];
	}
	return 1 - m / (CELLULOSE_DENSITY * t);
}
/** Gebart (1992), hexagonal packing; `solid` is the solid fraction, in (0, 1). */
function gebart(radius, solid) {
	const v = solid, a2 = radius * radius;
	return {
		along: 8 * a2 / HEX.c * (1 - v) ** 3 / (v * v),
		across: v < HEX.vMax ? HEX.C * a2 * (Math.sqrt(HEX.vMax / v) - 1) ** 2.5 : 0
	};
}
function deriveFields(inp) {
	const { width: W, height: H, cell } = inp.grid, n = W * H;
	const r = Math.max(0, Math.round(inp.orientationRadius));
	const smooth = (f) => r > 0 ? gaussianBlurWrap(f, W, H, r / Math.sqrt(3)) : f;
	const sm = smooth(inp.mass), sxx = smooth(inp.oxx), sxy = smooth(inp.oxy), syy = smooth(inp.oyy);
	const make = () => new Float32Array(n);
	const porosity = make(), fibreRadius = make(), axx = make(), axy = make(), ayy = make();
	const kxx = make(), kxy = make(), kyy = make(), capillaryRadius = make(), entryPressure = make(), washburn = make();
	const raw = Math.cos(inp.contactAngle), cos = raw > 1e-12 ? raw : Math.min(0, raw), gamma = WATER_SURFACE_TENSION;
	for (let i = 0; i < n; i++) {
		const m = inp.mass[i], T = Math.max(inp.thickness[i], 1e-12);
		const eps = Math.min(POROSITY_RANGE[1], Math.max(POROSITY_RANGE[0], 1 - m / (CELLULOSE_DENSITY * T)));
		const a = m > 0 ? inp.radiusMass[i] / m : inp.fallbackRadius;
		const ox = sm[i] > 0 ? sxx[i] / sm[i] : .5, oxy = sm[i] > 0 ? sxy[i] / sm[i] : 0, oy = sm[i] > 0 ? syy[i] / sm[i] : .5;
		const { along, across } = gebart(a, 1 - eps);
		const rc = eps * a / (1 - eps);
		porosity[i] = eps;
		fibreRadius[i] = a;
		axx[i] = ox;
		axy[i] = oxy;
		ayy[i] = oy;
		kxx[i] = along * ox + across * (1 - ox);
		kxy[i] = (along - across) * oxy;
		kyy[i] = along * oy + across * (1 - oy);
		capillaryRadius[i] = rc;
		entryPressure[i] = 2 * gamma * cos / rc;
		washburn[i] = Math.sqrt(Math.max(0, rc * gamma * cos / (2 * WATER_VISCOSITY)));
	}
	return {
		width: W,
		height: H,
		cell,
		grammage: inp.mass.slice(),
		thickness: inp.thickness.slice(),
		porosity,
		fibreRadius,
		orientation: {
			xx: axx,
			xy: axy,
			yy: ayy
		},
		permeability: {
			xx: kxx,
			xy: kxy,
			yy: kyy
		},
		capillaryRadius,
		entryPressure,
		washburn,
		contactAngle: new Float32Array(n).fill(inp.contactAngle)
	};
}

//#endregion
//#region src/paper/press.ts
const MIN_REMAINING = .2;
const REFERENCE_CELLS = 1024, REFERENCE_SEED = 2654435769;
/** Relative felt grammage, low-passed at the scale the wet sheet can follow. */
function feltRelief(felt, grid, rng) {
	const low = gaussianBlurWrap(depositFibres({
		furnish: [felt.fibre],
		grammage: felt.grammage,
		flocculation: 1,
		flexibility: 2,
		machineBias: 0,
		drape: false
	}, grid, rng).mass, grid.width, grid.height, felt.conformity / grid.cell);
	let m = 0;
	for (let i = 0; i < low.length; i++) m += low[i];
	m /= low.length;
	for (let i = 0; i < low.length; i++) low[i] = m > 0 ? low[i] / m - 1 : 0;
	return low;
}
/**
* The felt's relief, in units of its RMS at the conformity scale, so an
* imprint depth is one physical dent whatever the cell size. Rescaling each
* grid to unit RMS made a coarse felt 1.8 times deeper than a fine felt
* averaged to the same cells, since averaging lowers the RMS and the
* rescale put it back. The scale is measured once on a fine tile.
*/
function feltSurface(felt, grid, rng) {
	const sd = referenceScale(felt), out = feltRelief(felt, grid, rng);
	for (let i = 0; i < out.length; i++) out[i] /= sd;
	return out;
}
const scales = /* @__PURE__ */ new Map();
function referenceScale(felt) {
	const key = JSON.stringify([
		felt.fibre,
		felt.grammage,
		felt.conformity
	]);
	let sd = scales.get(key);
	if (sd === void 0) {
		const ref = feltRelief(felt, {
			width: REFERENCE_CELLS,
			height: REFERENCE_CELLS,
			cell: Math.min(felt.conformity, felt.fibre.width) / 2
		}, seededRandom(REFERENCE_SEED));
		let v = 0;
		for (let i = 0; i < ref.length; i++) v += ref[i] * ref[i];
		sd = Math.sqrt(v / ref.length) || 1;
		if (scales.size > 64) scales.clear();
		scales.set(key, sd);
	}
	return sd;
}
function pressSheet(initial, felt, imprintDepth, press, grid) {
	const out = new Float32Array(initial.length);
	for (let i = 0; i < out.length; i++) {
		const dent = felt ? imprintDepth * felt[i] : 0;
		out[i] = Math.max(initial[i] * MIN_REMAINING, initial[i] - dent) * press.compaction;
	}
	if (press.polishStrength > 0 && press.polishLength > 0) {
		const smooth = gaussianBlurWrap(out, grid.width, grid.height, press.polishLength / grid.cell);
		for (let i = 0; i < out.length; i++) out[i] += press.polishStrength * (smooth[i] - out[i]);
	}
	return out;
}

//#endregion
//#region src/paper/resample.ts
const MIN_DENSITY = 100;
function wholeFactor(from, to, w, h) {
	const f = to / from, k = Math.round(f);
	if (k < 1 || Math.abs(f - k) > 1e-6 * f || w % k || h % k) throw new RangeError(`resampling ${from} m cells to ${to} m needs a whole factor dividing ${w}×${h}, got ${f}`);
	return k;
}
function resamplePaper(sheet, cell) {
	const k = wholeFactor(sheet.cell, cell, sheet.width, sheet.height);
	const W = sheet.width / k, H = sheet.height / k, n = W * H, inv = 1 / (k * k);
	const z = () => new Float32Array(n);
	const mass = z(), thickness = z(), oxx = z(), oxy = z(), oyy = z(), radiusMass = z();
	for (let y = 0; y < sheet.height; y++) for (let x = 0; x < sheet.width; x++) {
		const s = y * sheet.width + x, d = Math.floor(y / k) * W + Math.floor(x / k), m = sheet.grammage[s];
		mass[d] += m * inv;
		thickness[d] += sheet.thickness[s] * inv;
		oxx[d] += sheet.orientation.xx[s] * m * inv;
		oxy[d] += sheet.orientation.xy[s] * m * inv;
		oyy[d] += sheet.orientation.yy[s] * m * inv;
		radiusMass[d] += sheet.fibreRadius[s] * m * inv;
	}
	let fallback = 0;
	for (let i = 0; i < sheet.fibreRadius.length; i++) fallback += sheet.fibreRadius[i];
	fallback /= sheet.fibreRadius.length;
	return deriveFields({
		grid: {
			width: W,
			height: H,
			cell
		},
		mass,
		thickness,
		oxx,
		oxy,
		oyy,
		radiusMass,
		contactAngle: sheet.contactAngle[0],
		orientationRadius: 0,
		fallbackRadius: fallback
	});
}
function fitDensityClosure(recipe, coarseCell, seed = 1, coarseCells = 16) {
	const k = Math.max(1, Math.round(coarseCell / latticeCell(recipe))), fineCell = coarseCell / k, N = coarseCells * k;
	if (!isFine(recipe, fineCell)) throw new RangeError(`a ${coarseCell} m cell must be within ${CELL_MATCH * 100}% of a whole multiple of the deposit lattice (${latticeCell(recipe)} m) to take a density closure`);
	const dep = depositFibres({
		furnish: recipe.furnish,
		grammage: recipe.grammage,
		flocculation: recipe.flocculation,
		flexibility: recipe.flexibility,
		machineBias: recipe.machineBias,
		drape: true
	}, {
		width: N,
		height: N,
		cell: fineCell
	}, seededRandom(seed));
	const n = coarseCells * coarseCells, m = new Float64Array(n), t = new Float64Array(n);
	for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
		const d = Math.floor(y / k) * coarseCells + Math.floor(x / k);
		m[d] += dep.mass[y * N + x];
		t[d] += dep.surface[y * N + x];
	}
	let mBar = 0;
	for (let i = 0; i < n; i++) mBar += m[i];
	mBar /= n;
	const rho = Array.from(m, (mi, i) => mi / Math.max(t[i], 1e-30)), g = Array.from(m, (mi) => mi / mBar - 1);
	let rBar = 0;
	for (const r of rho) rBar += r;
	rBar /= n;
	let sxy = 0, sxx = 0;
	for (let i = 0; i < n; i++) {
		sxy += g[i] * (rho[i] - rBar);
		sxx += g[i] * g[i];
	}
	const slope = sxx > 0 ? sxy / sxx : 0;
	const res = Array.from(m, (mi, i) => mi > 0 ? t[i] * Math.max(MIN_DENSITY, rBar + slope * g[i]) / mi - 1 : 0);
	let rm = 0;
	for (const r of res) rm += r;
	rm /= n;
	let v = 0, lag = 0;
	for (let y = 0; y < coarseCells; y++) for (let x = 0; x < coarseCells; x++) {
		const a = res[y * coarseCells + x] - rm;
		v += a * a;
		lag += a * (res[y * coarseCells + (x + 1) % coarseCells] - rm) + a * (res[(y + 1) % coarseCells * coarseCells + x] - rm);
	}
	return {
		cell: coarseCell,
		mean: rBar,
		slope,
		residual: Math.sqrt(v / n),
		residualCorrelation: v > 0 ? lag / (2 * v) : 0
	};
}
function closureFor(recipe, cell) {
	const c = recipe.densityClosures.find((d) => Math.abs(d.cell - cell) <= CELL_MATCH * cell);
	if (!c) throw new RangeError(`no density closure for ${cell} m cells; fit one with fitDensityClosure(recipe, ${cell}) and add it to recipe.densityClosures`);
	return c;
}
/**
* Zero-mean, unit-variance noise whose neighbouring cells correlate by `corr`:
* white noise through a separable [b, 1, b] kernel, whose neighbour
* correlation is 2b / (1 + 2b²). That reaches at most 1/√2, which covers
* every residual measured, at sub-cell correlation lengths a Gaussian blur
* cannot express.
*/
function correlatedNoise(width, height, corr, rng) {
	const r = Math.min(Math.max(corr, 0), Math.SQRT1_2), b = r > 0 ? (1 - Math.sqrt(Math.max(0, 1 - 2 * r * r))) / (2 * r) : 0;
	const n = width * height, w = new Float32Array(n), h = new Float32Array(n), out = new Float32Array(n), norm = 1 + 2 * b * b;
	for (let i = 0; i < n; i++) w[i] = normal(rng);
	for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) h[y * width + x] = w[y * width + x] + b * (w[y * width + (x + 1) % width] + w[y * width + (x + width - 1) % width]);
	for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) out[y * width + x] = (h[y * width + x] + b * (h[(y + 1) % height * width + x] + h[(y + height - 1) % height * width + x])) / norm;
	return out;
}
function closureThickness(mass, closure, width, height, rng) {
	let mBar = 0;
	for (let i = 0; i < mass.length; i++) mBar += mass[i];
	mBar /= mass.length;
	const eta = correlatedNoise(width, height, closure.residualCorrelation, rng);
	const out = new Float32Array(mass.length);
	for (let i = 0; i < mass.length; i++) out[i] = mass[i] / Math.max(MIN_DENSITY, closure.mean + closure.slope * (mBar > 0 ? mass[i] / mBar - 1 : 0)) * Math.max(.2, 1 + closure.residual * eta[i]);
	return out;
}

//#endregion
//#region src/paper/generate.ts
const FELT_STREAM = 1540483477;
const RESIDUAL_STREAM = 668265263;
function validateGrid(grid) {
	const { width, height, cell } = grid;
	if (!Number.isInteger(width) || !Number.isInteger(height) || width < 8 || height < 8) throw new RangeError(`paper grids need integer sizes of at least 8 cells, got ${width}×${height}`);
	if (!(cell > 0) || !Number.isFinite(cell)) throw new RangeError(`paper cell size must be a positive number of metres, got ${cell}`);
}
function generatePaper(recipe, grid, seed) {
	validateGrid(grid);
	const lattice = latticeCell(recipe), fine = isFine(recipe, grid.cell);
	if (grid.cell < lattice && !fine) throw new RangeError(`cells of ${grid.cell} m are finer than the deposit lattice (${lattice} m, half the narrowest fibre); flexibility is calibrated on the lattice, so generate at ${lattice} m`);
	const closure = fine ? null : closureFor(recipe, grid.cell);
	const dep = depositFibres({
		furnish: recipe.furnish,
		grammage: recipe.grammage,
		flocculation: recipe.flocculation,
		flexibility: recipe.flexibility,
		machineBias: recipe.machineBias,
		drape: fine
	}, grid, seededRandom(seed));
	const thickness = pressSheet(closure ? closureThickness(dep.mass, closure, grid.width, grid.height, seededRandom(seed ^ RESIDUAL_STREAM)) : dep.surface, recipe.felt ? feltSurface(recipe.felt, grid, seededRandom(seed ^ FELT_STREAM)) : null, recipe.felt?.imprintDepth ?? 0, recipe.press, grid);
	return deriveFields({
		grid,
		mass: dep.mass,
		thickness,
		oxx: dep.oxx,
		oxy: dep.oxy,
		oyy: dep.oyy,
		radiusMass: dep.radiusMass,
		contactAngle: recipe.contactAngle,
		orientationRadius: meanFibreLength(recipe) / (2 * grid.cell),
		fallbackRadius: meanFibreRadius(recipe)
	});
}

//#endregion
//#region src/paper/engine.ts
const CSS_PX = .0254 / 96;
const ENGINE_TEXEL = 2 * CSS_PX;
/**
* Each channel's physical span: byte 0 is `lo` and byte 255 is `hi`, so
* whatever reads the texture can decode it, and two papers keep their
* physical difference. Stretching each sheet to its own percentiles gave
* rough and hot-press cotton the same relief.
*/
const ENGINE_SPANS = {
	relief: {
		lo: -1e-4,
		hi: 1e-4
	},
	absorbency: {
		lo: 0,
		hi: .032
	},
	fibre: {
		lo: -12,
		hi: -9
	}
};
const encode = (v, { lo, hi }) => Math.round(255 * Math.min(1, Math.max(0, (v - lo) / (hi - lo))));
function toEngineChannels(sheet) {
	const n = sheet.width * sheet.height, data = new Uint8Array(n * 4);
	let mean = 0;
	for (let i = 0; i < n; i++) mean += sheet.thickness[i];
	mean /= n;
	for (let i = 0; i < n; i++) {
		data[i * 4] = encode(sheet.thickness[i] - mean, ENGINE_SPANS.relief);
		data[i * 4 + 1] = encode(sheet.washburn[i], ENGINE_SPANS.absorbency);
		data[i * 4 + 2] = encode(Math.log10((sheet.permeability.xx[i] + sheet.permeability.yy[i]) / 2), ENGINE_SPANS.fibre);
		data[i * 4 + 3] = 128;
	}
	return {
		width: sheet.width,
		height: sheet.height,
		data
	};
}
function enginePaper(recipe, { texels = 256, cssPxPerTexel = 2, metresPerCssPx = CSS_PX, seed = 1 } = {}) {
	return toEngineChannels(generatePaper(recipe, {
		width: texels,
		height: texels,
		cell: cssPxPerTexel * metresPerCssPx
	}, seed));
}

//#endregion
export { toEngineChannels as a, closureFor as c, bulkPorosity as d, PRESETS as f, latticeCell as h, enginePaper as i, fitDensityClosure as l, isFine as m, ENGINE_SPANS as n, generatePaper as o, PRESET_TARGETS as p, ENGINE_TEXEL as r, validateGrid as s, CSS_PX as t, resamplePaper as u };