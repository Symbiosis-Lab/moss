// Full-window nested-site threshold screen.
//
// Rendered into #moss-preview-overlay — the same slot as the cloud waiting
// screen — when opening a folder needs a decision before anything is written:
// the folder contains site(s) of its own (cases (a)/(b)), or it was itself
// accidentally made a site around them (case (d): ONE question — which folder
// is the correct site — and the loser is un-vaulted by the guard in the same
// action).
//
// It is a full-window state, NOT a modal: at this moment there is no content
// behind it — no `.moss` created, no build run, the preview iframe empty — so
// a screen over nothing blocks nothing, exactly the argument that let the
// waiting screen ship full-window. Its exits are real ones: every button maps
// to one `GuardDecision`, and "Choose a different folder" is a real cancel,
// not a broken version of this screen.
//
// It asks and nothing else. The walk that runs before the question is a
// `checking` ScreenContent on the one screen that owns waiting
// (screen/surface.ts), so this file has one job.
//
// The blueprint grid behind it is the OVERLAY's, not this screen's: it runs
// for the whole stretch the overlay is up, so the confirm beat's state swap
// (and every prompt re-render) leaves it alone instead of re-seeding its
// particle field. See screen/shell.ts.
//
// The surface pattern (re-render guard, deps-owned teardown) is copied from screen/surface.ts.

import { t } from "../../i18n/index";
import { escapeHtml } from '../screen/shell';
import { renderScreen } from '../screen/overlay-slot';
import {
  offersMakeRoot,
  rootFolderName,
  type NestedRootInfo,
  type ThresholdPrompt,
} from './seam';
import type { GuardDecision } from "../../host";

/** Enter is swallowed for this long after any (re)render. A late find can
 *  force a full re-render at the exact instant the user presses Return, and
 *  a queued keypress must never commit a card they have not read. */
export const RENDER_ENTER_GUARD_MS = 250;

/** The quiet bordered escape every variant carries (waiting-screen behavior reused). */
const cancelButton = () =>
  `<button type="button" class="moss-btn moss-btn-secondary" data-threshold-cancel>${escapeHtml(
    t('preview.waiting_choose_folder'),
  )}</button>`;

/**
 * One nested-site card. Pickable cards are real <button>s in a roving-tabindex
 * list; case (a)'s single card is informational (its action is the primary
 * button below it). The full path rides on `data-tooltip` (never `title=`) and
 * on `aria-label` so a truncated path stays reachable.
 */
function card(info: NestedRootInfo, opts: { pick: boolean; first: boolean }): string {
  const name = escapeHtml(info.name);
  const rel = escapeHtml(info.rel_path);
  const abs = escapeHtml(info.abs_path);
  const inner = `<span class="moss-threshold__card-name">${name}</span>
      <span class="moss-threshold__card-path">${rel}</span>`;
  if (!opts.pick) {
    return `<div class="moss-threshold__card" role="listitem" data-tooltip="${abs}" aria-label="${name} — ${abs}">${inner}</div>`;
  }
  const primary = opts.first ? ' data-threshold-primary' : '';
  return `<button type="button" class="moss-threshold__card moss-threshold__card--pick"
      data-threshold-card data-abs="${abs}" tabindex="${opts.first ? 0 : -1}"
      data-tooltip="${abs}" aria-label="${name} — ${abs}"${primary}>${inner}</button>`;
}

/** The muted "Still checking" footer under a progressively-revealed list. */
const stillCheckingFooter = (visible: boolean) =>
  `<p class="moss-threshold__footer" data-threshold-footer${visible ? '' : ' hidden'} aria-live="polite">${escapeHtml(
    t('threshold.still_checking'),
  )}</p>`;

/**
 * A capped walk may have missed sites, and once it ends the list would
 * otherwise LOOK complete — the cap-degrades-to-silence gap the design was
 * written to close. Emitted only when the report says truncated; hidden
 * while the still-checking footer is up, because "still checking" and
 * "stopped early" cannot both be true.
 */
const truncatedNote = (truncated: boolean, stillChecking: boolean) =>
  truncated
    ? `<p class="moss-threshold__footer" data-threshold-truncated${stillChecking ? ' hidden' : ''}>${escapeHtml(
        t('threshold.truncated_note'),
      )}</p>`
    : '';

