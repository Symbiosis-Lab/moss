//#region src/paper/default.ts
function createPaper({ width = 256, height = 256, seed = 90210 } = {}) {
	let a0 = seed;
	const rnd = () => {
		a0 = a0 + 1831565813 | 0;
		let t = Math.imul(a0 ^ a0 >>> 15, 1 | a0);
		t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
		return ((t ^ t >>> 14) >>> 0) / 4294967296;
	};
	const lattice = (n) => {
		const g = new Float32Array(n * n);
		for (let i = 0; i < g.length; i++) g[i] = rnd();
		return g;
	};
	const sm = (t) => t * t * (3 - 2 * t);
	const value = (g, n, x, y) => {
		const gx = x * n, gy = y * n, x0 = Math.floor(gx) % n, y0 = Math.floor(gy) % n, x1 = (x0 + 1) % n, y1 = (y0 + 1) % n, fx = sm(gx - Math.floor(gx)), fy = sm(gy - Math.floor(gy));
		const a = g[y0 * n + x0], b = g[y0 * n + x1], c = g[y1 * n + x0], d = g[y1 * n + x1];
		return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy;
	};
	const octs = [
		4,
		8,
		16,
		32,
		64,
		128
	];
	const fbm = (lats, x, y, o0) => {
		let s = 0, amp = 1, tot = 0;
		for (let o = o0; o < octs.length; o++) {
			s += amp * value(lats[o], octs[o], x, y);
			tot += amp;
			amp *= .55;
		}
		return s / tot;
	};
	const L = [
		0,
		1,
		2,
		3
	].map(() => octs.map(lattice));
	const ch = [
		0,
		1,
		2,
		3
	].map(() => new Float32Array(width * height));
	for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
		const i = y * width + x, u = x / width, v = y / height;
		ch[0][i] = fbm(L[0], u, v, 2) * .7 + .3 * rnd();
		ch[1][i] = fbm(L[1], u, v, 1);
		ch[2][i] = fbm(L[2], u, v, 0);
		ch[3][i] = fbm(L[3], u, v, 3);
	}
	const px = new Uint8Array(width * height * 4);
	for (let k = 0; k < 4; k++) {
		let lo = 1, hi = 0;
		for (const t of ch[k]) {
			lo = Math.min(lo, t);
			hi = Math.max(hi, t);
		}
		for (let i = 0; i < width * height; i++) px[i * 4 + k] = (ch[k][i] - lo) / (hi - lo) * 255;
	}
	return {
		width,
		height,
		data: px
	};
}

//#endregion
export { createPaper as t };