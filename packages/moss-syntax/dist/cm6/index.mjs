import { a as shortcodeAssetRef, i as parseAttrKvSpans, r as isOpenMatch, t as SHORTCODE_OPEN_RE } from "../shortcode-CNckZosN.mjs";
import { RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import { HighlightStyle, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { Decoration, EditorView, ViewPlugin, WidgetType } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { linter } from "@codemirror/lint";

//#region src/cm6/cm-editor-focus.ts
/** The host's report that the user started or stopped working in the editor. */
const setEditorFocusedEffect = StateEffect.define();
/** Install to make reveals follow focus. Starts unfocused, as a newly opened
*  editor is until the user reaches into it. */
const editorFocusField = StateField.define({
	create: () => false,
	update(value, tr) {
		for (const e of tr.effects) if (e.is(setEditorFocusedEffect)) value = e.value;
		return value;
	}
});
/** True when the host installed `editorFocusField` and the editor is not
*  focused: no line is active and no node touches the selection. */
function isRevealSuspended(state) {
	return state.field(editorFocusField, false) === false;
}
/** True when this transaction suspended or resumed reveals — effect-only, so
*  invisible to the doc/selection checks alone. */
function revealSuspensionChanged(tr) {
	return isRevealSuspended(tr.startState) !== isRevealSuspended(tr.state);
}

//#endregion
//#region src/cm6/cm-source-mode.ts
/** Set source mode on/off. Effect-only transactions (no doc change) — the
*  save state machine never sees a toggle. */
const setSourceModeEffect = StateEffect.define();
const sourceModeField = StateField.define({
	create: () => false,
	update(value, tr) {
		for (const e of tr.effects) if (e.is(setSourceModeEffect)) value = e.value;
		return value;
	}
});
/** True when the document is in source mode. Safe on states without the
*  field (non-markdown editors don't register it) — defaults to false. */
function isSourceMode(state) {
	return state.field(sourceModeField, false) ?? false;
}
/**
* True when this transaction flipped source mode. Decoration StateFields and
* ViewPlugins gate their rebuilds on doc/selection changes; a toggle changes
* neither, so every builder that reads the predicates must ALSO rebuild on
* this — otherwise the mode switch would not repaint until the next keystroke.
*/
function sourceModeChanged(tr) {
	return isSourceMode(tr.startState) !== isSourceMode(tr.state);
}
/**
* The one rebuild gate for every consumer of the reveal predicates: did this
* transaction change anything getActiveLines / nodeTouchesSelection reads?
* Doc, selection — and the mode flip and the editor gaining or losing focus,
* which are effect-only and therefore invisible to the doc/selection checks
* alone. Builders compose their own
* extras (treeAdvanced, refsResolved) on top; they must never re-spell this
* set, because a hand-written gate is how four consumers shipped stale
* decorations across the flip (thermo review, 2026-09-01).
*/
function revealInputsChanged(tr) {
	return tr.docChanged || !!tr.selection || sourceModeChanged(tr) || revealSuspensionChanged(tr);
}
/** ViewUpdate-shaped twin of revealInputsChanged, for ViewPlugin update gates. */
function revealInputsChangedIn(update) {
	return update.docChanged || update.selectionSet || update.transactions.some((tr) => sourceModeChanged(tr) || revealSuspensionChanged(tr));
}

//#endregion
//#region src/cm6/cm-active-lines.ts
/** Returns the set of 1-based line numbers that contain any selection range.
*  In source mode: every line of the document. Unfocused: none. */
function getActiveLines(state) {
	const lines = /* @__PURE__ */ new Set();
	if (isSourceMode(state)) {
		for (let l = 1; l <= state.doc.lines; l++) lines.add(l);
		return lines;
	}
	if (isRevealSuspended(state)) return lines;
	for (const range of state.selection.ranges) {
		const startLine = state.doc.lineAt(range.from).number;
		const endLine = state.doc.lineAt(range.to).number;
		for (let l = startLine; l <= endLine; l++) lines.add(l);
	}
	return lines;
}
/** Returns true if any line of the node range overlaps with active lines. */
function isNodeActive(state, from, to, activeLines) {
	const startLine = state.doc.lineAt(from).number;
	const endLine = state.doc.lineAt(to).number;
	for (let l = startLine; l <= endLine; l++) if (activeLines.has(l)) return true;
	return false;
}
/** True when [from, to] touches any line a selection range is on. */
function spanOnActiveLine(state, from, to) {
	return isNodeActive(state, from, to, getActiveLines(state));
}
/**
* True if any selection range overlaps the closed interval [from, to].
*
* Boundary-inclusive (non-strict `<=`): a collapsed caret parked exactly at
* `from` or `to` counts as touching, so a node revealed on edge contact is
* editable. Strict operators are deliberately avoided — they cause the
* "markers re-hide and trap the cursor outside the run" class of bugs.
*
* This is the inline (per-node) reveal predicate — the granular counterpart to
* isNodeActive's per-line test. Used by cm-live-preview (Emphasis/Strong/
* Strikethrough/InlineCode/Link/Wikilink) and cm-link-resolver (link feedback).
*/
function nodeTouchesSelection(state, from, to) {
	if (isSourceMode(state)) return true;
	if (isRevealSuspended(state)) return false;
	return state.selection.ranges.some((r) => from <= r.to && r.from <= to);
}

//#endregion
//#region src/cm6/cm-link-extract.ts
/**
* Walk the full syntax tree of `state` and return one `ExtractedTarget` for
* every navigable link: standard markdown links and wikilinks.
*
* Results are in document order (tree iteration is depth-first, left-to-right).
*/
function extractLinkTargets(state) {
	const results = [];
	syntaxTree(state).iterate({ enter(node) {
		if (node.name === "Link") {
			let child = node.node.firstChild;
			while (child) {
				if (child.name === "URL") {
					const from = child.from;
					const to = child.to;
					results.push({
						target: state.doc.sliceString(from, to),
						from,
						to,
						nodeFrom: node.from,
						nodeTo: node.to
					});
					break;
				}
				child = child.nextSibling;
			}
			return false;
		}
		if (node.name === "Wikilink") {
			const inner = node.node;
			let targetNode = null;
			let closeMark = null;
			let sawOpenMark = false;
			let child = inner.firstChild;
			while (child) {
				if (child.name === "WikilinkTarget") targetNode = {
					from: child.from,
					to: child.to
				};
				else if (child.name === "WikilinkMark") if (sawOpenMark) closeMark = {
					from: child.from,
					to: child.to
				};
				else sawOpenMark = true;
				child = child.nextSibling;
			}
			if (targetNode) {
				const target = state.doc.sliceString(targetNode.from, targetNode.to).trim();
				const labelFrom = targetNode.to + 1;
				if (closeMark != null && state.doc.sliceString(targetNode.to, labelFrom) === "|" && labelFrom < closeMark.from && closeMark) results.push({
					target,
					from: labelFrom,
					to: closeMark.from,
					nodeFrom: node.from,
					nodeTo: node.to
				});
				else results.push({
					target,
					from: targetNode.from,
					to: targetNode.to,
					nodeFrom: node.from,
					nodeTo: node.to
				});
			}
			return false;
		}
	} });
	return results;
}

//#endregion
//#region src/cm6/cm-image-extract.ts
/**
* True for the Lezer node names that mean "an asset embed": a standard
* markdown image `![alt](url)` (`Image`) and a wikilink embed `![[file]]`
* (`WikilinkEmbed`).
*
* ADR-041 invariant: an embed is identified by node NAME. The old
* `node.name === 'Image' && !getChild('URL')` discriminator was a negative
* test four modules re-derived independently; this is the one predicate.
*/
function isEmbedNode(name) {
	return name === "Image" || name === "WikilinkEmbed";
}
const NAMED_WIDTHS = new Set([
	"body",
	"wide",
	"page",
	"screen",
	"full"
]);
/**
* Recognize one width segment (named or percent) → canonical string.
* MIRROR of moss-core `media::parse_image_width` — agrees for every
* canonical/moss-emitted width (named tokens, `NN%`, `NN.N%`; the write path
* only ever produces these). They may diverge on malformed hand-typed input
* (e.g. `"55 %"`, `".5%"`) which this read-side regex rejects but Rust's
* `f64::parse` accepts; harmless since the editor never authors such strings.
* Keep the canonical/clamp/format behavior in sync.
*/
function parseImageWidth(seg) {
	const s = seg.trim();
	if (NAMED_WIDTHS.has(s)) return s === "full" ? "screen" : s;
	const m = /^(\d+(?:\.\d+)?)%$/.exec(s);
	if (m) {
		const v = Math.min(parseFloat(m[1]), 100);
		if (v <= 0) return void 0;
		return `${v}%`;
	}
}
/**
* Pull a width segment out of pipe-delimited text; returns the first match.
* Exported so the live-preview layer (cm-live-preview.ts) reuses this single
* read-side width parser instead of writing a third copy — keeping editor↔build
* parity (this module MIRRORS moss-core `parse_image_width`).
*/
function widthFromPipe(text) {
	for (const seg of text.split("|")) {
		const w = parseImageWidth(seg);
		if (w) return w;
	}
}
/**
* Read an embed node's target, display text and width FROM THE TREE.
*
* This is the single accessor that replaced four independent hand-rolled
* re-scans of the raw source (in this module, cm-live-preview and
* cm-email-guard) — the ADR-036 "parse once, lower to many" rule: never a
* second hand-rolled scan of source the Lezer tree already models.
*
* Discriminates on `node.name` (ADR-041), never on the presence of a `URL`
* child. Returns null when the node is not an embed, is structurally
* incomplete, or has an empty target — `![[]]` (which the slash menu inserts
* on every image verb) and `![]()` must NOT enter the resolve batch.
*/
function embedParts(node, doc) {
	if (node.name === "Image") {
		const url = node.node.getChild("URL");
		if (!url) return null;
		const target$1 = doc.sliceString(url.from, url.to);
		if (!target$1.trim()) return null;
		const closeBracket = node.node.getChild("ImageClosingMark");
		const alt = closeBracket ? doc.sliceString(node.from + 2, closeBracket.from) : "";
		const altMatch = /^!\[([^\]]*)\]/.exec(doc.sliceString(node.from, node.to));
		return {
			syntax: "markdown-image",
			target: target$1,
			targetFrom: url.from,
			targetTo: url.to,
			alt,
			pothole: null,
			width: altMatch ? widthFromPipe(altMatch[1]) : void 0
		};
	}
	if (node.name !== "WikilinkEmbed") return null;
	const t = node.node.getChild("WikilinkTarget");
	if (!t) return null;
	const target = doc.sliceString(t.from, t.to).trim();
	if (!target) return null;
	const hasPothole = doc.sliceString(t.to, t.to + 1) === "|";
	const pothole = hasPothole ? doc.sliceString(t.to + 1, node.to - 2) : "";
	return {
		syntax: "wikilink-embed",
		target,
		targetFrom: node.from,
		targetTo: node.to,
		alt: hasPothole ? pothole.trim() : target,
		pothole: hasPothole ? pothole : null,
		width: hasPothole ? widthFromPipe(pothole) : void 0
	};
}
/** `sort:` values the build recognises; anything else clears the axis. */
const SORT_AXES = new Set([
	"date",
	"weight",
	"title"
]);
/**
* Parse the `key:value,key:value` pothole of a folder embed.
*
* Parity with the build's `parse_params` is GATED, not asserted in prose:
* `crates/moss-core/tests/fixtures/folder-embed-params.vectors.json` is run by
* this module's test AND by `crates/moss-core/tests/folder_embed_params.rs`.
* **Add a vector before you add a key** — a param added on the Rust side alone
* produces a silently missing chip rather than a red test.
*
* The rule most easily got wrong, and the reason the vectors exist: each keyed
* token ASSIGNS, so a later unparseable value CLEARS an earlier good one
* (`sort:weight,sort:weght` → no sort). Bare tokens produce none of the five.
*/
function parseFolderParams(raw) {
	const out = {};
	for (const rawTok of raw.split(",")) {
		const tok = rustTrim(rawTok);
		if (!tok) continue;
		const colon = tok.indexOf(":");
		if (colon === -1) continue;
		const k = rustTrim(tok.slice(0, colon));
		const v = rustTrim(tok.slice(colon + 1));
		switch (k) {
			case "limit":
				out.limit = parseUsize(v);
				break;
			case "sort":
				out.sort = SORT_AXES.has(v) ? v : void 0;
				break;
			case "style":
				out.style = v;
				break;
			case "depth":
				out.depth = v;
				break;
			case "group":
				out.group = v;
				break;
			default: break;
		}
	}
	return out;
}
/**
* Rust `str::trim`, not JS `String.prototype.trim`.
*
* The two disagree on exactly two code points, and a fuzz differential over
* the real parsers found both: `U+0085` NEL is Unicode `White_Space` (Rust
* trims it, JS does not) and `U+FEFF` ZWNBSP is in the JS `WhiteSpace`
* production but has no `White_Space` property (JS trims it, Rust does not).
* The BUILD is the authority for what a param means, so the card mirrors
* Rust — `sort:<NEL>date` sorts by date, `limit:<BOM>3` sets no limit.
*
* The class below is the complete Unicode `White_Space` set, which is fixed
* (no new members since Unicode 4.1), so this cannot drift with a Node
* upgrade. Pinned by the `nel-*` / `bom-*` shared vectors.
*/
const RUST_WS = "\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
const RUST_TRIM_RE = new RegExp(`^[${RUST_WS}]+|[${RUST_WS}]+$`, "g");
function rustTrim(s) {
	return s.replace(RUST_TRIM_RE, "");
}
/** Rust `usize::from_str`: an optional leading `+`, digits only, in range. */
function parseUsize(v) {
	if (!/^\+?\d+$/.test(v)) return void 0;
	const n = BigInt(v);
	if (n > 18446744073709551615n) return void 0;
	return Number(n);
}
/**
* The parsed folder pothole of an embed, or **null** when this is not a
* wikilink embed.
*
* Null for a markdown image `![](/awards/)` on purpose: the build dispatches a
* folder-listing marker only from the `![[…]]` form, so that form lists
* nothing and a card must not claim otherwise.
*/
function folderParamsFromEmbed(parts) {
	if (parts.syntax !== "wikilink-embed") return null;
	return parseFolderParams(parts.pothole ?? "");
}
/** Chip order is fixed so two embeds with identical semantics render
*  identically regardless of typing order. */
const CHIP_ORDER = [
	"style",
	"sort",
	"depth",
	"group",
	"limit"
];
/**
* Canonical `key:value` chips for a parsed pothole.
*
* Derived from the PARSED params, never from the raw text: a token the build
* silently drops produces no chip, and that absence is the diagnostic.
* `limit:0` is omitted because the build's truncation guard is `n > 0` — a
* zero limit does nothing.
*/
function folderChips(p) {
	const out = [];
	for (const key of CHIP_ORDER) {
		const v = p[key];
		if (v === void 0) continue;
		if (key === "limit" && v === 0) continue;
		out.push(`${key}:${v}`);
	}
	return out;
}
/**
* Recognise `[![[x.png]]](/url)` / `[![alt](x.png)](/url)` — a link whose text
* is exactly one embed, which live preview renders as one clickable image.
*
* SHAPE ONLY. Non-null iff all of:
*   1. the node is a `Link`;
*   2. it has a `URL` child spanning a non-empty string (so `[…][ref]` and
*      `[…]()` are rejected — they have no destination to click through to);
*   3. it has exactly one embed child;
*   4. the link's text is ONLY that embed (rejects `[see ![[x]] here](/u)` and
*      `[![[a]] ![[b]]](/u)`);
*   5. the whole thing is on ONE line — the multi-line card form
*      (`[` \n … \n `](url)`) is `findBlockLinks`' domain, and keeping this
*      single-line means this introduces no new overlap with it.
*/
function linkedEmbedOf(link, doc) {
	if (link.name !== "Link") return null;
	const url = link.getChild("URL");
	if (!url) return null;
	const href = doc.sliceString(url.from, url.to).trim();
	if (!href) return null;
	const embed = link.getChild("WikilinkEmbed") ?? link.getChild("Image");
	if (!embed) return null;
	if (doc.sliceString(link.from + 1, embed.from).trim() !== "") return null;
	let k = embed.to;
	while (doc.sliceString(k, k + 1) === " ") k++;
	if (doc.sliceString(k, k + 1) !== "]") return null;
	if (doc.sliceString(link.from, link.to).indexOf("\n") !== -1) return null;
	return {
		embed,
		link: {
			from: link.from,
			to: link.to
		},
		href,
		urlFrom: url.from,
		urlTo: url.to
	};
}
/** The linked-embed unit an embed node belongs to, or null if it is bare. */
function linkUnitOfEmbed(embed, doc) {
	const parent = embed.parent;
	return parent && parent.name === "Link" ? linkedEmbedOf(parent, doc) : null;
}
/**
* The asset a `ShortcodeOpenLine` names in its attributes, as a document-span
* `ImageTarget` — or null when the fence has no name, no attrs, or names no
* asset (every shortcode but `hero` today).
*
* Covered through `extractImageTargets`, which is its only caller — testing it
* directly would need a hand-built cursor and would assert the same thing.
*/
function shortcodeOpenLineAsset(openLine, doc) {
	let name = "";
	let attrsFrom = -1;
	let attrsTo = -1;
	const c = openLine.node.cursor();
	if (c.firstChild()) do
		if (c.name === "ShortcodeName") name = doc.sliceString(c.from, c.to);
		else if (c.name === "ShortcodeAttrs") {
			attrsFrom = c.from;
			attrsTo = c.to;
		}
	while (c.nextSibling());
	if (!name || attrsFrom < 0) return null;
	const ref = shortcodeAssetRef(name, doc.sliceString(attrsFrom, attrsTo));
	if (!ref) return null;
	return {
		target: ref.target,
		from: attrsFrom + ref.from,
		to: attrsFrom + ref.to,
		attr: true
	};
}
/**
* Walk the syntax tree of `state` and return one `ImageTarget` for every
* image or asset embed:
*
*   - `![alt](url)`     → `{ target: url, from: urlFrom, to: urlTo }`
*   - `![[file.png]]`   → `{ target: 'file.png', from: nodeFrom, to: nodeTo }`
*   - `![[f.png|alt]]`  → `{ target: 'f.png', from: nodeFrom, to: nodeTo }`
*
* …plus the one place an asset is named OUTSIDE an embed — a shortcode
* attribute on an opening fence:
*
*   - `:::hero {image=p.jpg}` → `{ target: 'p.jpg', …, attr: true }`
*
* That last case is why this function is the right seam for it rather than a
* fourth scanner: resolution, the broken-asset underline and the hover
* popover all read this one list, so a hero's image joins all three at once.
*
* Results are in document order (depth-first, left-to-right tree iteration) —
* `cm-reference-resolver` feeds them straight to a `RangeSetBuilder`, which
* requires it. A shortcode's target is pushed when its OPEN LINE is entered,
* and everything already pushed lies before that line, so order holds.
*/
function extractImageTargets(state) {
	const out = [];
	syntaxTree(state).iterate({ enter(node) {
		if (node.name === "ShortcodeOpenLine") {
			const ref = shortcodeOpenLineAsset(node, state.doc);
			if (ref) out.push(ref);
			return;
		}
		if (!isEmbedNode(node.name)) return;
		const parts = embedParts(node, state.doc);
		if (parts) out.push({
			target: parts.target,
			from: parts.targetFrom,
			to: parts.targetTo,
			width: parts.width
		});
		return false;
	} });
	return out;
}
/**
* Locate the Image node a block widget belongs to, AT GESTURE TIME.
*
* Block image/embed widgets must not bake absolute document positions into
* their DOM: `eq()` deliberately excludes positions (so edits above the image
* reuse the DOM without an <img> flash), which means a captured `nodeFrom`
* goes stale after any edit above. Instead, the widget asks the view where its
* DOM currently sits (`view.posAtDOM`) and resolves the Image node on that
* line fresh from the syntax tree.
*
* `intraLineOffset` is the Image node's offset WITHIN its source line,
* captured at build time. It is stable under edits above the line and
* disambiguates multiple images anchored to the same line end: the candidate
* whose intra-line offset is closest wins.
*
* Returns null when the widget's position cannot be mapped or no Image node
* exists on the line (e.g. the source was deleted mid-gesture) — callers
* must treat that as "do not commit".
*/
function imageNodeAtWidget(view, dom, intraLineOffset) {
	let pos;
	try {
		pos = view.posAtDOM(dom);
	} catch (_) {
		return null;
	}
	if (pos < 0 || pos > view.state.doc.length) return null;
	const line = view.state.doc.lineAt(pos);
	const candidates = [];
	syntaxTree(view.state).iterate({
		from: line.from,
		to: line.to,
		enter(node) {
			if (!isEmbedNode(node.name)) return;
			candidates.push({
				from: node.from,
				to: node.to
			});
			return false;
		}
	});
	if (candidates.length === 0) return null;
	let best = candidates[0];
	let bestDist = Math.abs(best.from - line.from - intraLineOffset);
	for (const c of candidates) {
		const d = Math.abs(c.from - line.from - intraLineOffset);
		if (d < bestDist) {
			best = c;
			bestDist = d;
		}
	}
	return best;
}
/**
* Resolve the embed node enclosing `pos` — the source span a width rewrite
* replaces. Handles BOTH `![alt](url)` and every `![[…]]` form (folder, PDF,
* video, iframe), because both are embeds by name (ADR-041).
*
* The one owner of the resolveInner-and-climb, so a caller cannot climb for
* `'Image'` only and silently no-op on every wikilink embed.
*
* Returns null when `pos` is not inside an embed.
*/
function embedNodeAt(state, pos) {
	let node = syntaxTree(state).resolveInner(pos, 1);
	while (node && !isEmbedNode(node.name)) node = node.parent;
	return node ? {
		from: node.from,
		to: node.to
	} : null;
}

//#endregion
//#region src/cm6/cm-criticmarkup.ts
const PATTERNS = [
	{
		re: /\{~~([\s\S]*?)~>([\s\S]*?)~~\}/g,
		type: "substitution"
	},
	{
		re: /\{\+\+([\s\S]*?)\+\+\}/g,
		type: "addition"
	},
	{
		re: /\{--([\s\S]*?)--\}/g,
		type: "deletion"
	},
	{
		re: /\{==([\s\S]*?)==\}/g,
		type: "highlight"
	},
	{
		re: /\{>>([\s\S]*?)<<\}/g,
		type: "comment"
	}
];
/**
* Parse CriticMarkup tokens from the document, skipping tokens inside code
* regions (fenced blocks, indented blocks, inline spans) as reported by the
* SYNTAX TREE — the same code regions the editor highlights as code.
*
* @returns Array of parsed marks in document order.
*/
function parseMarks(state) {
	const masked = maskCodeFromTree(state);
	const marks = [];
	for (const { re, type } of PATTERNS) {
		re.lastIndex = 0;
		let m;
		while ((m = re.exec(masked)) !== null) {
			const mark = {
				from: m.index,
				to: m.index + m[0].length,
				type
			};
			if (type === "substitution") mark.mid = m.index + 3 + m[1].length;
			marks.push(mark);
		}
	}
	marks.sort((a, b) => a.from - b.from);
	const result = [];
	let lastEnd = 0;
	for (const m of marks) {
		if (m.from < lastEnd) continue;
		result.push(m);
		lastEnd = m.to;
	}
	return result;
}
/** Lezer node names whose content is literal code — no CriticMarkup inside. */
const CODE_NODE_NAMES = new Set([
	"FencedCode",
	"CodeBlock",
	"InlineCode"
]);
/**
* Return the document text with characters inside code regions replaced by
* spaces (newlines preserved so offsets and line numbers still line up).
* Code regions come from the syntax tree — fenced blocks, INDENTED blocks
* (which the old hand-rolled masking missed), and inline spans — so this
* agrees byte-for-byte with what the editor treats as code.
*/
function maskCodeFromTree(state) {
	const text = state.doc.toString();
	const ranges = [];
	syntaxTree(state).iterate({ enter(node) {
		if (!CODE_NODE_NAMES.has(node.name)) return;
		ranges.push({
			from: node.from,
			to: node.to
		});
		return false;
	} });
	if (ranges.length === 0) return text;
	const out = text.split("");
	for (const { from, to } of ranges) for (let i = from; i < to && i < out.length; i++) if (out[i] !== "\n") out[i] = " ";
	return out.join("");
}
const addMark = Decoration.mark({ class: "cm-criticmarkup-addition" });
const delMark = Decoration.mark({ class: "cm-criticmarkup-deletion" });
const subMark = Decoration.mark({ class: "cm-criticmarkup-substitution" });
const subOldMark = Decoration.mark({ class: "cm-criticmarkup-substitution-old" });
const subNewMark = Decoration.mark({ class: "cm-criticmarkup-substitution-new" });
const hlMark = Decoration.mark({ class: "cm-criticmarkup-highlight" });
const delimMark = Decoration.mark({ class: "cm-criticmarkup-delim" });
/** Emit one mark as muted 3-char delimiters + styled payload between them.
*  Empty payloads (`{++++}`) add no payload range — CM6 rejects empty marks. */
function addDelimited(builder, m, payload) {
	builder.add(m.from, m.from + 3, delimMark);
	if (m.from + 3 < m.to - 3) builder.add(m.from + 3, m.to - 3, payload);
	builder.add(m.to - 3, m.to, delimMark);
}
function buildDecorations(state) {
	const marks = parseMarks(state);
	const builder = new RangeSetBuilder();
	for (let i = 0; i < marks.length; i++) {
		const m = marks[i];
		switch (m.type) {
			case "addition":
				addDelimited(builder, m, addMark);
				break;
			case "deletion":
				addDelimited(builder, m, delMark);
				break;
			case "highlight": {
				const next = marks[i + 1];
				if (!(next && next.type === "comment" && next.from === m.to)) addDelimited(builder, m, hlMark);
				break;
			}
			case "substitution":
				if (m.mid === void 0) {
					addDelimited(builder, m, subMark);
					break;
				}
				builder.add(m.from, m.from + 3, delimMark);
				if (m.from + 3 < m.to - 3) builder.add(m.from + 3, m.to - 3, subMark);
				if (m.from + 3 < m.mid) builder.add(m.from + 3, m.mid, subOldMark);
				if (m.mid + 2 < m.to - 3) builder.add(m.mid + 2, m.to - 3, subNewMark);
				builder.add(m.to - 3, m.to, delimMark);
				break;
		}
	}
	return builder.finish();
}
const REBUILD_TRIGGER = /[{}+\-=<>~`]/;
/** Slice all inserted text from a ChangeSet into a single string. */
function changedInsertedText(changes) {
	let inserted = "";
	changes.iterChanges((_fromA, _toA, _fromB, _toB, ins) => {
		if (ins.length > 0) inserted += ins.toString();
	});
	return inserted;
}
/** Slice all removed text out of the pre-change doc into a single string. */
function changedRemovedText(changes, oldDoc) {
	let removed = "";
	changes.iterChanges((fromA, toA) => {
		if (toA > fromA) removed += oldDoc.sliceString(fromA, toA);
	});
	return removed;
}
/** Number of changed bytes (max contiguous changed window in the new doc). */
function changeSpan(changes) {
	let lo = Infinity;
	let hi = -Infinity;
	changes.iterChanges((_fromA, _toA, fromB, toB) => {
		if (fromB < lo) lo = fromB;
		if (toB > hi) hi = toB;
	});
	return hi < 0 ? 0 : hi - lo;
}
/**
* CM6 extension that highlights CriticMarkup tokens.
*
* Incremental strategy (PATTERN 7): on each transaction, `RangeSet.map` re-
* positions the existing decorations through the change set in O(log n). Only
* when the change inserts/deletes a CriticMarkup or code trigger character —
* or the change is unusually large — do we fall back to a full re-parse.
* Typical typing of plain prose costs one `RangeSet.map`, not a whole-document
* regex scan. Effect-only transactions rebuild only when the incremental
* parse advanced (code masking beyond the initially-parsed region).
*/
function criticmarkupExtension() {
	return ViewPlugin.fromClass(class {
		constructor(view) {
			this._fullBuilds = 0;
			this.decorations = buildDecorations(view.state);
			this._fullBuilds = 1;
		}
		update(update) {
			if (!update.docChanged) {
				if (syntaxTree(update.state) != syntaxTree(update.startState)) {
					this.decorations = buildDecorations(update.state);
					this._fullBuilds++;
				}
				return;
			}
			const { changes } = update;
			const span = changeSpan(changes);
			const inserted = changedInsertedText(changes);
			const removed = changedRemovedText(changes, update.startState.doc);
			const touchesTrigger = REBUILD_TRIGGER.test(inserted) || REBUILD_TRIGGER.test(removed);
			if (span <= 1e3 && !touchesTrigger) {
				this.decorations = this.decorations.map(changes);
				return;
			}
			this.decorations = buildDecorations(update.state);
			this._fullBuilds++;
		}
	}, { decorations: (v) => v.decorations });
}

//#endregion
//#region src/cm6/cm-highlight.ts
const mossHighlight = HighlightStyle.define([
	{
		tag: tags.link,
		color: "var(--moss-hl-link)"
	},
	{
		tag: tags.url,
		color: "var(--moss-hl-link)"
	},
	{
		tag: tags.processingInstruction,
		color: "var(--moss-hl-punct)"
	},
	{
		tag: tags.squareBracket,
		color: "var(--moss-hl-punct)"
	},
	{
		tag: tags.heading,
		fontWeight: "600"
	},
	{
		tag: tags.emphasis,
		fontStyle: "italic"
	},
	{
		tag: tags.strong,
		fontWeight: "600"
	},
	{
		tag: tags.keyword,
		color: "var(--moss-hl-keyword)"
	},
	{
		tag: [tags.string, tags.special(tags.string)],
		color: "var(--moss-hl-string)"
	},
	{
		tag: tags.comment,
		color: "var(--moss-hl-comment)",
		fontStyle: "italic"
	},
	{
		tag: [
			tags.number,
			tags.bool,
			tags.null
		],
		color: "var(--moss-hl-string)"
	},
	{
		tag: [tags.propertyName, tags.attributeName],
		color: "var(--moss-hl-attr)"
	},
	{
		tag: [tags.function(tags.variableName), tags.function(tags.propertyName)],
		color: "var(--moss-hl-keyword)"
	},
	{
		tag: tags.operator,
		color: "var(--moss-hl-operator)"
	},
	{
		tag: tags.tagName,
		color: "var(--moss-hl-keyword)"
	},
	{
		tag: tags.typeName,
		color: "var(--moss-hl-keyword)"
	}
]);
function mossHighlightExtension() {
	return syntaxHighlighting(mossHighlight);
}

//#endregion
//#region src/cm6/cm-shortcode-block.ts
/**
* Block-decoration StateField for shortcode blocks. Reads the SYNCHRONOUS Lezer
* syntax tree and pairs Open↔Close fence markers with an arity stack, so
* positions are always current for tr.state — no stale offsets, no glitches.
*
* Resting (cursor outside block): open fence → micro-tag widget (one-line-tall,
* click-to-focus); body lines tinted (live-preview renders their inline
* markdown); close fence → faint rule. Active (selection head within the paired
* range): all lines raw for editing. Editing a line ABOVE the block does not
* change range-overlap, so the block never toggles.
*
* Large-doc caveat: syntaxTree(state) may be incomplete past the viewport for
* very long docs. The parse worker extends the tree via effect-only
* transactions; both fields rebuild on that advance (see `treeAdvanced`), so
* late-parsed blocks decorate as soon as the parse reaches them. We do not
* assert whole-doc completeness at any single point in time.
*/
const DEFAULT_STRINGS = {
	assetMissingHint: () => "This file isn't in your folder. moss won't publish the site until it is.",
	legacyDividerLabel: () => "old divider — use +++",
	legacyDividerTooltip: () => "The grid still splits here, but --- is the old way to write a cell divider. Replace it with +++."
};
/**
* Pair Open↔Close fence markers with an arity stack. Returns the TOP-LEVEL
* blocks; each carries its nested `children` so the decoration layer can render
* `::::buttons` inside `:::grid` instead of leaking the inner fences as raw
* text. (Nesting parity with the Rust extractor — see cm-shortcode-scanner.)
*/
function collectShortcodeBlocks(state) {
	const stack = [];
	const out = [];
	syntaxTree(state).iterate({ enter(node) {
		if (node.name === "ShortcodeOpenLine") {
			const lineFrom = state.doc.lineAt(node.from).from;
			const m = SHORTCODE_OPEN_RE.exec(state.doc.sliceString(lineFrom, node.to));
			const arity = m ? m[2].length : 3;
			let name = "", attrs = "";
			const c = node.node.cursor();
			if (c.firstChild()) do
				if (c.name === "ShortcodeName") name = state.doc.sliceString(c.from, c.to);
				else if (c.name === "ShortcodeAttrs") attrs = state.doc.sliceString(c.from, c.to).trim();
			while (c.nextSibling());
			stack.push({
				from: lineFrom,
				arity,
				name,
				attrs,
				children: []
			});
		} else if (node.name === "ShortcodeCloseLine") {
			const arity = state.doc.sliceString(node.from, node.to).trim().length;
			for (let i = stack.length - 1; i >= 0; i--) if (stack[i].arity === arity) {
				const open = stack.splice(i)[0];
				const block = {
					from: open.from,
					to: node.to,
					name: open.name,
					attrs: open.attrs,
					closeFrom: node.from,
					arity: open.arity,
					children: open.children
				};
				if (stack.length > 0) stack[stack.length - 1].children.push(block);
				else out.push(block);
				break;
			}
		}
	} });
	return out;
}
/** Flatten a block tree to a document-ordered list (parent before children). */
function flattenBlocks(blocks) {
	const out = [];
	const walk = (b) => {
		out.push(b);
		for (const c of b.children) walk(c);
	};
	for (const b of blocks) walk(b);
	return out;
}
/**
* Char ranges covered by every `:::` fence BODY — the open and close fence
* lines excluded, document order, non-overlapping.
*
* Only TOP-LEVEL blocks contribute a range. A nested `::::buttons` lives inside
* its parent's body by construction, so adding it would produce a second range
* covering ground the parent already covers — and a caller asking "is this
* position in a fence body?" would then have to dedupe. One range per top-level
* block answers that question in one pass.
*
* Exists so the editor can render a fence body at STRUCTURE density (media
* become one-line tokens) while ordinary body text is untouched — see
* docs/archive/2026-08-21-editor-container-media-density-design.md. Lives here
* rather than in the editor so there stays exactly one reader of the block
* tree; two readers of the same structure drift.
*/
function shortcodeBodyRanges(state) {
	const out = [];
	for (const top of collectShortcodeBlocks(state)) {
		const bodyFrom = Math.min(state.doc.lineAt(top.from).to + 1, top.closeFrom);
		if (bodyFrom < top.closeFrom) out.push({
			from: bodyFrom,
			to: top.closeFrom
		});
	}
	return out;
}
/** True when `pos` falls inside any fence body. Linear over the ranges, which
*  number one per top-level block on the page — the caller that runs per
*  syntax-tree node computes the ranges ONCE and passes them in. */
function inShortcodeBody(ranges, pos) {
	return ranges.some((r) => pos >= r.from && pos < r.to);
}
/** Shortcodes whose body splits into cells on a `+++` line (grid, buttons). */
const CELL_DIVIDER_NAMES = new Set(["grid", "buttons"]);
/** A body line that is exactly `+++` — the cell divider (mirrors the Rust
*  `split_grid_cells` / `cells::split_cells` rule; legacy `---` form omitted). */
function isCellDividerLine(text) {
	return text.trim() === "+++";
}
/**
* A body line that is exactly `---` — the OLD spelling of a cell divider.
* Still accepted by the build (`split_grid_cells`), still deprecated, and only
* ever a divider inside `:::grid`: `split_cells`, which every other cell type
* uses, does not know `---` at all. Mirrors `match_divider` in the Rust
* `editor_scan`, which likewise flags `---` only at grid depth.
*/
function isLegacyDividerLine(text) {
	return text.trim() === "---";
}
/** The deepest block whose BODY contains `lineFrom`, or null. */
function innermostBlockAt(lineFrom, subtree) {
	let innermost = null;
	for (const b of subtree) if (b.from < lineFrom && lineFrom < b.closeFrom) {
		if (!innermost || b.from > innermost.from) innermost = b;
	}
	return innermost;
}
/**
* True when the INNERMOST block containing `lineFrom` in its body is a
* cell-dividing shortcode (grid or buttons). A `+++` inside a nested
* `::::buttons` still divides (buttons is a cell type); it renders as literal
* body text only when the innermost container is a NON-cell type (e.g. hero) —
* matching the build, which splits cells per-block, not recursively.
*/
function dividesCellsAt(lineFrom, subtree) {
	const innermost = innermostBlockAt(lineFrom, subtree);
	return innermost != null && CELL_DIVIDER_NAMES.has(innermost.name);
}
/**
* True when a `---` on this line is a deprecated cell divider rather than
* ordinary content — i.e. the innermost enclosing block is a `:::grid`.
* Inside `:::buttons` (or anywhere else) `---` is just text, and hinting
* there would be wrong.
*/
function legacyDividesCellsAt(lineFrom, subtree) {
	return innermostBlockAt(lineFrom, subtree)?.name === "grid";
}
/**
* Count the legacy `---` dividers the Rust `editor_scan` would also count:
* only those directly in a TOP-LEVEL `:::grid` body. Exists so the dev-only
* divergence check can compare the two parsers on dividers, not just on block
* names — the grid-only rule above is now written twice and would otherwise
* drift silently.
*/
function topLevelLegacyDividerCount(state) {
	let n = 0;
	for (const top of collectShortcodeBlocks(state)) {
		if (top.name !== "grid") continue;
		const subtree = flattenBlocks([top]);
		let pos = state.doc.lineAt(top.from).from;
		while (pos <= top.to) {
			const ln = state.doc.lineAt(pos);
			if (isLegacyDividerLine(ln.text) && innermostBlockAt(ln.from, subtree) === top) n++;
			if (ln.to + 1 > top.to) break;
			pos = ln.to + 1;
		}
	}
	return n;
}
/**
* Block-range activation — the editor-wide reveal contract: any selection
* range TOUCHING [from, to] reveals the block's source. Matches the
* selection-overlap activation tables and inline marks use (getActiveLines /
* isNodeActive in cm-live-preview), so Cmd+A and multi-line drags reveal
* shortcode fences like everything else. (Was head-only before, which made
* shortcode blocks the one construct Cmd+A did not reveal.)
*/
function isBlockActive(state, b) {
	return nodeTouchesSelection(state, b.from, b.to);
}
const ICONS = {
	grid: "▦",
	hero: "◉",
	gallery: "⊞",
	buttons: "⊡",
	subscribe: "✉",
	recent: "◷"
};
/** Icon for a nameless `:::{.class}` block ("Pure-CSS region" in
*  shortcode-grammar.md) — distinct from both the named-shortcode icons and
*  the `‹/›` unknown-name fallback, since this isn't unknown, it just has no
*  name to show. */
const CLASS_REGION_ICON = "▢";
const CLASS_TOKEN_RE = /\.[\w-]+/g;
/**
* A nameless block has no name to label the tag with, so the class list
* IS the name — it's the only thing the author typed that identifies the
* block. `{.tagline}` → "tagline"; `{.subscribe-card .wide}` →
* "subscribe-card wide". Returns null when there's no class to show (an
* attrs-only block with no class, which shouldn't normally occur but must
* not crash the tag into a blank label).
*/
function classListLabel(attrs) {
	const classes = attrs.match(CLASS_TOKEN_RE);
	if (!classes || classes.length === 0) return null;
	return classes.map((c) => c.slice(1)).join(" ");
}
const WIDTH_RE = /\b(wide|page|screen|full|body)\b/;
/** Params shown in the tag (on hover), for layout-ambiguous types. */
function tagParams(attrs) {
	const parts = [];
	const perLine = /\bper-line=([^\s"'{}]+)/.exec(attrs);
	const cols = /\bcols=([^\s"'{}]+)/.exec(attrs);
	if (perLine) parts.push(`per-line ${perLine[1]}`);
	else if (cols) parts.push(`cols ${cols[1]}`);
	else {
		const positional = /^([^\s{][^\s]*)/.exec(attrs.trim());
		if (positional && !WIDTH_RE.test(positional[1])) parts.push(positional[1]);
	}
	if (WIDTH_RE.test(attrs)) parts.push(WIDTH_RE.exec(attrs)[1]);
	const brace = attrs.indexOf("{");
	const img = brace === -1 ? null : parseAttrKvSpans(attrs.slice(brace))?.find((kv) => kv.key === "image");
	if (img) {
		const path = img.value.split("|")[0].trim();
		if (path) parts.push(path.split("/").pop());
	}
	return parts.join("  ·  ");
}
/** Resolve the icon + label a resting block's tag shows. A named block
*  (`:::grid`) shows its own icon and name; a nameless `:::{.class}` block
*  (see `classListLabel`) shows the class-region icon and its class list. */
function tagIconAndLabel(name, attrs) {
	if (name === "") {
		const classLabel = classListLabel(attrs);
		if (classLabel) return {
			icon: CLASS_REGION_ICON,
			label: classLabel
		};
	}
	return {
		icon: ICONS[name] ?? "‹/›",
		label: name
	};
}
var MicroTagWidget = class extends WidgetType {
	constructor(icon, label, params, bodyPos, asset, strings) {
		super();
		this.icon = icon;
		this.label = label;
		this.params = params;
		this.bodyPos = bodyPos;
		this.asset = asset;
		this.strings = strings;
	}
	eq(o) {
		return o.icon === this.icon && o.label === this.label && o.params === this.params && o.bodyPos === this.bodyPos && sameAsset(o.asset, this.asset);
	}
	toDOM() {
		const el = document.createElement("span");
		el.className = "cm-sc-tag";
		el.appendChild(this.renderLeading());
		const nm = document.createElement("span");
		nm.className = "cm-sc-tag-nm";
		nm.textContent = this.label;
		el.appendChild(nm);
		if (this.params) {
			const at = document.createElement("span");
			at.className = "cm-sc-tag-at";
			at.textContent = this.params;
			el.appendChild(at);
		}
		el.addEventListener("mousedown", (e) => {
			e.preventDefault();
			e.stopPropagation();
			const view = EditorView.findFromDOM(el);
			if (!view) return;
			view.dispatch({
				selection: { anchor: this.bodyPos },
				scrollIntoView: true
			});
			view.focus();
		});
		return el;
	}
	/**
	* The thumbnail, the missing-file marker, or the glyph — in that order of
	* preference. All three occupy the same slot and the same box, so the tag
	* stays exactly one line tall whichever one wins: a hero that resolves must
	* not reflow the document relative to one that hasn't resolved yet.
	*/
	renderLeading() {
		if (this.asset && this.asset !== "missing") {
			const img = document.createElement("img");
			img.className = "cm-sc-tag-thumb";
			img.alt = "";
			img.src = this.asset.url;
			return img;
		}
		const ic = document.createElement("span");
		ic.className = "cm-sc-tag-ic";
		if (this.asset === "missing") {
			ic.classList.add("cm-sc-tag-ic--missing");
			ic.textContent = MISSING_ICON;
			ic.setAttribute("data-tooltip", this.strings.assetMissingHint());
			return ic;
		}
		ic.textContent = this.icon;
		return ic;
	}
	ignoreEvent(e) {
		return e.type !== "mousedown";
	}
};
/** Widget identity for the asset slot — drives `eq`, hence DOM reuse. */
function sameAsset(a, b) {
	if (a === b) return true;
	if (!a || !b || a === "missing" || b === "missing") return false;
	return a.url === b.url;
}
/** Stands in for a thumbnail when the named file isn't there. */
const MISSING_ICON = "⊘";
/**
* The note next to a `---` cell divider: it still works, but `+++` is the
* spelling to use. Sits at the END of the divider line so the author reads
* what they typed first and the correction second, and so it never competes
* with a decoration over the `---` itself — live-preview may already have
* turned that text into a rule or a setext heading marker.
*
* Deliberately not a replacement: hiding the `---` would leave the author
* told to type `+++` with nothing visible to change.
*/
var LegacyDividerHintWidget = class extends WidgetType {
	constructor(linePos, strings) {
		super();
		this.linePos = linePos;
		this.strings = strings;
	}
	eq(o) {
		return o.linePos === this.linePos;
	}
	toDOM() {
		const el = document.createElement("span");
		el.className = "cm-sc-legacy-hint";
		el.textContent = this.strings.legacyDividerLabel();
		el.setAttribute("data-tooltip", this.strings.legacyDividerTooltip());
		el.addEventListener("mousedown", (e) => {
			e.preventDefault();
			e.stopPropagation();
			const view = EditorView.findFromDOM(el);
			if (!view) return;
			view.dispatch({
				selection: { anchor: this.linePos },
				scrollIntoView: true
			});
			view.focus();
		});
		return el;
	}
	ignoreEvent(e) {
		return e.type !== "mousedown";
	}
};
const REPLACE$1 = Decoration.replace({});
const MARK_SC_DELIM = Decoration.mark({ class: "cm-sc-delim" });
const MARK_SC_NAME = Decoration.mark({ class: "cm-sc-name" });
const MARK_SC_ATTR_KEY = Decoration.mark({ class: "cm-sc-attr-key" });
/**
* Token marks for a revealed OPEN fence line (`:::name {k=v}`): colons and
* braces muted, name in keyword weight, attr keys secondary, values plain.
* Reuses the fence grammar's own regex and the attr parser — no second parser.
* Marks are emitted left-to-right so the RangeSetBuilder stays sorted.
*/
function addOpenFenceTokenMarks(builder, ln) {
	const m = SHORTCODE_OPEN_RE.exec(ln.text);
	if (!isOpenMatch(m)) return;
	const colonsFrom = ln.from + m[1].length;
	const colonsTo = colonsFrom + m[2].length;
	builder.add(colonsFrom, colonsTo, MARK_SC_DELIM);
	const nameLen = m[3]?.length ?? 0;
	if (nameLen > 0) builder.add(colonsTo, colonsTo + nameLen, MARK_SC_NAME);
	const brace = ln.text.indexOf("{", m[1].length + m[2].length + nameLen);
	if (brace === -1) return;
	builder.add(ln.from + brace, ln.from + brace + 1, MARK_SC_DELIM);
	const kvs = parseAttrKvSpans(ln.text.slice(brace));
	if (kvs === null) return;
	for (const kv of kvs) builder.add(ln.from + brace + kv.keyFrom, ln.from + brace + kv.keyTo, MARK_SC_ATTR_KEY);
	const close = ln.text.lastIndexOf("}");
	if (close > brace) builder.add(ln.from + close, ln.from + close + 1, MARK_SC_DELIM);
}
/** Token marks for a revealed CLOSE fence line (`:::`): the colons, muted. */
function addCloseFenceTokenMarks(builder, ln) {
	const indent = ln.text.length - ln.text.trimStart().length;
	const colons = ln.text.trim().length;
	if (colons > 0) builder.add(ln.from + indent, ln.from + indent + colons, MARK_SC_DELIM);
}
/**
* What a block's tag should draw in its icon slot. Null resolver (jsdom,
* a read-only viewer, any editor built without `getFromFile`) → the glyph.
*/
function blockAsset(b, resolve) {
	if (!resolve) return null;
	const ref = shortcodeAssetRef(b.name, b.attrs);
	return ref ? resolve(ref.target) : null;
}
function buildBlockDecorations(state, strings, resolve) {
	const builder = new RangeSetBuilder();
	for (const top of collectShortcodeBlocks(state)) {
		const subtree = flattenBlocks([top]);
		const openByStart = /* @__PURE__ */ new Map();
		const closeByStart = /* @__PURE__ */ new Map();
		for (const b of subtree) {
			openByStart.set(b.from, b);
			closeByStart.set(b.closeFrom, b);
		}
		if (isBlockActive(state, top)) {
			let pos$1 = state.doc.lineAt(top.from).from;
			while (pos$1 <= top.to) {
				const ln = state.doc.lineAt(pos$1);
				builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-active" }));
				if (openByStart.has(ln.from)) addOpenFenceTokenMarks(builder, ln);
				else if (closeByStart.has(ln.from)) addCloseFenceTokenMarks(builder, ln);
				if (isLegacyDividerLine(ln.text) && legacyDividesCellsAt(ln.from, subtree)) builder.add(ln.to, ln.to, Decoration.widget({
					widget: new LegacyDividerHintWidget(ln.from, strings),
					side: 1
				}));
				if (ln.to + 1 > top.to) break;
				pos$1 = ln.to + 1;
			}
			continue;
		}
		let pos = state.doc.lineAt(top.from).from;
		while (pos <= top.to) {
			const ln = state.doc.lineAt(pos);
			const openB = openByStart.get(ln.from);
			const closeB = closeByStart.get(ln.from);
			if (openB) {
				const bodyPos = Math.min(ln.to + 1, openB.to);
				builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-openrest" }));
				const { icon, label } = tagIconAndLabel(openB.name, openB.attrs);
				builder.add(ln.from, ln.from, Decoration.widget({
					widget: new MicroTagWidget(icon, label, tagParams(openB.attrs), bodyPos, blockAsset(openB, resolve), strings),
					side: -1
				}));
				if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE$1);
			} else if (closeB) {
				builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-closerest" }));
				if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE$1);
			} else if (isCellDividerLine(ln.text) && dividesCellsAt(ln.from, subtree)) {
				builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-divider" }));
				if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE$1);
			} else if (isLegacyDividerLine(ln.text) && legacyDividesCellsAt(ln.from, subtree)) {
				builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-legacy-divider" }));
				builder.add(ln.to, ln.to, Decoration.widget({
					widget: new LegacyDividerHintWidget(ln.from, strings),
					side: 1
				}));
			} else builder.add(ln.from, ln.from, Decoration.line({ class: "cm-sc-line cm-sc-line-body" }));
			if (ln.to + 1 > top.to) break;
			pos = ln.to + 1;
		}
	}
	return builder.finish();
}
/**
* True when the incremental parse extended the tree without a doc change —
* the parse worker's effect-only transactions on large documents. Without
* this trigger, blocks beyond the initially-parsed region never decorate
* until the next edit or cursor move (scrolling produces no transaction at
* all for StateFields).
*/
function treeAdvanced(tr) {
	return syntaxTree(tr.state) != syntaxTree(tr.startState);
}
function shortcodeBlockField(opts) {
	const { resolveAsset, refsResolvedEffect } = opts;
	const strings = {
		...DEFAULT_STRINGS,
		...opts.strings
	};
	return StateField.define({
		create: (state) => buildBlockDecorations(state, strings, resolveAsset),
		update(value, tr) {
			const resolved = refsResolvedEffect !== void 0 && tr.effects.some((e) => e.is(refsResolvedEffect));
			if (!revealInputsChanged(tr) && !treeAdvanced(tr) && !resolved) return value;
			return buildBlockDecorations(tr.state, strings, resolveAsset);
		},
		provide: (f) => EditorView.decorations.from(f)
	});
}
function buildAtomicRanges(state) {
	const builder = new RangeSetBuilder();
	for (const top of collectShortcodeBlocks(state)) {
		if (isBlockActive(state, top)) continue;
		for (const b of flattenBlocks([top])) {
			const openLine = state.doc.lineAt(b.from);
			if (openLine.to > openLine.from) builder.add(openLine.from, openLine.to, REPLACE$1);
		}
	}
	return builder.finish();
}
const atomicField = StateField.define({
	create: buildAtomicRanges,
	update(value, tr) {
		if (!revealInputsChanged(tr) && !treeAdvanced(tr)) return value;
		return buildAtomicRanges(tr.state);
	}
});
function shortcodeBlockExtension(opts = {}) {
	return [
		shortcodeBlockField(opts),
		atomicField,
		EditorView.atomicRanges.of((view) => view.state.field(atomicField))
	];
}

//#endregion
//#region src/cm6/cm-link-resolver.ts
/**
* Map the cached LINK envelopes for every extracted link span to Diagnostic[].
*
* Reads the shared reference cache with is_embed:false. Targets not yet in the
* cache (the batch hasn't landed) produce no diagnostic — the linter re-runs
* via `needsRefresh(refsResolvedEffect)` once the batch dispatches its effect.
*
* Pure over (state, cache) — no async, no IPC. One diagnostic per extracted
* span (so the same target appearing twice gets two diagnostics).
*/
function runLinkLintSource(view, cache) {
	const targets = extractLinkTargets(view.state);
	if (targets.length === 0) return [];
	const diagnostics = [];
	for (const { target, from, to, nodeFrom, nodeTo } of targets) {
		if (nodeTouchesSelection(view.state, nodeFrom, nodeTo)) continue;
		const env = cache.get(target, false);
		if (!env) continue;
		if (env.kind.kind === "link" && env.message) diagnostics.push({
			from,
			to,
			severity: "warning",
			message: env.message,
			source: "link-validator"
		});
		else if (env.kind.kind === "moved") diagnostics.push({
			from,
			to,
			severity: "error",
			message: env.message ?? `no page at this URL any more — it lives at ${env.url ?? "?"}`,
			source: "link-validator"
		});
	}
	return diagnostics;
}
const clickableLinkDecoration = Decoration.mark({ class: "cm-link-clickable" });
const unresolvedDimDecoration = Decoration.mark({ class: "cm-link-unresolved-dim" });
/**
* Build the link-feedback decorations:
*   - `cm-link-clickable` on EVERY link NODE and every EMBED node (Cmd-held
*     cursor affordance).
*   - `cm-link-unresolved-dim` on not-found targets, EXCEPT the one the cursor
*     is on (per-node) — while the user is typing toward a future page, no
*     feedback at all for that link; sibling links keep their dim cue.
*
* Embeds get the hand but never the dim, which is why they are a separate walk
* rather than an addition to `extractLinkTargets`: that extractor skips
* `WikilinkEmbed` on purpose (ADR-041) because a lint underline belongs on link
* text, not on a rendered card. The CURSOR is a different question — an embed
* is exactly as followable as a link, and cm-link-nav has followed one since
* `followTargetAt` learned about embeds. While the card is rendered its source
* is hidden and nothing is painted; the moment the cursor reveals the raw
* `![[ … ]]` above the card, the press follows it and the hand must say so.
*/
function buildLinkDecorations(state, cache) {
	const builder = new RangeSetBuilder();
	const ranges = [];
	syntaxTree(state).iterate({ enter(node) {
		if (!isEmbedNode(node.name)) return;
		ranges.push({
			from: node.from,
			to: node.to,
			deco: clickableLinkDecoration
		});
		return false;
	} });
	for (const { target, from, to, nodeFrom, nodeTo } of extractLinkTargets(state)) {
		ranges.push({
			from: nodeFrom,
			to: nodeTo,
			deco: clickableLinkDecoration
		});
		if (cache.get(target, false)?.kind.kind === "not-found" && !nodeTouchesSelection(state, nodeFrom, nodeTo)) ranges.push({
			from,
			to,
			deco: unresolvedDimDecoration
		});
	}
	ranges.sort((a, b) => a.from - b.from || b.to - a.to);
	for (const r of ranges) builder.add(r.from, r.to, r.deco);
	return builder.finish();
}
/**
* Create a CM6 extension that validates links by READING the shared reference
* cache (no RPC of its own).
*
* - Uses Lezer-based `extractLinkTargets`.
* - The lint source is synchronous (a pure pull over the cache); the `linter()`
*   delay (600 ms) keeps the diagnostic paint debounced.
* - `needsRefresh(refsResolvedEffect)` re-runs the source when the reference
*   resolver's batch lands (initial resolve AND the post-FileChanged re-batch,
*   which the resolver triggers by clearing its cache + re-scheduling).
* - `@codemirror/lint` shows the diagnostic message on hover natively.
*
* @param opts.resolvedCache       Shared reference cache (read-only lookup).
* @param opts.refsResolvedEffect  The effect the resolver dispatches after a batch.
*/
function linkValidationExtension(opts) {
	const lintSource = (view) => runLinkLintSource(view, opts.resolvedCache);
	return { extension: [linter(lintSource, {
		delay: 600,
		needsRefresh: (update) => revealInputsChangedIn(update) || update.transactions.some((tr) => tr.effects.some((e) => e.is(opts.refsResolvedEffect)))
	}), ViewPlugin.fromClass(class LinkDecorator {
		constructor(view) {
			this.decorations = buildLinkDecorations(view.state, opts.resolvedCache);
		}
		update(update) {
			const batchLanded = update.transactions.some((tr) => tr.effects.some((e) => e.is(opts.refsResolvedEffect)));
			if (revealInputsChangedIn(update) || update.viewportChanged || batchLanded) this.decorations = buildLinkDecorations(update.state, opts.resolvedCache);
		}
	}, { decorations: (v) => v.decorations })] };
}

//#endregion
//#region src/cm6/cm-footnote.ts
const REPLACE = Decoration.replace({});
/** The label, raised — `[^` and `]` are hidden, so this IS the marker. */
const MARK_REF = Decoration.mark({ class: "cm-lp-footnote-ref" });
/** The same treatment on a definition line, so marker and note read as a pair. */
const MARK_DEF_LABEL = Decoration.mark({ class: "cm-lp-footnote-label" });
/** "There is somewhere to go from here" — the editor's ONE followable-cursor
*  class (`.cm-editor.cm-meta-held .cm-link-clickable`, editor.css), the same
*  one links and embed cards wear.
*
*  It is emitted from OUTSIDE the reveal gate below, and that is the whole
*  point: a footnote is just as followable with its `[^1]` showing as with it
*  rendered, so an affordance derived from the rendering promised nothing in
*  raw source while Cmd+click followed anyway. Followability is a fact about
*  the document, not about what the cursor happens to be near. */
const CLICKABLE = Decoration.mark({ class: "cm-link-clickable" });
/** Always-on line tint marking a line as note rather than body prose. Applied
*  to every line the definition spans — a note is a container, not a line. */
const LINE_DEF = Decoration.line({ class: "cm-lp-footnote-def" });
/** Direct children of `node` named `name`, in order. */
function childrenNamed(node, name) {
	const out = [];
	const cur = node.cursor();
	if (cur.firstChild()) do
		if (cur.name === name) out.push({
			from: cur.from,
			to: cur.to
		});
	while (cur.nextSibling());
	return out;
}
/**
* Every footnote in the document, indexed by label: where it is defined, and
* where it is first referenced.
*
* One walk answers all three questions the feature asks — may this marker
* render (is it defined), does this definition earn a back-jump (is it
* referenced), and where does either jump land. Splitting them across three
* walks is how a marker and its jump end up disagreeing about which
* definition is "the" one when a label is defined twice.
*
* Duplicate labels: FIRST wins, on both sides. The build renders the first
* definition's body and back-links to the first reference (ADR-035), so the
* editor's jump lands where the reader's would.
*/
function footnoteIndex(state) {
	const definitions = /* @__PURE__ */ new Map();
	const firstRefs = /* @__PURE__ */ new Map();
	syntaxTree(state).iterate({ enter(node) {
		const into = node.name === "FootnoteDefinition" ? definitions : node.name === "FootnoteRef" ? firstRefs : null;
		if (!into) return;
		const label = childrenNamed(node.node, "FootnoteLabel")[0];
		const marks = childrenNamed(node.node, "FootnoteMark");
		if (!label || marks.length === 0) return;
		const text = state.doc.sliceString(label.from, label.to);
		if (!into.has(text)) into.set(text, {
			from: node.from,
			to: node.to,
			labelFrom: label.from,
			labelTo: label.to,
			markEnd: marks[marks.length - 1].to
		});
		return node.name === "FootnoteDefinition" ? void 0 : false;
	} });
	return {
		definitions,
		firstRefs
	};
}
/**
* Where following the footnote at `pos` should land, or null.
*
* A footnote is a reference like any other, so it answers the same two
* gestures every other reference does (Cmd/Ctrl+click and Mod-Enter, wired in
* cm-link-nav.ts) — but it navigates WITHIN the document rather than opening a
* file, so it cannot go through `navigateToReference`.
*
* Both directions, matching what the built page gives a reader:
*   - on a marker  → its definition;
*   - on a definition's MARKER → its first reference (the build's `↩`
*     back-link).
*
* The definition side is deliberately scoped to the `[^1]:` chrome rather than
* the whole note. The note body is prose the author edits, and Mod-Enter with
* the caret in the middle of a sentence should insert a line, not teleport.
*
* Ends excluded, the rule `followTargetAt` uses and for the same reason: at
* `node.from` the caret is beside the construct, not on it, and Mod-Enter
* should still be Mod-Enter there.
*/
function footnoteJumpTarget(state, pos) {
	let node = syntaxTree(state).resolveInner(pos, 1);
	while (node && node.name !== "FootnoteRef" && node.name !== "FootnoteDefinition") node = node.parent;
	if (!node) return null;
	const label = childrenNamed(node.node, "FootnoteLabel")[0];
	if (!label) return null;
	const text = state.doc.sliceString(label.from, label.to);
	const { definitions, firstRefs } = footnoteIndex(state);
	if (node.name === "FootnoteRef") return pos > node.from && pos < node.to ? definitions.get(text) ?? null : null;
	const marks = childrenNamed(node.node, "FootnoteMark");
	const markEnd = marks.length ? marks[marks.length - 1].to : node.from;
	return pos > node.from && pos < markEnd ? firstRefs.get(text) ?? null : null;
}
/**
* Decorations for every footnote marker and definition in `state`.
*
* Returns an empty array for the overwhelmingly common case of a document
* with no footnotes, so the cost of the feature on an ordinary page is one
* tree walk that matches nothing.
*/
function footnoteDecorations(state, activeLines) {
	const { definitions, firstRefs } = footnoteIndex(state);
	const decos = [];
	syntaxTree(state).iterate({ enter(node) {
		if (node.name === "FootnoteRef") {
			const label = childrenNamed(node.node, "FootnoteLabel")[0];
			if (!label) return false;
			if (!definitions.has(state.doc.sliceString(label.from, label.to))) return false;
			decos.push({
				from: node.from,
				to: node.to,
				deco: CLICKABLE
			});
			if (nodeTouchesSelection(state, node.from, node.to)) return false;
			for (const mark of childrenNamed(node.node, "FootnoteMark")) decos.push({
				from: mark.from,
				to: mark.to,
				deco: REPLACE
			});
			decos.push({
				from: label.from,
				to: label.to,
				deco: MARK_REF
			});
			return false;
		}
		if (node.name === "FootnoteDefinition") {
			const first = state.doc.lineAt(node.from).number;
			const last = state.doc.lineAt(node.to).number;
			for (let n = first; n <= last; n++) {
				const at = state.doc.line(n).from;
				decos.push({
					from: at,
					to: at,
					deco: LINE_DEF
				});
			}
			const label = childrenNamed(node.node, "FootnoteLabel")[0];
			if (!label) return;
			const marks = childrenNamed(node.node, "FootnoteMark");
			const chromeEnd = marks.length ? marks[marks.length - 1].to : node.from;
			if (firstRefs.has(state.doc.sliceString(label.from, label.to)) && chromeEnd > node.from) decos.push({
				from: node.from,
				to: chromeEnd,
				deco: CLICKABLE
			});
			if (isNodeActive(state, node.from, chromeEnd, activeLines)) return;
			for (const mark of childrenNamed(node.node, "FootnoteMark")) decos.push({
				from: mark.from,
				to: mark.to,
				deco: REPLACE
			});
			decos.push({
				from: label.from,
				to: label.to,
				deco: MARK_DEF_LABEL
			});
			return;
		}
	} });
	return decos;
}
/** Base styles, so the feature needs no edit to a stylesheet to work. */
const footnoteTheme = EditorView.baseTheme({
	".cm-lp-footnote-ref, .cm-lp-footnote-label": {
		verticalAlign: "super",
		fontSize: "0.72em",
		color: "var(--moss-color-accent)"
	},
	".cm-lp-footnote-def": { color: "var(--moss-color-text-secondary)" }
});
/**
* The extension: a ViewPlugin that decorates the visible document, plus the
* base theme. Registered separately from cm-live-preview's plugin rather than
* folded into it — the two produce disjoint ranges, and keeping this pass
* standalone means the footnote feature is one file to read and one line to
* remove.
*/
function footnoteExtension() {
	return [ViewPlugin.fromClass(class {
		constructor(view) {
			this.decorations = this.build(view);
		}
		update(update) {
			if (revealInputsChangedIn(update) || update.viewportChanged) this.decorations = this.build(update.view);
		}
		build(view) {
			const decos = footnoteDecorations(view.state, getActiveLines(view.state));
			return Decoration.set(decos.map((d) => d.deco.range(d.from, d.to)), true);
		}
	}, { decorations: (v) => v.decorations }), footnoteTheme];
}

//#endregion
export { buildLinkDecorations, classListLabel, collectShortcodeBlocks, criticmarkupExtension, dividesCellsAt, editorFocusField, embedNodeAt, embedParts, extractImageTargets, extractLinkTargets, flattenBlocks, folderChips, folderParamsFromEmbed, footnoteDecorations, footnoteExtension, footnoteIndex, footnoteJumpTarget, footnoteTheme, getActiveLines, imageNodeAtWidget, inShortcodeBody, isBlockActive, isCellDividerLine, isEmbedNode, isLegacyDividerLine, isNodeActive, isRevealSuspended, isSourceMode, legacyDividesCellsAt, linkUnitOfEmbed, linkValidationExtension, linkedEmbedOf, mossHighlight, mossHighlightExtension, nodeTouchesSelection, parseFolderParams, parseMarks, revealInputsChanged, revealInputsChangedIn, revealSuspensionChanged, runLinkLintSource, setEditorFocusedEffect, setSourceModeEffect, shortcodeBlockExtension, shortcodeBodyRanges, sourceModeChanged, sourceModeField, spanOnActiveLine, tagParams, topLevelLegacyDividerCount, widthFromPipe };