function shell(mode: string, stackHtml: string): string {
  return `
    <div class="moss-threshold" data-mode="${mode}">
      <div class="moss-threshold__stack">${stackHtml}</div>
    </div>
  `;
}

/** Cases (a) and (b): the folder is not a site yet; one of its descendants is. */
function unownedTemplate(prompt: ThresholdPrompt, stillChecking: boolean): string {
  const folder = rootFolderName(prompt.report);
  const nested = prompt.report.nested;
  const one = nested.length === 1;
  const title = one
    ? t('threshold.one_title')
    : t('threshold.many_title', { count: String(nested.length) });
  const body = one
    ? t('threshold.one_body', { folder })
    : t('threshold.many_body', { folder });

  const cards = nested
    .map((info, i) => card(info, { pick: !one, first: i === 0 }))
    .join('\n');

  const primaryAction = one
    ? `<div class="moss-threshold__actions">
        <button type="button" class="moss-btn moss-btn-primary" data-threshold-primary
          data-abs="${escapeHtml(nested[0].abs_path)}">${escapeHtml(
            t('threshold.open_site', { name: nested[0].name }),
          )}</button>
      </div>`
    : '';

  const makeRoot = offersMakeRoot(prompt)
    ? `<button type="button" class="moss-link--subtle" data-threshold-make-root>${escapeHtml(
        t('threshold.make_root_instead', { folder }),
      )}</button>`
    : '';

  return shell(
    one ? 'one' : 'many',
    `<h1 class="moss-threshold__title">${escapeHtml(title)}</h1>
      <p class="moss-threshold__body">${escapeHtml(body)}</p>
      <div class="moss-threshold__cards" role="list" data-threshold-cards>${cards}</div>
      ${primaryAction}
      ${stillCheckingFooter(stillChecking)}
      ${truncatedNote(prompt.report.truncated, stillChecking)}
      <div class="moss-threshold__escapes">
        ${makeRoot}
        ${cancelButton()}
      </div>`,
  );
}

/**
 * Case (d), per the amendment: the outer folder and the nested site(s) are
 * peers under ONE question — which folder is the correct site. Picking either
 * opens it; the guard un-vaults the other in the same action. The nested
 * site(s) come first (they are usually what people meant), so the first
 * nested card is the pre-focused fast lane.
 */
function ownedTemplate(prompt: ThresholdPrompt, stillChecking: boolean): string {
  const folder = rootFolderName(prompt.report);
  const nested = prompt.report.nested;
  const body =
    nested.length === 1
      ? t('threshold.which_body_one', { folder })
      : t('threshold.which_body_many', { folder, count: String(nested.length) });

  const outerCard = (first: boolean) => `<button type="button" class="moss-threshold__card moss-threshold__card--pick"
      data-threshold-card data-threshold-outer tabindex="${first ? 0 : -1}"${first ? ' data-threshold-primary' : ''}
      data-tooltip="${escapeHtml(prompt.report.root)}"
      aria-label="${escapeHtml(folder)} — ${escapeHtml(prompt.report.root)}">
      <span class="moss-threshold__card-name">${escapeHtml(folder)}</span>
      <span class="moss-threshold__card-path">${escapeHtml(t('threshold.outer_card_label'))}</span>
    </button>`;

  // Amendment 2: lead with the decision layer's guess when it made one. The
  // suggested card comes first and is the focused primary; everything else is
  // the same equal-weight card one interaction away. A nested suggestion that
  // names no known candidate is rendered as no suggestion (defensive — the
  // backend owns the heuristic, the screen only orders cards by it).
  const sug = prompt.suggestion;
  const sugNested =
    sug?.kind === 'nested' ? nested.find((n) => n.abs_path === sug.abs_path) : undefined;
  const effective = sug?.kind === 'outer' ? sug : sugNested ? sug : null;

  const items: string[] = [];
  if (effective?.kind === 'outer') {
    items.push(outerCard(true));
    for (const info of nested) items.push(card(info, { pick: true, first: false }));
  } else if (effective) {
    items.push(card(sugNested!, { pick: true, first: true }));
    for (const info of nested) {
      if (info !== sugNested) items.push(card(info, { pick: true, first: false }));
    }
    items.push(outerCard(false));
  } else {
    nested.forEach((info, i) => items.push(card(info, { pick: true, first: i === 0 })));
    items.push(outerCard(false));
  }

  const title = effective
    ? t('threshold.suggest_title', {
        name: effective.kind === 'outer' ? folder : sugNested!.name,
      })
    : t('threshold.which_title');

  return shell(
    'which',
    `<h1 class="moss-threshold__title">${escapeHtml(title)}</h1>
      <p class="moss-threshold__body">${escapeHtml(body)}</p>
      <div class="moss-threshold__cards" role="list" data-threshold-cards>
        ${items.join('\n')}
      </div>
      <p class="moss-threshold__note">${escapeHtml(t('threshold.remove_note'))}</p>
      ${stillCheckingFooter(stillChecking)}
      ${truncatedNote(prompt.report.truncated, stillChecking)}
      <div class="moss-threshold__escapes">
        ${cancelButton()}
      </div>`,
  );
}

