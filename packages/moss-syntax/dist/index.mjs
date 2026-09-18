import { a as shortcodeAssetRef, i as parseAttrKvSpans, n as isCloseFence, o as shortcodeBlockConfig, r as isOpenMatch, s as SHORTCODES, t as SHORTCODE_OPEN_RE } from "./shortcode-BvIGBw1f.mjs";

//#region src/wikilink-grammar.ts
const BANG = 33;
const OPEN_BRACKET = 91;
const CLOSE_BRACKET = 93;
const NEWLINE$1 = 10;
const wikilinkConfig = {
	defineNodes: [
		"Wikilink",
		"WikilinkMark",
		"WikilinkTarget",
		"WikilinkEmbed"
	],
	parseInline: [{
		name: "Wikilink",
		before: "Link",
		parse(cx, next, pos) {
			let open = pos;
			let embed = false;
			if (next === BANG) {
				if (cx.char(pos + 1) !== OPEN_BRACKET || cx.char(pos + 2) !== OPEN_BRACKET) return -1;
				embed = true;
				open = pos + 1;
			} else if (next === OPEN_BRACKET) {
				if (cx.char(pos + 1) !== OPEN_BRACKET) return -1;
			} else return -1;
			const end = cx.end;
			let i = open + 2;
			while (i < end) {
				const ch = cx.char(i);
				if (ch === NEWLINE$1) return -1;
				if (ch === CLOSE_BRACKET && cx.char(i + 1) === CLOSE_BRACKET) {
					const wikilinkEnd = i + 2;
					const contentFrom = open + 2;
					const contentTo = i;
					const pipeIdx = cx.slice(contentFrom, contentTo).indexOf("|");
					const targetTo = pipeIdx === -1 ? contentTo : contentFrom + pipeIdx;
					const children = [
						cx.elt("WikilinkMark", pos, contentFrom),
						cx.elt("WikilinkTarget", contentFrom, targetTo),
						cx.elt("WikilinkMark", i, wikilinkEnd)
					];
					cx.addElement(cx.elt(embed ? "WikilinkEmbed" : "Wikilink", pos, wikilinkEnd, children));
					return wikilinkEnd;
				}
				i++;
			}
			return -1;
		}
	}]
};

//#endregion
//#region src/math-grammar.ts
const DOLLAR = 36;
const BACKSLASH = 92;
const SPACE = 32;
const TAB = 9;
const NEWLINE = 10;
/** Whitespace in the sense of the measured delimiter rules. `cx.char` returns
*  -1 past the end of the inline section, which is correctly NOT whitespace
*  (an opener at end-of-section simply finds no closer). */
function isWhitespace(ch) {
	return ch === SPACE || ch === TAB || ch === NEWLINE;
}
/** `$$…$$`: no whitespace rules on either side (the TeX keeps surrounding
*  spaces/newlines verbatim — `$$ x^2 $$` → " x^2 "). The FIRST unescaped `$`
*  after the opener is the only closer candidate: it must start a `$$` pair,
*  or the opener FAILS (measured: `$$a$b$$` → inline "a" from the retry at
*  pos+1, NOT display "a$b"; `$$ $ $$` → no math). The scan may cross soft
*  line breaks; the inline section ends at a blank line, matching pulldown's
*  paragraph-break behavior. */
function parseDisplay(cx, pos) {
	const end = cx.end;
	let i = pos + 2;
	while (i < end) {
		const ch = cx.char(i);
		if (ch === BACKSLASH) {
			i += 2;
			continue;
		}
		if (ch === DOLLAR) {
			if (cx.char(i + 1) !== DOLLAR) return -1;
			return cx.addElement(cx.elt("DisplayMath", pos, i + 2, [cx.elt("MathMark", pos, pos + 2), cx.elt("MathMark", i, i + 2)]));
		}
		i++;
	}
	return -1;
}
/** `$…$`: opener not followed by whitespace; the FIRST unescaped `$` after
*  the opener is the only closer candidate — preceded by whitespace means the
*  whole opener FAILS, the scan does NOT continue to a later `$` (measured:
*  `$a $b$ c$` → inline "b", not "a $b"). May span soft line breaks — see
*  the module header. */
function parseInlineMath(cx, pos) {
	const after = cx.char(pos + 1);
	if (after < 0 || isWhitespace(after)) return -1;
	const end = cx.end;
	let i = pos + 1;
	while (i < end) {
		const ch = cx.char(i);
		if (ch === BACKSLASH) {
			i += 2;
			continue;
		}
		if (ch === DOLLAR) {
			if (isWhitespace(cx.char(i - 1))) return -1;
			return cx.addElement(cx.elt("InlineMath", pos, i + 1, [cx.elt("MathMark", pos, pos + 1), cx.elt("MathMark", i, i + 1)]));
		}
		i++;
	}
	return -1;
}
const mathConfig = {
	defineNodes: [
		"InlineMath",
		"DisplayMath",
		"MathMark"
	],
	parseInline: [{
		name: "Math",
		before: "Emphasis",
		parse(cx, next, pos) {
			if (next !== DOLLAR) return -1;
			return cx.char(pos + 1) === DOLLAR ? parseDisplay(cx, pos) : parseInlineMath(cx, pos);
		}
	}]
};

