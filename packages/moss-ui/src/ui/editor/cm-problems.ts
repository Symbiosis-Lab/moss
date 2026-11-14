// cm-problems.ts — every content problem in a markdown document, as one record
// per occurrence: marked once on its exact span, explained once in the
// reference card, fixed from the card opened with ⌘K.
//
// Two sources feed the records. The live reference resolver's envelopes
// (cm-reference-resolver.ts) cover every link and image as the author types.
// The last build's publish evidence covers what only the build sees, such as
// the exact spans inside a Hero or Gallery block. Where the live resolver has
// an answer for a span it wins, because it is the fresher of the two.
//
// The consequence decides the look: `blocks` (red) stops publishing, `warns`
// (amber) publishes with something broken or wrong, `draft` (dimmed) is
// intentional, such as a link to a page not written yet.

import { Facet, RangeSetBuilder, StateEffect, StateField, type EditorState, type Extension, type Transaction } from '@codemirror/state';
import { Decoration, EditorView } from '@codemirror/view';
import { syntaxTree, syntaxTreeAvailable } from '@codemirror/language';
import { extractImageTargets, extractLinkTargets, nodeTouchesSelection, type ReferenceCacheRead } from '@symbiosis-lab/moss-syntax/cm6';
import { t } from "../i18n/index";
import { refsResolvedEffect } from './cm-reference-resolver';
import { referenceSiteAt } from './cm-link-nav';
import { baseName, toAbsolute, toRelative } from './path-domain';
import type { CardAction, CardAugmenter } from './cm-reference-card';
import type { PublishPreflightHint } from './cm-reveal-flash';

export type Consequence = 'blocks' | 'warns' | 'draft';

export interface Problem {
  /** The span the mark covers: a link's address, an image's path. */
  from: number;
  to: number;
  /** The whole construct. While the selection touches it a live problem is
   *  not marked (the author is typing it); null for build evidence, which
   *  `Open at line` selects and must therefore stay visible. */
  construct: { from: number; to: number } | null;
  consequence: Consequence;
  /** The reference as written, the card's title when no other card claims it. */
  title: string;
  sentence: string;
  /** Offered first in the ⌘K card. */
  fixes: CardAction[];
}

export interface ProblemInputs {
  resolvedCache: ReferenceCacheRead;
  /** The reference form for the file at an absolute path, written from the
   *  open page — the file picker's `assetRefForPath`. A host without one gets
   *  no "Use <file>" fixes. */
  refForPath?: (absolutePath: string) => Promise<string | null>;
  /** Absolute project root, to place an ambiguous candidate's root-relative path. */
  projectRoot: () => string | null;
}

const problemInputs = Facet.define<ProblemInputs, ProblemInputs | null>({
  combine: (values) => values[0] ?? null,
});

/** Replace the build's evidence for the open document (empty clears it). */
export const setEvidenceProblems = StateEffect.define<readonly PublishPreflightHint[]>();

const MAX_CANDIDATE_FIXES = 3;

interface Envelope {
  kind: { kind: string };
  resolved: { provenance: string; absolute_path: string } | null;
  url: string | null;
  message: string | null;
  candidates: string[];
}

/** Replace the destination of the link at `at`, if it still reads `target`. */
function replaceLinkDestination(view: EditorView, at: number, target: string, insert: string): void {
  const site = referenceSiteAt(view.state, at);
  if (!site || site.target !== target) return;
  view.dispatch({ changes: { from: site.targetFrom, to: site.targetTo, insert } });
}

/** Replace the path, only if it still reads what it read when the card was
 *  built: the reference form is fetched first, and the author may have typed
 *  meanwhile. */
function replaceImagePath(view: EditorView, path: { from: number; to: number }, target: string, insert: string): void {
  if (path.to > view.state.doc.length || view.state.doc.sliceString(path.from, path.to) !== target) return;
  view.dispatch({ changes: { ...path, insert } });
}

/** What moss looked for: a bare name anywhere in the folder, a path where it points. */
function missingSentence(reference: string): string {
  const target = reference.split('|')[0].trim();
  return target.includes('/') ? t('problem.image_missing_at', { path: target }) : t('problem.image_missing', { name: target });
}