/** The inline confirm beat for "make this folder a site" — a state swap of
 *  the same screen, never a second dialog. "Go back" is pre-focused: the
 *  irreversible-ish action is one deliberate Tab or click away, never the
 *  default keypress. */
function makeRootConfirmTemplate(prompt: ThresholdPrompt): string {
  const folder = rootFolderName(prompt.report);
  const path = prompt.report.nested[0]?.rel_path ?? '';
  return shell(
    'confirm-make-root',
    `<h1 class="moss-threshold__title">${escapeHtml(t('threshold.make_root_title', { folder }))}</h1>
      <p class="moss-threshold__body">${escapeHtml(
        t('threshold.make_root_body', { folder, path }),
      )}</p>
      <div class="moss-threshold__actions">
        <button type="button" class="moss-btn moss-btn-secondary" data-threshold-back>${escapeHtml(
          t('action.go_back'),
        )}</button>
        <button type="button" class="moss-btn moss-btn-primary" data-threshold-confirm>${escapeHtml(
          t('threshold.make_root_confirm'),
        )}</button>
      </div>`,
  );
}

export const thresholdTemplates = {
  prompt: (prompt: ThresholdPrompt, stillChecking: boolean) =>
    prompt.kind === 'owned'
      ? ownedTemplate(prompt, stillChecking)
      : unownedTemplate(prompt, stillChecking),
  makeRootConfirm: makeRootConfirmTemplate,
};

/**
 * The identity of a prompt's *markup*, as opposed to its live details. The
 * surface re-renders when this changes; a same-key call is a no-op, and a
 * candidate list that merely GREW is appended in place so late finds never
 * move focus (progressive reveal).
 */
export function thresholdShapeKey(prompt: ThresholdPrompt): string {
  const sug =
    prompt.suggestion === null
      ? ''
      : prompt.suggestion.kind === 'outer'
        ? 'outer'
        : `nested:${prompt.suggestion.abs_path}`;
  return `${prompt.kind}/${sug}/${prompt.report.truncated}/${prompt.report.root}/${prompt.report.nested
    .map((n) => n.abs_path)
    .join('|')}`;
}

/** A changed guess is new markup (the lead sentence and card order move). */
function sameSuggestion(a: ThresholdPrompt, b: ThresholdPrompt): boolean {
  if (a.suggestion === null || b.suggestion === null) return a.suggestion === b.suggestion;
  if (a.suggestion.kind !== b.suggestion.kind) return false;
  return a.suggestion.kind === 'outer' ||
    a.suggestion.abs_path === (b.suggestion as { abs_path: string }).abs_path;
}

/**
 * The threshold screen as a surface: what is on the overlay, and what has to
 * be torn down when it leaves. Mirrors FullWindowScreen (screen/surface.ts):
 * the manager keeps only the decision to show it.
 */