//#endregion
//#region src/wikilink-syntax.ts
const WIKILINK_RE = /^!?\[\[([\s\S]*)\]\]$/;
/**
* Remove the wikilink/embed delimiters (`[[ ]]`, optional leading `!`),
* preserving the inner text — including any `|alias` / `#anchor`. A bare path
* passes through unchanged. Use this for the round-tripping picker display
* where the alias must survive an edit.
*/
function stripWikilinkBrackets(raw) {
	const s = raw.trim();
	const m = WIKILINK_RE.exec(s);
	return m ? m[1].trim() : s;
}
/**
* Resolve a wikilink/embed value to the bare reference target: strip the
* `[[ ]]` delimiters AND drop the `|alias`/`|size` pothole and `#anchor`,
* mirroring `classify_reference`'s split. Use this before resolving an asset
* (e.g. the cover FilePicker chip) so the editor resolves the same source file
* the build does.
*/
function wikilinkTarget(raw) {
	return stripWikilinkBrackets(raw).split("|")[0].split("#")[0].trim();
}
/** Wrap a bare reference in `[[ ]]` (idempotent; trims first). */
function wrapWikilink(raw) {
	const s = raw.trim();
	return s.startsWith("[[") ? s : `[[${s}]]`;
}
/**
* Canonical moss embed insert: produces `![[name]]` from a bare filename or
* folder path. No URL-encoding — the build fuzzy-resolves bare names;
* separator/encoded paths 404 on deploy. Folder names ending with `/` are
* kept as-is (e.g. `![[webapp/]]`).
*/
function wrapEmbedWikilink(name) {
	return `![[${name}]]`;
}