function imageProblem(env: Envelope, target: string, path: { from: number; to: number }, construct: { from: number; to: number }, inputs: ProblemInputs): Problem | null {
  // The mark sits on the path alone, as the build's does: an embed's brackets
  // and width open no card.
  const base = { ...path, construct, title: target };
  const root = inputs.projectRoot();
  const refForPath = inputs.refForPath;
  // The label names the file by its place in the folder; what is written is
  // the reference form moss computes for that file from this page, which can
  // be shorter. Fetching it to label the button would make the card wait.
  const useFile = (absolutePath: string): CardAction[] => (refForPath ? [{
    label: t('problem.fix_use', { ref: root ? toRelative(absolutePath, root) : baseName(absolutePath) }),
    run: async (view) => {
      const ref = await refForPath(absolutePath);
      if (ref) replaceImagePath(view, path, target, ref);
    },
  }] : []);
  const useCandidates = (): CardAction[] =>
    (root ? env.candidates.slice(0, MAX_CANDIDATE_FIXES).flatMap((c) => useFile(toAbsolute(c, root))) : []);
  if (env.kind.kind === 'not-found') {
    // The resolver names the same picture under another extension; the asset
    // card that claims this span adds choosing a file and removing the line.
    return { ...base, consequence: 'blocks', sentence: missingSentence(target), fixes: useCandidates() };
  }
  if (env.kind.kind === 'ambiguous') {
    return {
      ...base,
      consequence: 'warns',
      // The resolver sorts the file the build uses first.
      sentence: t('problem.image_ambiguous', { name: baseName(target), winner: env.candidates[0] ?? '' }),
      fixes: useCandidates(),
    };
  }
  const provenance = env.resolved?.provenance;
  if (env.resolved && provenance === 'case-mismatch') {
    const name = baseName(env.resolved.absolute_path);
    return { ...base, consequence: 'warns', sentence: t('problem.image_case', { name }), fixes: useFile(env.resolved.absolute_path) };
  }
  if (env.resolved && provenance === 'separator-fallback') {
    return { ...base, consequence: 'warns', sentence: t('problem.image_separator'), fixes: useFile(env.resolved.absolute_path) };
  }
  return null;
}

function linkProblem(env: Envelope, target: string, from: number, to: number, construct: { from: number; to: number }): Problem | null {
  const base = { from, to, construct, title: target };
  const update = (key: 'problem.fix_update' | 'problem.fix_use', url: string): CardAction => ({
    label: t(key, { ref: url }),
    run: (view) => replaceLinkDestination(view, construct.from, target, url),
  });
  if (env.kind.kind === 'moved' && env.url) {
    return { ...base, consequence: 'warns', sentence: t('problem.link_moved', { canonical: env.url }), fixes: [update('problem.fix_update', env.url)] };
  }
  // A resolved link carrying a message is a case or slug mismatch: the page
  // exists at `url`, and the address as written may not reach it once published.
  if (env.kind.kind === 'link' && env.message && env.url) {
    return { ...base, consequence: 'warns', sentence: t('problem.link_mismatch', { canonical: env.url }), fixes: [update('problem.fix_use', env.url)] };
  }
  if (env.kind.kind === 'not-found') {
    return { ...base, consequence: 'draft', sentence: t('problem.link_missing', { target }), fixes: [] };
  }
  return null;
}

/** Live problems, and the spans the live resolver has an answer for. Pure
 *  over (state, inputs) — exported for tests. */
export function referenceProblems(state: EditorState, inputs: ProblemInputs): { problems: Problem[]; answered: Array<{ from: number; to: number }> } {
  const problems: Problem[] = [];
  const answered: Array<{ from: number; to: number }> = [];
  for (const { target, from, to, nodeFrom, nodeTo } of extractImageTargets(state)) {
    const env = target ? inputs.resolvedCache.get(target, true) : undefined;
    if (!env) continue;
    answered.push({ from, to });
    const problem = imageProblem(env, target, { from, to }, { from: nodeFrom, to: nodeTo }, inputs);
    if (problem) problems.push(problem);
  }
  for (const { target, from, to, nodeFrom, nodeTo } of extractLinkTargets(state)) {
    const env = target ? inputs.resolvedCache.get(target, false) : undefined;
    if (!env) continue;
    answered.push({ from, to });
    const problem = linkProblem(env, target, from, to, { from: nodeFrom, to: nodeTo });
    if (problem) problems.push(problem);
  }
  return { problems, answered };
}

function evidenceProblem(hint: PublishPreflightHint): Problem {
  return {
    from: hint.from, to: hint.to, construct: null, consequence: 'blocks',
    title: hint.reference, sentence: missingSentence(hint.reference), fixes: [],
  };
}

/** Evidence follows edits elsewhere and is dropped the moment an edit touches
 *  its own text: the author is fixing it, and the build's verdict was on the
 *  old text. */
function mapEvidence(evidence: readonly Problem[], tr: Transaction): Problem[] {
  const out: Problem[] = [];
  for (const p of evidence) {
    if (tr.changes.touchesRange(p.from, p.to)) continue;
    out.push({ ...p, from: tr.changes.mapPos(p.from, 1), to: tr.changes.mapPos(p.to, -1) });
  }
  return out;
}

const intersects = (a: { from: number; to: number }, b: { from: number; to: number }) => a.from <= b.to && b.from <= a.to;

interface ProblemSet {
  live: Problem[];
  answered: Array<{ from: number; to: number }>;
  evidence: Problem[];
  /** live plus the evidence the live resolver has no answer for, by position. */
  all: Problem[];
}