export class ThresholdScreenSurface {
  /** Shape key of the live prompt (null = not showing a prompt). */
  private promptKey: string | null = null;
  /** Abs paths of the cards currently in the DOM, for progressive append. */
  private renderedPaths: string[] = [];
  private stillChecking = false;
  /** The prompt behind the confirm beat, to swap back on "Go back". */
  private livePrompt: ThresholdPrompt | null = null;
  private mode: 'none' | 'prompt' | 'confirm' = 'none';
  /**
   * One-way latch: the guard awaits exactly ONE decision, and on this screen
   * a second one is not a duplicate but a contradiction — in case (d) the two
   * cards delete opposite `.moss` folders. First decide flips it and disarms
   * every control; it re-arms only when a NEW render asks a new question
   * (never in drop(), so markup a caller leaves behind stays inert).
   */
  private decided = false;
  /** The user has interacted (pointer or key) with the current question.
   *  Once true, a full re-render restores THEIR focus instead of yanking it
   *  back to the primary. Reset when a new question starts (teardown). */
  private engaged = false;
  /** When the current markup was rendered — the Enter guard's clock. */
  private renderedAt = 0;
  /** Containers whose engagement/Enter-guard listeners are already attached
   *  (they live on the container itself and survive innerHTML swaps). */
  private readonly containerWired = new WeakSet<HTMLElement>();
  /** The container the live markup was rendered into — `rearm()`'s target. */
  private container: HTMLElement | null = null;

  constructor(
    private readonly deps: {
      /** The screen's one output: a GuardDecision for the awaiting guard. */
      decide: (decision: GuardDecision) => void;
    },
  ) {}

  /**
   * Show (or progressively extend) the prompt. While the walk is still
   * running, `stillChecking` keeps the muted footer up; late finds with the
   * same kind/root APPEND cards without moving focus. A first render focuses
   * the primary element — Return commits the fast lane.
   */
  showPrompt(container: HTMLElement, prompt: ThresholdPrompt, stillChecking = false): void {
    this.container = container;
    const key = thresholdShapeKey(prompt);
    const paths = prompt.report.nested.map((n) => n.abs_path);
    const liveList = this.mode === 'prompt' && !!container.querySelector('[data-threshold-cards]');

    if (liveList && key === this.promptKey) {
      this.setStillChecking(container, stillChecking);
      return;
    }

    // Progressive reveal: same question, more candidates → append below,
    // never re-render (a re-render would blur whatever the user focused).
    if (
      liveList &&
      this.livePrompt?.kind === prompt.kind &&
      this.livePrompt.report.root === prompt.report.root &&
      // A truncated flip adds the incomplete-list note, which is new markup.
      this.livePrompt.report.truncated === prompt.report.truncated &&
      sameSuggestion(this.livePrompt, prompt) &&
      paths.length > this.renderedPaths.length &&
      this.renderedPaths.every((p, i) => p === paths[i]) &&
      // Case (a) upgrades to case (b) markup (a picker list), so a growth out
      // of the single-card unowned shape is a re-render, not an append.
      !(prompt.kind === 'unowned' && this.renderedPaths.length === 1)
    ) {
      const list = container.querySelector<HTMLElement>('[data-threshold-cards]')!;
      const pick = prompt.kind === 'owned' || prompt.report.nested.length > 1;
      const newCards = prompt.report.nested
        .slice(this.renderedPaths.length)
        .map((info) => card(info, { pick, first: false }))
        .join('\n');
      // The owned list ends with the outer-folder card (unless the outer is
      // the suggested lead); new finds slot in above a trailing outer so the
      // nested peers stay grouped, and append at the end otherwise.
      const outer = list.querySelector('[data-threshold-outer]');
      if (outer && !outer.nextElementSibling) outer.insertAdjacentHTML('beforebegin', newCards);
      else list.insertAdjacentHTML('beforeend', newCards);
      this.wireCards(container);
      this.promptKey = key;
      this.renderedPaths = paths;
      this.livePrompt = prompt;
      this.setStillChecking(container, stillChecking);
      return;
    }

    // A full re-render replaces the markup under the user's hands. Remember
    // whether they had engaged (and where their focus was) across teardown,
    // so a late SHALLOWER find — which fails the append prefix-check above —
    // cannot yank focus back to the primary with a Return already in flight.
    const wasEngaged = this.engaged;
    const prevFocus = this.focusKey(container);
    this.teardown();
    this.engaged = wasEngaged;
    this.mode = 'prompt';
    this.decided = false;
    this.promptKey = key;
    this.renderedPaths = paths;
    this.livePrompt = prompt;
    this.stillChecking = stillChecking;
    this.wireContainer(container);
    this.renderedAt = Date.now();
    renderScreen(container, thresholdTemplates.prompt(prompt, stillChecking));
    container.classList.add('visible');
    this.wire(container);
    if (!this.engaged) {
      container.querySelector<HTMLElement>('[data-threshold-primary]')?.focus();
    } else if (prevFocus) {
      this.focusByKey(container, prevFocus);
    }
  }