//#endregion
//#region src/completion-core.ts
const OPEN_WIKILINK = /(!?)\[\[([^\]\n]*)$/;
/**
* The wikilink phase at `cursorInLine`, or null when the cursor is not inside
* an open `[[…` / `![[…` (plain text, or after a closed `[[x]]`). The span
* runs from just after the brackets (or the `#`) over the typed query and the
* rest of the target the cursor sits inside (`wikilinkTailLength`).
*/
function parseWikilinkPhase(lineText, cursorInLine) {
	const m = OPEN_WIKILINK.exec(lineText.slice(0, cursorInLine));
	if (!m) return null;
	const embed = m[1] === "!";
	const inner = m[2];
	const to = cursorInLine + wikilinkTailLength(lineText.slice(cursorInLine));
	const hash = inner.indexOf("#");
	if (hash === -1) return {
		phase: "wikilink",
		embed,
		query: inner,
		from: cursorInLine - inner.length,
		to
	};
	const query = inner.slice(hash + 1);
	return {
		phase: "heading",
		syntax: "wikilink",
		page: inner.slice(0, hash) || null,
		query,
		from: cursorInLine - query.length,
		to
	};
}
/**
* The phase for an inline `![alt](…)` or `[text](…)` target containing the
* cursor. An image target is the `image` phase (assets only). A link target
* is the `link` phase, or the `heading` phase once the author types `#` — a
* page part that opens with `/` names a place on the published site, whose
* headings need a source file this parser cannot name, so it offers nothing.
*/
function parseInlineTargetPhase(lineText, cursorInLine) {
	const t = parseInlineTarget(lineText, cursorInLine);
	if (!t) return null;
	if (t.image) return {
		phase: "image",
		query: t.query,
		from: t.from,
		to: t.to
	};
	const hash = t.query.indexOf("#");
	if (hash === -1) return {
		phase: "link",
		query: t.query,
		from: t.from,
		to: t.to
	};
	const page = t.query.slice(0, hash);
	if (page.startsWith("/")) return null;
	const query = t.query.slice(hash + 1);
	return {
		phase: "heading",
		syntax: "inline",
		page: page || null,
		query,
		from: t.from + hash + 1,
		to: t.to
	};
}
/**
* Parse the attr region of a fence line (cursor at or past the `{`).
*
* Scans left-to-right from just after `{` to the cursor, tracking whether
* we're collecting an attr NAME or an attr VALUE. A bare (unescaped) `=`
* switches name→value; unquoted whitespace ends a value and starts a new
* name; a quote character as the FIRST char of a value opens a quoted span
* that swallows internal whitespace verbatim (so `button="Sign up now"`
* doesn't truncate the query at the space). Whichever mode is active when
* the loop reaches the cursor determines the phase: 'attr-name' (still
* typing the attr's name — matches against the static attrs list) or
* 'attr-value' (typing the value of the most recently opened, still-open
* `name=`) — this is the fix for `{image=photo.jpg}` being misread as an
* attr-*name* query by the old single regex.
*
* Returns null when the cursor sits just past a CLOSED quoted value
* (`{image="a.png"|}`): the value is finished, so there is nothing to complete,
* and the old code produced the corrupt query `a.png"` there.
*/
function parseAttrPhase(lineText, blockName, braceAbsIdx, cursorInLine) {
	let mode = "name";
	let nameStart = braceAbsIdx + 1;
	let curName = "";
	let inQuote = false;
	let quoteChar = "";
	let valueStart = -1;
	let valueClosed = false;
	for (let i = braceAbsIdx + 1; i < cursorInLine; i++) {
		const ch = lineText[i];
		if (mode === "name") {
			if (ch === "=") {
				curName = lineText.slice(nameStart, i);
				mode = "value";
				valueStart = i + 1;
				inQuote = false;
				valueClosed = false;
			} else if (/[\s,]/.test(ch)) {
				nameStart = i + 1;
				curName = "";
			}
		} else if (inQuote) {
			if (ch === quoteChar) {
				inQuote = false;
				valueClosed = true;
			}
		} else if ((ch === "\"" || ch === "'") && i === valueStart) {
			inQuote = true;
			quoteChar = ch;
		} else if (/[\s,]/.test(ch)) {
			mode = "name";
			nameStart = i + 1;
			curName = "";
			valueStart = -1;
			valueClosed = false;
		}
	}
	if (mode === "value") {
		if (valueClosed) return null;
		let queryFrom = valueStart;
		if (queryFrom < lineText.length && (lineText[queryFrom] === "\"" || lineText[queryFrom] === "'")) queryFrom += 1;
		const query = lineText.slice(queryFrom, cursorInLine);
		return {
			phase: "attr-value",
			blockName,
			attrName: curName,
			query,
			from: queryFrom,
			valueFrom: valueStart
		};
	}
	return {
		phase: "attr-name",
		blockName,
		query: lineText.slice(nameStart, cursorInLine),
		from: nameStart
	};
}
/** Parse a fence line (`:::name {attrs}`). Returns null if not a fence line. */
function parseFenceLine(lineText, cursorInLine) {
	const trimmed = lineText.trimStart();
	const leadingWs = lineText.length - trimmed.length;
	if (!trimmed.startsWith(":::")) return null;
	const afterFence = trimmed.slice(3);
	const fenceEnd = leadingWs + 3;
	if (cursorInLine < fenceEnd) return null;
	const braceIdx = afterFence.indexOf("{");
	const braceAbsIdx = braceIdx === -1 ? -1 : fenceEnd + braceIdx;
	if (braceAbsIdx !== -1 && cursorInLine > braceAbsIdx) {
		const nameMatch = afterFence.match(/^([\w-]+)/);
		return parseAttrPhase(lineText, nameMatch ? nameMatch[1] : "", braceAbsIdx, cursorInLine);
	}
	return {
		phase: "name",
		query: afterFence.slice(0, cursorInLine - fenceEnd).replace(/[^a-zA-Z0-9_-]/g, ""),
		from: fenceEnd
	};
}
/** Index of the first unescaped `|` in `text`, or -1. */
function findUnescapedPipe(text) {
	for (let i = 0; i < text.length; i++) if (text[i] === "|" && text[i - 1] !== "\\") return i;
	return -1;
}
/**
* Chars in `lineText` starting at `parenOpen + 1` that belong to a markdown
* link or image TARGET: bounded by the closing `)` (or end of line while still
* typing) and, before that, by the first unescaped `|` (moss's display-attrs
* suffix).
*/
function inlineTargetBound(lineText, parenOpen) {
	let close = lineText.indexOf(")", parenOpen + 1);
	if (close === -1) close = lineText.length;
	const pipe = findUnescapedPipe(lineText.slice(parenOpen + 1, close));
	return pipe === -1 ? close : parenOpen + 1 + pipe;
}
/**
* Find the markdown target (`![alt](‹here›)` or `[text](‹here›)`) containing
* `cursorInLine`, anywhere in the line — inside a `:::grid` cell, inside a
* link (`[![alt](p)](/url)`), or in ordinary prose. First match containing the
* cursor wins; `image` says which form it was.
*
* Deliberately a hand-rolled scanner rather than a reuse of the Lezer-based
* `cm-image-extract.ts::extractImageTargets`: the markdown Lezer emits no
* `Image`/`Link` node until the closing `)` exists, and moss ships no
* `closeBrackets()` (`cm-editor.ts` uses `minimalSetup`), so during live
* typing of `![](關於/頭` there is no node to resolve. A completion source must
* answer over INCOMPLETE syntax, which a committed tree cannot do.
*
* Falsifier for that exception: if the lint's target spans and these ever
* diverge on a real document, unify on Lezer with an explicit incomplete-token
* fallback. Both are pure and separately tested, so the divergence is
* observable.
*/
function parseInlineTarget(lineText, cursorInLine) {
	const re = /(!?)\[(?!!\[)[^\]\n]*\]\(/g;
	let m;
	while ((m = re.exec(lineText)) !== null) {
		const parenOpen = m.index + m[0].length - 1;
		const bound = inlineTargetBound(lineText, parenOpen);
		if (cursorInLine > parenOpen && cursorInLine <= bound) {
			const from = parenOpen + 1;
			return {
				query: lineText.slice(from, cursorInLine),
				from,
				to: bound,
				image: m[1] === "!"
			};
		}
	}
	return null;
}
/**
* Parse a gallery body line for asset-path completion. Body lines are a bare
* `path`, or `![alt](path)`, either optionally suffixed with `|attrs`
* (`shortcode_extract.rs` `parse_gallery_body`). Gallery does not support
* `![[...]]` wikilink-embed syntax in its body, so that form is not special-
* cased here.
*
* A cursor past an unescaped `|` (editing the display-attrs suffix, not the
* path) yields no completion. The `![alt](path)` form is delegated to
* `parseInlineTarget` — one implementation for every image target in the
* document; only the bare-path arm is gallery-specific.
*/
function parseGalleryBodyLine(lineText, cursorInLine) {
	const img = parseInlineTarget(lineText, cursorInLine);
	if (img?.image) return {
		query: img.query,
		from: img.from,
		to: img.to
	};
	if (/^\s*!\[/.test(lineText)) return null;
	const pipeIdx = findUnescapedPipe(lineText);
	if (pipeIdx !== -1 && cursorInLine > pipeIdx) return null;
	const boundary = pipeIdx === -1 ? lineText.length : pipeIdx;
	const leadingWs = lineText.length - lineText.trimStart().length;
	if (cursorInLine < leadingWs || cursorInLine > boundary) return null;
	return {
		query: lineText.slice(leadingWs, cursorInLine),
		from: leadingWs,
		to: boundary
	};
}
/**
* The full value token of a shortcode attr starting at `valueFrom` (which
* INCLUDES an opening quote if there is one), split into the end of the token
* and the `|attrs` suffix the accept path must preserve.
*
* - Quoted, closing quote on this line: the span includes both quotes. A `}`
*   inside the quotes is part of the value and does not end it.
* - Quoted, NO closing quote on this line: the value is unterminated, so its
*   real extent is unknowable from this line. The span stops at the block's
*   closing `}` if there is one, and otherwise at `fallbackTo` (the caller
*   passes the cursor). **Never** to end of line — that swallowed the `}` and
*   everything after it, turning `:::hero {image="關}` into an unparseable
*   `:::hero {image="關於/頭像.png"`. Reachable straight from the shipped hero
*   snippet: its `photo.jpg` tab-stop is selected, so typing `"` then a CJK
*   prefix produces exactly that line.
* - Bareword: ends at the first `[\s,}]`, else at `fallbackTo`.
*
* `pipeSuffix` is the display-attrs tail, UNESCAPED — the caller re-renders
* `path + pipeSuffix` through `renderShortcodeAttrValue`, so handing back the
* raw source slice would escape it a second time (`\"Hi\"` → `\\\"Hi\\\"`).
*/
function attrValueSpan(lineText, valueFrom, fallbackTo) {
	const q = lineText[valueFrom];
	const quoted = q === "\"" || q === "'";
	const innerFrom = quoted ? valueFrom + 1 : valueFrom;
	let to;
	let terminated = false;
	if (quoted) {
		let j = innerFrom;
		while (j < lineText.length) {
			if (lineText[j] === "\\") {
				j += 2;
				continue;
			}
			if (lineText[j] === q) break;
			j += 1;
		}
		terminated = j < lineText.length;
		if (terminated) to = j + 1;
		else {
			const brace = lineText.indexOf("}", innerFrom);
			to = brace === -1 ? Math.max(fallbackTo, innerFrom) : brace;
		}
	} else {
		const stop = lineText.slice(valueFrom).search(/[\s,}]/);
		to = stop === -1 ? Math.max(fallbackTo, valueFrom) : valueFrom + stop;
	}
	const innerTo = quoted && terminated ? Math.max(innerFrom, to - 1) : to;
	const inner = lineText.slice(innerFrom, innerTo);
	const pipe = findUnescapedPipe(inner);
	if (pipe === -1) return {
		to,
		pipeSuffix: ""
	};
	const suffix = inner.slice(pipe);
	return {
		to,
		pipeSuffix: quoted ? suffix.replace(/\\(.)/g, "$1") : suffix
	};
}
/**
* Render `v` as a shortcode attr value: bareword when the grammar accepts it
* verbatim, else a double-quoted string with `\` and `"` escaped.
*
* THE one renderer — `cm-completion.ts`'s accept path and
* `cm-insert-bar.ts`'s hero template both call it. The bareword character set
* mirrors `is_bareword` in `crates/moss-core/src/shortcode/attrs.rs`; escaping
* mirrors `read_quoted` there. Drift is gated by
* `crates/moss-core/tests/fixtures/attr-value.vectors.json`, which both the
* Rust parser test and the TS renderer test read — not by a comment.
*/
function renderShortcodeAttrValue(v) {
	if (v.length > 0 && /^[A-Za-z0-9:/._-]+$/.test(v)) return v;
	return `"${v.replace(/\\/g, "\\\\").replace(/"/g, "\\\"")}"`;
}
/**
* Chars after the cursor that still belong to the wikilink TARGET: up to the
* first `]`, `|` or `#`. Lets a mid-token accept replace the whole target
* instead of splicing the completion in front of its tail.
*
* Returns 0 when no terminator is found before end of line. An unclosed `[[`
* has no target end to find, and treating end-of-line as one deleted arbitrary
* prose: `see [[Res and then the rest of the sentence` swallowed 33 characters
* on accept. Extending only over a target we can actually see the end of is the
* whole safety margin here.
*/
function wikilinkTailLength(after) {
	const stop = after.search(/[\]|#\n]/);
	return stop === -1 ? 0 : stop;
}
/**
* Scan backward from `lineNumber - 1` for the nearest unclosed `:::name`
* fence open. A bare close fence (`:::`) encountered first means the last
* block already closed before this line, so it returns null. Assumes
* non-nested blocks (moss shortcodes don't nest), matching
* `blockAlreadyClosed`'s scan direction/assumptions.
*/
function findEnclosingBlockName(doc, lineNumber) {
	for (let n = lineNumber - 1; n >= 1; n--) {
		const text = doc.line(n).text;
		if (/^\s*:{3,}\s*$/.test(text)) return null;
		const openMatch = /^\s*:{3,}\s*([\w-]+)/.exec(text);
		if (openMatch) return openMatch[1];
	}
	return null;
}

//#endregion
//#region src/scan.ts
/**
* Text-driven shortcode block scanner — the parse over a plain string that
* hosts without a `@lezer/markdown` seam need (Obsidian's markdown language
* is closed to grammar extensions; see
* packages/obsidian-moss/src/syntax/shortcode-parser.ts, the stub this
* replaces, and #1020).
*
* The line predicates are REUSED from ./shortcode.ts, not copied: an open
* fence is exactly `SHORTCODE_OPEN_RE` + `isOpenMatch`, a close fence is
* exactly `isCloseFence`, and open↔close pairing uses the same arity stack as
* the editor's tree walk (`collectShortcodeBlocks` in
* ./cm6/cm-shortcode-block.ts): a close fence of arity N closes
* the TOPMOST open of arity N (`::::` inside `:::` nests); opens above the
* matched one never got a close and are reported as unclosed children.
*
* Two deliberate differences from the Lezer path, both on degenerate input:
*
* - **Unclosed blocks are reported, not dropped.** `collectShortcodeBlocks`
*   discards an open that never closes (a decoration must not half-render);
*   a host linting while the author is still typing needs the block BEFORE
*   its close fence exists, so here it comes back with `closeFrom: null` and
*   `to` at the end of its last body line.
* - **No markdown block context.** A pure line scan cannot know a `:::`
*   sits inside a fenced code block or blockquote; the grammar (which runs
*   inside a real markdown parse) does. On documents without those wrappers
*   the two agree exactly — `scan.test.ts` cross-checks against the Lezer
*   parse over the shared structure corpus.
*
* CRLF is tolerated the same way the predicates tolerate it (they trim):
* a trailing `\r` counts as line terminator, so `from`/`to` offsets never
* include it.
*/
/** Scan `text` for shortcode blocks. Returns the TOP-LEVEL blocks; each
*  carries its nested `children` (document order, parent before children). */
function scanShortcodeBlocks(text) {
	const stack = [];
	const out = [];
	/** End offset of the most recently scanned line (excludes `\r`/`\n`). */
	let prevLineEnd = 0;
	/** An open that never got its close: report it, don't drop it. */
	const unclosed = (o, lastEnd) => ({
		from: o.from,
		to: lastEnd,
		name: o.name,
		attrs: o.attrs,
		arity: o.arity,
		closeFrom: null,
		children: o.children
	});
	let lineFrom = 0;
	const n = text.length;
	while (lineFrom <= n) {
		if (lineFrom === n && n > 0 && text[n - 1] === "\n") break;
		let nl = text.indexOf("\n", lineFrom);
		if (nl === -1) nl = n;
		const lineEnd = nl > lineFrom && text[nl - 1] === "\r" ? nl - 1 : nl;
		const line = text.slice(lineFrom, lineEnd);
		const m = SHORTCODE_OPEN_RE.exec(line);
		if (isOpenMatch(m)) stack.push({
			from: lineFrom,
			arity: m[2].length,
			name: m[3] ?? "",
			attrs: m[4].trim(),
			children: []
		});
		else if (isCloseFence(line)) {
			const arity = line.trim().length;
			for (let i = stack.length - 1; i >= 0; i--) if (stack[i].arity === arity) {
				const removed = stack.splice(i);
				const open = removed[0];
				for (let j = removed.length - 1; j >= 1; j--) removed[j - 1].children.push(unclosed(removed[j], prevLineEnd));
				const block = {
					from: open.from,
					to: lineEnd,
					name: open.name,
					attrs: open.attrs,
					arity: open.arity,
					closeFrom: lineFrom,
					children: open.children
				};
				if (stack.length > 0) stack[stack.length - 1].children.push(block);
				else out.push(block);
				break;
			}
		}
		prevLineEnd = lineEnd;
		if (nl >= n) break;
		lineFrom = nl + 1;
	}
	for (let j = stack.length - 1; j >= 1; j--) stack[j - 1].children.push(unclosed(stack[j], prevLineEnd));
	if (stack.length > 0) out.push(unclosed(stack[0], prevLineEnd));
	return out;
}
/** Flatten CM6 snippet tab-stops (`${1:photo.jpg}` → `photo.jpg`). */
const flattenTemplate = (t) => t.replace(/\$\{\d+:([^}]*)\}/g, "$1");
/**
* The authorable shortcode vocabulary, derived from the generated catalog
* (Rust SSOT: crates/moss-core/src/contract/shortcodes.rs). Non-authorable
* kinds (`apply`) parse and render but are never offered to authors, so they
* are excluded here — this list exists for unknown-name diagnostics and
* completion, both author-facing.
*/
const KNOWN_SHORTCODES = SHORTCODES.filter((s) => s.authorable).map((s) => ({
	name: s.name,
	doc: `:::${flattenTemplate(s.canonicalTemplate).split("\n")[0]}`
}));

//#endregion
//#region src/footnote-grammar.ts
const BRACKET_OPEN = 91;
const BRACKET_CLOSE = 93;
const CARET = 94;
const COLON = 58;
/**
* Columns a continuation line must be indented to stay inside the note.
*
* Fixed at four, not derived from the marker's width the way a list item
* derives it from `- `. That is pulldown's rule and it is the one an author
* can hold in their head: every footnote continues at the same indent,
* whatever its label is called.
*/
const CONTINUATION_INDENT = 4;
/**
* Scan a `[^label]` starting at `pos`, returning the offset just past the
* closing `]`, or -1. `charAt` returns -1 past the end.
*
* A label may hold anything but `[`, `]` and a newline, and may not be empty
* — `[^]` is not a footnote. Both bracket characters are excluded so a
* nested-bracket typo fails fast instead of swallowing the rest of the line.
*/
function scanLabel(charAt, pos, end) {
	if (charAt(pos) !== BRACKET_OPEN || charAt(pos + 1) !== CARET) return -1;
	let i = pos + 2;
	while (i < end) {
		const ch = charAt(i);
		if (ch === BRACKET_CLOSE) return i === pos + 2 ? -1 : i + 1;
		if (ch === BRACKET_OPEN || ch === 10 || ch < 0) return -1;
		i++;
	}
	return -1;
}
const footnoteConfig = {
	defineNodes: [
		"FootnoteRef",
		"FootnoteMark",
		"FootnoteLabel",
		{
			name: "FootnoteDefinition",
			block: true,
			composite(_cx, line, value) {
				if (line.next < 0) return true;
				if (line.indent < line.baseIndent + value) return false;
				line.moveBaseColumn(line.baseIndent + value);
				return true;
			}
		}
	],
	parseInline: [{
		name: "FootnoteRef",
		before: "Emphasis",
		parse(cx, next, pos) {
			if (next !== BRACKET_OPEN) return -1;
			const to = scanLabel((i) => cx.char(i), pos, cx.end);
			if (to < 0) return -1;
			return cx.addElement(cx.elt("FootnoteRef", pos, to, [
				cx.elt("FootnoteMark", pos, pos + 2),
				cx.elt("FootnoteLabel", pos + 2, to - 1),
				cx.elt("FootnoteMark", to - 1, to)
			]));
		}
	}],
	parseBlock: [{
		name: "FootnoteDefinition",
		before: "LinkReference",
		parse(cx, line) {
			if (line.indent - line.baseIndent >= 4) return false;
			const text = line.text;
			const off = line.pos;
			const labelEnd = scanLabel((i) => i < text.length ? text.charCodeAt(i) : -1, off, text.length);
			if (labelEnd < 0 || text.charCodeAt(labelEnd) !== COLON) return false;
			const from = cx.lineStart + off;
			const bodyStart = text.charCodeAt(labelEnd + 1) === 32 ? labelEnd + 2 : labelEnd + 1;
			cx.startComposite("FootnoteDefinition", line.basePos, CONTINUATION_INDENT);
			cx.addElement(cx.elt("FootnoteMark", from, from + 2));
			cx.addElement(cx.elt("FootnoteLabel", from + 2, cx.lineStart + labelEnd - 1));
			cx.addElement(cx.elt("FootnoteMark", cx.lineStart + labelEnd - 1, cx.lineStart + labelEnd + 1));
			line.moveBase(bodyStart);
			return null;
		}
	}]
};

//#endregion
export { KNOWN_SHORTCODES, SHORTCODES, SHORTCODE_OPEN_RE, attrValueSpan, findEnclosingBlockName, findUnescapedPipe, footnoteConfig, isCloseFence, isOpenMatch, mathConfig, parseAttrKvSpans, parseAttrPhase, parseFenceLine, parseGalleryBodyLine, parseInlineTarget, parseInlineTargetPhase, parseWikilinkPhase, renderShortcodeAttrValue, scanShortcodeBlocks, shortcodeAssetRef, shortcodeBlockConfig, stripWikilinkBrackets, wikilinkConfig, wikilinkTailLength, wikilinkTarget, wrapEmbedWikilink, wrapWikilink };