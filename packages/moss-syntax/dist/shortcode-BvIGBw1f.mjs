//#region src/contract/shortcodes.generated.ts
const SHORTCODES = [
	{
		name: "subscribe",
		attrs: [{ name: "button" }, { name: "placeholder" }],
		canonicalTemplate: "subscribe {button=\"${1:Subscribe}\"}\n:::",
		authorable: true
	},
	{
		name: "buttons",
		attrs: [],
		canonicalTemplate: "buttons\n[${1:Get started}](${2:/})\n:::",
		authorable: true
	},
	{
		name: "gallery",
		attrs: [{ name: "cols" }],
		canonicalTemplate: "gallery {cols=${1:3}}\n![](${2:photo.jpg})\n:::",
		authorable: true
	},
	{
		name: "hero",
		attrs: [{
			name: "image",
			assetKinds: ["Image", "Video"]
		}, { name: "wide" }],
		assetAttr: "image",
		canonicalTemplate: "hero {image=${1:photo.jpg}}\n# ${2:Title}\n${3:Subtitle}\n:::",
		authorable: true
	},
	{
		name: "grid",
		attrs: [{ name: "cols" }, { name: "wide" }],
		canonicalTemplate: "grid {cols=${1:2}}\n${2:cell one}\n+++\n${3:cell two}\n:::",
		authorable: true
	},
	{
		name: "recent",
		attrs: [
			{ name: "count" },
			{ name: "since" },
			{ name: "last" }
		],
		canonicalTemplate: "recent {count=${1:5}}\n${2:No posts yet.}\n:::",
		authorable: true
	},
	{
		name: "apply",
		attrs: [{ name: "placeholder" }, { name: "button" }],
		canonicalTemplate: "apply {button=\"${1:Apply}\"}\n:::",
		authorable: false
	}
];