  /** Detection ended: the footer comes down without touching the list, and a
   *  capped walk's incomplete-list note comes up in its place. */
  private setStillChecking(container: HTMLElement, on: boolean): void {
    if (on === this.stillChecking) return;
    this.stillChecking = on;
    const footer = container.querySelector<HTMLElement>('[data-threshold-footer]');
    if (footer) {
      if (on) footer.removeAttribute('hidden');
      else footer.setAttribute('hidden', '');
    }
    const note = container.querySelector<HTMLElement>('[data-threshold-truncated]');
    if (note) {
      if (on) note.setAttribute('hidden', '');
      else note.removeAttribute('hidden');
    }
  }

  /** Forget the screen, because its markup is about to be written over. */
  drop(): void {
    this.teardown();
  }

  private teardown(): void {
    // `container` is deliberately NOT nulled: the render paths set it before
    // calling teardown(), and rearm()'s `mode === 'none'` gate already keeps
    // a dropped surface from touching stale markup.
    this.mode = 'none';
    this.promptKey = null;
    this.renderedPaths = [];
    this.livePrompt = null;
    this.stillChecking = false;
    this.engaged = false;
  }

  /** Container-level listeners: engagement tracking and the Enter guard.
   *  Attached once per container; `innerHTML` swaps never detach them. */
  private wireContainer(container: HTMLElement): void {
    if (this.containerWired.has(container)) return;
    this.containerWired.add(container);
    container.addEventListener('pointerdown', () => {
      this.engaged = true;
    });
    container.addEventListener(
      'keydown',
      (e: KeyboardEvent) => {
        if (e.key === 'Enter' && Date.now() - this.renderedAt < RENDER_ENTER_GUARD_MS) {
          e.preventDefault();
          e.stopPropagation();
          return;
        }
        this.engaged = true;
      },
      true,
    );
  }

  /** Which control holds focus right now, as a key that survives a re-render. */
  private focusKey(container: HTMLElement): string | null {
    const el = document.activeElement;
    if (!(el instanceof HTMLElement) || !container.contains(el)) return null;
    if (el.hasAttribute('data-threshold-outer')) return 'outer';
    if (el.dataset.abs) return `abs:${el.dataset.abs}`;
    if (el.hasAttribute('data-threshold-cancel')) return 'cancel';
    if (el.hasAttribute('data-threshold-make-root')) return 'make-root';
    return null;
  }

  /** Re-focus the control `focusKey` named, if the new markup still has it. */
  private focusByKey(container: HTMLElement, key: string): void {
    let el: HTMLElement | null = null;
    if (key === 'outer') el = container.querySelector('[data-threshold-outer]');
    else if (key === 'cancel') el = container.querySelector('[data-threshold-cancel]');
    else if (key === 'make-root') el = container.querySelector('[data-threshold-make-root]');
    else if (key.startsWith('abs:')) {
      const abs = key.slice(4);
      el =
        Array.from(container.querySelectorAll<HTMLElement>('[data-abs]')).find(
          (c) => c.dataset.abs === abs,
        ) ?? null;
    }
    if (!el) return; // the control is gone; do not guess at a substitute
    if (el.hasAttribute('data-threshold-card')) {
      for (const c of container.querySelectorAll<HTMLElement>('[data-threshold-card]')) {
        c.tabIndex = c === el ? 0 : -1;
      }
    }
    el.focus();
  }

