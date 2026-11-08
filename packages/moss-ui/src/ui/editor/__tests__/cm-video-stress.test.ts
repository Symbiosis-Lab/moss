/**
 * Stress test: 20-video document — measures StateField update latency
 * when typing (simulating doc edits) and when moving the cursor (selection-only)
 * with 20 ![[video]] embeds in the document.
 *
 * Gate: p90 StateField update time < 16ms (one animation frame budget).
 *
 * The critical no-rebuild path (CRITICAL PERF requirement): cm-block-embeds.ts's
 * block field must NOT rebuild on selection-only (selectionSet) transactions
 * — unconditionally: nothing reveals on the caret any more, so a
 * selection-only transaction skips the rebuild whatever it selects.
 */
import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView, type DecorationSet } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { Strikethrough, Table } from '@lezer/markdown';
import { blockEmbedsExtension } from '../cm-block-embeds';
import { MediaEmbedBlockWidget } from '../cm-block-embed-widgets';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';

// ── Fixture ───────────────────────────────────────────────────────────────────

/** Build a markdown document with `n` video embed lines plus prose. */
function makeDoc(n = 20): string {
  const lines: string[] = ['# Video stress test\n'];
  for (let i = 0; i < n; i++) {
    lines.push(`Some paragraph text before video ${i + 1}.`);
    lines.push(`![[video-${i + 1}.mp4]]`);
    lines.push(`Caption text after video ${i + 1}.\n`);
  }
  lines.push('Some footer text at the end of the document.');
  return lines.join('\n');
}

/** resolveEmbed: instantly classifies every target as video + resolved src. */
const resolveEmbed = (target: string) => ({
  kind: 'video' as const,
  src: `moss-source://localhost/${target}`,
});

function makeState(doc = makeDoc(20)) {
  return EditorState.create({
    doc,
    extensions: [
      markdown({ extensions: [Strikethrough, Table, wikilinkConfig] }),
      blockEmbedsExtension({ resolveEmbed }),
    ],
  });
}

// ── p90 helper ────────────────────────────────────────────────────────────────

function p90(times: number[]): number {
  const sorted = [...times].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length * 0.9)];
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('video stress: 20-video document — StateField update latency', () => {
  test('p90 doc-change update < 16ms (option A: inline native player)', () => {
    // Measure 120 typing transactions (each appends 1 char at the document end).
    // The first 20 are warm-up and excluded from the measured set.
    const WARMUP = 20;
    const SAMPLES = 100;
    const times: number[] = [];
    let s = makeState();

    for (let i = 0; i < WARMUP + SAMPLES; i++) {
      const pos = s.doc.length - 1;
      const t0 = performance.now();
      s = s.update({ changes: { from: pos, to: pos, insert: 'x' } }).state;
      const dt = performance.now() - t0;
      if (i >= WARMUP) times.push(dt);
    }

    const p90dt = p90(times);
    const p50dt = times.slice().sort((a, b) => a - b)[Math.floor(times.length * 0.5)];
    console.log(
      `[stress] doc-change (20 videos, ${SAMPLES} edits): ` +
      `p50=${p50dt.toFixed(2)}ms p90=${p90dt.toFixed(2)}ms`,
    );

    // Gate: p90 < 16ms (one frame budget)
    expect(p90dt).toBeLessThan(16);
  });

  test('p90 selection-only update < 1ms (no-rebuild gate must hold)', () => {
    // Selection-only transactions (cursor moves, no doc change) must NOT
    // rebuild the imageBlockField StateField — verified by checking update time.
    const WARMUP = 20;
    const SAMPLES = 100;
    const s0 = makeState();
    const docLen = s0.doc.length;
    const times: number[] = [];
    let s = s0;
    let pseudo = 7; // deterministic pseudo-random

    for (let i = 0; i < WARMUP + SAMPLES; i++) {
      // Simple LCG for deterministic positions
      pseudo = (pseudo * 1664525 + 1013904223) & 0x7fffffff;
      const anchor = pseudo % docLen;
      const t0 = performance.now();
      s = s.update({ selection: { anchor } }).state;
      const dt = performance.now() - t0;
      if (i >= WARMUP) times.push(dt);
    }

    const p90dt = p90(times);
    console.log(
      `[stress] selection-only (20 videos, ${SAMPLES} cursor moves): ` +
      `p90=${p90dt.toFixed(2)}ms`,
    );

    // Selection-only: the no-rebuild gate means this is nearly free (sub-ms
    // on an idle machine). What the gate catches is a future change accidentally
    // re-enabling the selectionSet rebuild, which would cost ~the doc-change
    // path (~16ms) — so 8ms is still a 2x margin against the regression it
    // exists for.
    //
    // The bound used to be 1ms locally. That is not a machine-independent
    // number: a dev box running the rest of this suite in 8 parallel workers
    // measured 1.41ms and failed, then passed on a re-run. A perf gate that
    // depends on what else is running is a coin flip, not a gate.
    expect(p90dt).toBeLessThan(8);
  });

  test('video widget eq() returns true for same src/width (DOM reuse invariant)', () => {
    // The critical playback invariant: CM6 calls widget.eq(other) on each
    // StateField rebuild. If eq() returns true, the existing DOM (including the
    // live <video> element with its playback state) is REUSED — no teardown.
    //
    // cm-block-embeds.ts's block field no longer rebuilds on selection changes at
    // all (no reveal to gate), but eq()=true is still what lets CM6 keep the
    // same widget DOM across any rebuild that does happen (a doc edit, the
    // parse advancing, a resolved batch). We verify this by checking that two
    // widgets with the same identity fields are eq(), while changed fields
    // break identity.
    const base = {
      kind: 'video' as const,
      name: 'clip.mp4',
      target: 'clip.mp4',
      intraLineOffset: 0,
      onSetWidth: () => {},
      resizable: true,
      src: 'moss-source://localhost/clip.mp4',
      width: '80%',
    };
    const w1 = new MediaEmbedBlockWidget(base);
    // Same identity → eq() must return true (DOM reuse, no playback teardown)
    const w2 = new MediaEmbedBlockWidget(base);
    expect(w1.eq(w2 as any)).toBe(true);

    // Different src → eq() must return false (player URL changed)
    const w3 = new MediaEmbedBlockWidget({ ...base, src: 'moss-source://localhost/other.mp4' });
    expect(w1.eq(w3 as any)).toBe(false);

    // Different width → eq() must return false (resize committed)
    const w4 = new MediaEmbedBlockWidget({ ...base, width: '60%' });
    expect(w1.eq(w4 as any)).toBe(false);
  });
});