//#endregion
//#region src/shortcode.ts
const SHORTCODE_OPEN_RE = /^(\s*)(:{3,})([A-Za-z][\w-]*)?(.*)$/;
/** True when `m` (a `SHORTCODE_OPEN_RE` match) is a real opener: either it
*  has a name, or its remainder is an attrs block (starts with `{`). Shared
*  by `parse` and `endLeaf` so the two can't drift on what counts as open. */
function isOpenMatch(m) {
	if (!m) return false;
	if (m[3]) return true;
	return m[4].trim().startsWith("{");
}
/** A pure-colon line of arity >=3 is a close fence. */
function isCloseFence(lineText) {
	const t = lineText.trim();
	return t.length >= 3 && /^:+$/.test(t);
}
const shortcodeBlockConfig = {
	defineNodes: [
		"ShortcodeOpenLine",
		"ShortcodeName",
		"ShortcodeAttrs",
		"ShortcodeCloseLine"
	],
	parseBlock: [{
		name: "ShortcodeOpenLine",
		parse(cx, line) {
			const m = SHORTCODE_OPEN_RE.exec(line.text.slice(line.pos));
			if (!isOpenMatch(m)) return false;
			const arity = m[2].length;
			const wsLen = m[1].length;
			const from = cx.lineStart + line.pos + wsLen;
			const lineEnd = cx.lineStart + line.text.length;
			const nameFrom = from + arity;
			const nameTo = nameFrom + (m[3]?.length ?? 0);
			const children = m[3] ? [cx.elt("ShortcodeName", nameFrom, nameTo)] : [];
			if (m[4] && m[4].trim().length) children.push(cx.elt("ShortcodeAttrs", nameTo, lineEnd));
			cx.addElement(cx.elt("ShortcodeOpenLine", from, lineEnd, children));
			cx.nextLine();
			return true;
		},
		endLeaf(_cx, line) {
			return isOpenMatch(SHORTCODE_OPEN_RE.exec(line.text.slice(line.pos)));
		},
		before: "FencedCode"
	}, {
		name: "ShortcodeCloseLine",
		parse(cx, line) {
			if (!isCloseFence(line.text.slice(line.pos))) return false;
			cx.addElement(cx.elt("ShortcodeCloseLine", cx.lineStart + line.pos, cx.lineStart + line.text.length));
			cx.nextLine();
			return true;
		},
		endLeaf(_cx, line) {
			return isCloseFence(line.text.slice(line.pos));
		},
		before: "FencedCode"
	}]
};
/** MIRROR of `attrs::is_key_start`. */
const isKeyStart = (c) => /[A-Za-z]/.test(c);
/** MIRROR of `attrs::is_key_continue`. */
const isKeyContinue = (c) => /[A-Za-z0-9_-]/.test(c);
/**
* MIRROR of `attrs::is_bareword` — Unicode-alphanumeric, not ASCII, so a
* `頭像.png` / `café.jpg` written unquoted reads the same here as in the
* build. `\p{Alphabetic}` + `\p{N}` is Rust's `char::is_alphanumeric`.
*/
const isBareword = (c) => /[:/._-]/.test(c) || /[\p{Alphabetic}\p{N}]/u.test(c);
/** MIRROR of `attrs::match_width_token` — the bare keywords that are NOT an error. */
const WIDTH_TOKENS = new Set([
	"body",
	"wide",
	"page",
	"screen",
	"full"
]);
/**
* Read every `key=value` item out of an attribute block.
*
* `input` must start (after whitespace) with `{`. Returns `null` for any
* structural error, which is the caller-visible spelling of Rust's
* `Err(AttrError)` + `.unwrap_or_default()`: no attributes, not partial ones.
*
* Classes (`.foo`) and the id (`#bar`) are consumed and discarded — this
* reader exists to find asset paths, and neither can hold one.
*/
function parseAttrKvSpans(input) {
	const n = input.length;
	let i = 0;
	const skipWs = () => {
		while (i < n && /\s/.test(input[i])) i++;
	};
	skipWs();
	if (input[i] !== "{") return null;
	i++;
	const out = [];
	for (;;) {
		skipWs();
		if (i >= n) return null;
		const c = input[i];
		if (c === "}") return out;
		if (c === "." || c === "#") {
			i++;
			while (i < n && isBareword(input[i])) i++;
			continue;
		}
		if (!isKeyStart(c)) return null;
		const keyFrom = i;
		i++;
		while (i < n && isKeyContinue(input[i])) i++;
		const keyTo = i;
		const key = input.slice(keyFrom, keyTo);
		skipWs();
		if (input[i] !== "=") {
			if (WIDTH_TOKENS.has(key)) continue;
			return null;
		}
		i++;
		skipWs();
		if (input[i] === "\"") {
			i++;
			const from$1 = i;
			let value = "";
			for (;;) {
				if (i >= n) return null;
				const ch = input[i];
				if (ch === "\"") {
					out.push({
						key,
						value,
						from: from$1,
						to: i,
						keyFrom,
						keyTo
					});
					i++;
					break;
				}
				if (ch === "\\") {
					i++;
					if (i >= n) return null;
					value += input[i];
					i++;
					continue;
				}
				value += ch;
				i++;
			}
			continue;
		}
		if (i >= n || !isBareword(input[i])) return null;
		const from = i;
		while (i < n && isBareword(input[i])) i++;
		out.push({
			key,
			value: input.slice(from, i),
			from,
			to: i,
			keyFrom,
			keyTo
		});
	}
}
/**
* The attribute each shortcode names an asset in, read off the generated
* catalog (Rust SSOT: crates/moss-core/src/contract/shortcodes.rs). This used
* to be a hand copy of a fact `SHORTCODE_CATALOG` also carried by hand;
* both now derive from the same artifact, so there is nothing left to drift
* (`cm-shortcode-lezer.test.ts` still asserts the derivation reads what the
* artifact says).
*
* `gallery` has no `assetAttr` on purpose: its assets are markdown images in
* the BODY, which the Lezer walk already sees.
*/
const ASSET_ATTR_BY_SHORTCODE = new Map(SHORTCODES.flatMap((s) => s.assetAttr ? [[s.name, s.assetAttr]] : []));
/**
* MIRROR of moss-core `media::split_pipe`: everything before the first `|` is
* the path, the rest is display attributes (`cover top`, `color=…`).
*/
function splitPipe(raw) {
	const bar = raw.indexOf("|");
	return bar === -1 ? raw : raw.slice(0, bar);
}
/**
* The asset a shortcode's opening line names, or null.
*
* `args` is everything after `:::name` on that line — exactly the
* `ShortcodeAttrs` node's text. A MIRROR of moss-core `extract_hero::parse_hero`'s
* image-source priority order:
*
*   1. the `image=` attribute inside `{…}` (canonical grammar);
*   2. a bare path written before the `{…}` (`:::hero ./cover.jpg`) — the
*      legacy directive-line form still parsed by the build.
*
* Its priority 3 — a media reference on the first body line — is deliberately
* not read here: that one IS a markdown embed, so live preview already paints
* it and the Lezer walk already resolves it. Reading it again would double
* every hero's hover and lint.
*/
function shortcodeAssetRef(name, args) {
	const attr = ASSET_ATTR_BY_SHORTCODE.get(name);
	if (!attr) return null;
	const brace = args.indexOf("{");
	if (brace !== -1) {
		const hit = parseAttrKvSpans(args.slice(brace))?.find((kv) => kv.key === attr);
		if (hit) {
			const path$1 = splitPipe(hit.value);
			const trimmed$1 = path$1.trimStart();
			const lead = path$1.length - trimmed$1.length;
			const target$1 = trimmed$1.trimEnd();
			if (!target$1) return null;
			const from$1 = brace + hit.from + lead;
			return {
				target: target$1,
				from: from$1,
				to: from$1 + target$1.length
			};
		}
	}
	const path = splitPipe(brace === -1 ? args : args.slice(0, brace));
	const trimmed = path.trimStart();
	const target = trimmed.trimEnd();
	if (!target) return null;
	const from = path.length - trimmed.length;
	return {
		target,
		from,
		to: from + target.length
	};
}

//#endregion
export { shortcodeAssetRef as a, parseAttrKvSpans as i, isCloseFence as n, shortcodeBlockConfig as o, isOpenMatch as r, SHORTCODES as s, SHORTCODE_OPEN_RE as t };