  /**
   * A decide send failed TRANSIENTLY (the question is still parked backend-
   * side): drop the one-way latch and re-enable the controls, so the user can
   * answer again instead of facing a disarmed screen over a still-pending
   * question. A refusal (answered/superseded/nobody-awaiting) never comes
   * here — the pass closes the screen instead.
   */
  rearm(): void {
    if (this.mode === 'none' || !this.decided) return;
    this.decided = false;
    if (!this.container) return;
    for (const b of this.container.querySelectorAll<HTMLButtonElement>('button')) {
      b.disabled = false;
    }
  }

  /** The single exit for every decision path: latch, disarm, then hand over. */
  private submit(container: HTMLElement, decision: GuardDecision): void {
    if (this.decided || this.mode === 'none') return;
    this.decided = true;
    for (const b of container.querySelectorAll<HTMLButtonElement>('button')) {
      b.disabled = true;
    }
    this.deps.decide(decision);
  }

  /** Wire every control the current markup carries. */
  private wire(container: HTMLElement): void {
    container
      .querySelector<HTMLButtonElement>('[data-threshold-cancel]')
      ?.addEventListener('click', () => this.submit(container, { kind: 'cancel' }));

    // Case (a)'s primary is a plain button carrying its target's abs path;
    // case (b)/(d) primaries are cards, wired below.
    const primary = container.querySelector<HTMLButtonElement>('button[data-threshold-primary]:not([data-threshold-card])');
    if (primary?.dataset.abs) {
      const abs = primary.dataset.abs;
      primary.addEventListener('click', () =>
        this.submit(container, { kind: 'open_nested', abs_path: abs }),
      );
    }

    container.querySelector<HTMLButtonElement>('[data-threshold-make-root]')?.addEventListener(
      'click',
      () => this.showMakeRootConfirm(container),
    );

    container
      .querySelector<HTMLButtonElement>('[data-threshold-confirm]')
      ?.addEventListener('click', () => this.submit(container, { kind: 'open_here' }));

    container.querySelector<HTMLButtonElement>('[data-threshold-back]')?.addEventListener(
      'click',
      () => this.backToPrompt(container),
    );

    this.wireCards(container);
  }

  /** Roving-tabindex picker list: ↑/↓ move, Return/click opens. Re-run after a
   *  progressive append (idempotent — listeners are per-element and appended
   *  elements are the only unwired ones). */
  private wireCards(container: HTMLElement): void {
    const cards = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
    );
    cards.forEach((el) => {
      if (el.dataset.thresholdWired) return;
      el.dataset.thresholdWired = '1';
      el.addEventListener('click', () => {
        if (el.hasAttribute('data-threshold-outer')) {
          this.submit(container, { kind: 'open_here' });
        } else if (el.dataset.abs) {
          this.submit(container, { kind: 'open_nested', abs_path: el.dataset.abs });
        }
      });
      el.addEventListener('keydown', (e: KeyboardEvent) => {
        if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
        e.preventDefault();
        const all = Array.from(
          container.querySelectorAll<HTMLButtonElement>('[data-threshold-card]'),
        );
        const at = all.indexOf(el);
        const next = all[(at + (e.key === 'ArrowDown' ? 1 : all.length - 1)) % all.length];
        all.forEach((c) => (c.tabIndex = c === next ? 0 : -1));
        next.focus();
      });
    });
  }

  /** State swap into the confirm beat — same screen, new question, Back focused. */
  private showMakeRootConfirm(container: HTMLElement): void {
    if (!this.livePrompt) return;
    this.mode = 'confirm';
    this.renderedAt = Date.now();
    renderScreen(container, thresholdTemplates.makeRootConfirm(this.livePrompt));
    this.wire(container);
    container.querySelector<HTMLElement>('[data-threshold-back]')?.focus();
  }

  /** Back out of the confirm beat to the prompt, focus returned to the link
   *  that opened it (the least surprising place to resume). */
  private backToPrompt(container: HTMLElement): void {
    if (!this.livePrompt) return;
    const prompt = this.livePrompt;
    const still = this.stillChecking;
    this.promptKey = null; // force a full prompt render
    this.showPrompt(container, prompt, still);
    container.querySelector<HTMLElement>('[data-threshold-make-root]')?.focus();
  }
}