function compose(live: Problem[], answered: ProblemSet['answered'], evidence: Problem[]): ProblemSet {
  const kept = evidence.filter((e) => !answered.some((a) => intersects(a, e)));
  const all = [...live, ...kept].sort((a, b) => a.from - b.from || a.to - b.to);
  return { live, answered, evidence, all };
}

function liveOf(state: EditorState): { problems: Problem[]; answered: ProblemSet['answered'] } {
  const inputs = state.facet(problemInputs);
  return inputs ? referenceProblems(state, inputs) : { problems: [], answered: [] };
}

export const problemsField = StateField.define<ProblemSet>({
  create(state) {
    const { problems, answered } = liveOf(state);
    return compose(problems, answered, []);
  },
  update(value, tr) {
    let evidence = tr.docChanged ? mapEvidence(value.evidence, tr) : value.evidence;
    let evidenceSet = false;
    for (const e of tr.effects) {
      if (e.is(setEvidenceProblems)) { evidence = e.value.map(evidenceProblem); evidenceSet = true; }
    }
    // A large document parses in steps after it opens, through effect-only
    // transactions. A reference the parse reaches that is not cached yet
    // arrives with the resolver's next batch; one already cached is picked up
    // once, when the parse reaches the end, rather than at every step.
    // (The parse's progress is shared between the two states, so the start
    // state cannot say it was unfinished; the tree itself changing can.)
    const parseFinished = !tr.docChanged && syntaxTree(tr.state) !== syntaxTree(tr.startState)
      && syntaxTreeAvailable(tr.state, tr.state.doc.length);
    const relive = tr.docChanged || tr.effects.some((e) => e.is(refsResolvedEffect)) || parseFinished;
    if (!relive && !evidenceSet && evidence === value.evidence) return value;
    if (!relive) return compose(value.live, value.answered, evidence);
    const { problems, answered } = liveOf(tr.state);
    return compose(problems, answered, evidence);
  },
});

const MARKS: Record<Consequence, Decoration> = {
  blocks: Decoration.mark({ class: 'cm-problem cm-problem--blocks' }),
  warns: Decoration.mark({ class: 'cm-problem cm-problem--warns' }),
  draft: Decoration.mark({ class: 'cm-problem cm-problem--draft' }),
};

// A selection change re-filters the composed list; it never walks the tree.
const problemMarks = EditorView.decorations.compute([problemsField, 'selection'], (state) => {
  const builder = new RangeSetBuilder<Decoration>();
  for (const p of state.field(problemsField).all) {
    if (p.construct && nodeTouchesSelection(state, p.construct.from, p.construct.to)) continue;
    builder.add(p.from, p.to, MARKS[p.consequence]);
  }
  return builder.finish();
});

/** The problem covering `pos`, if any. */
export function problemAt(state: EditorState, pos: number): Problem | null {
  return state.field(problemsField, false)?.all.find((p) => pos >= p.from && pos <= p.to) ?? null;
}

/** Puts the problem's sentence first in whatever card claims the position, and
 *  its fixes first among the card's actions; with no card, builds one. The
 *  body is wrapped, never written into: a provider may still be filling it. */
export const problemCardAugmenter: CardAugmenter = (spec, state, pos) => {
  const problem = problemAt(state, pos);
  if (!problem) return spec;
  const body = document.createElement('div');
  const sentence = body.appendChild(document.createElement('p'));
  sentence.className = 'cm-reference-card-problem';
  sentence.textContent = problem.sentence;
  if (!spec) return { title: problem.title, body, actions: [...problem.fixes] };
  body.appendChild(spec.body);
  return { ...spec, body, actions: [...problem.fixes, ...(spec.actions ?? [])] };
};

/** Puts the caret on the next problem that stops publishing, wrapping to the
 *  first; false when the page has none. */
export function goToNextProblem(view: EditorView): boolean {
  const blocking = (view.state.field(problemsField, false)?.all ?? []).filter((p) => p.consequence === 'blocks');
  if (blocking.length === 0) return false;
  const head = view.state.selection.main.head;
  const next = blocking.find((p) => p.from > head) ?? blocking[0];
  view.dispatch({ selection: { anchor: next.from }, scrollIntoView: true });
  view.focus();
  return true;
}

export function problemsExtension(inputs: ProblemInputs): Extension {
  return [problemInputs.of(inputs), problemsField, problemMarks];
}

/** The records as plain data, for the agent-facing debug hook. */
export function readProblems(state: EditorState): Array<Pick<Problem, 'from' | 'to' | 'consequence' | 'title' | 'sentence'> & { fixes: string[] }> {
  return (state.field(problemsField, false)?.all ?? []).map((p) => ({
    from: p.from, to: p.to, consequence: p.consequence, title: p.title, sentence: p.sentence,
    fixes: p.fixes.map((f) => f.label),
  }));
}
