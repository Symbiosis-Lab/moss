/**
 * chip.ts — the breadcrumb scope chip: the map's one hierarchy control.
 *
 * One glass chip at the viewport's top left, showing the trail from "every
 * work" down to the current scope and letting a reader move either
 * direction: an earlier crumb widens back out, the terminal crumb's chevron
 * (when places lie under it) opens a `role="menu"` of its own children to
 * dig further in. Widening and narrowing are the SAME call
 * (`ChipCallbacks.setScope`, `map.ts`'s own `setScope`) a card's place link
 * and a zoom-driven re-cluster already make — this module adds no second
 * way to change scope, only a third surface that reaches the one mechanism
 * `scope.ts`/`state.ts` already own.
 *
 * When the map has a current article (`currentArticleId`: the embed's own
 * article, `embed.ts`), the root crumb becomes a two-segment switch, "This
 * article | All articles"; the place trail and the place menu continue only
 * after "All articles". An open ring appends one more, non-widenable crumb
 * after the scope's own trail (`scope.ts`'s `crumbs`, reused whole, never
 * re-derived here); the crumb just before it is what closes the ring,
 * because clicking ANY non-terminal crumb calls `setScope` on its own
 * (possibly unchanged) scope, and `setScope` already closes the ring as its
 * first step (`map.ts`) — no ring-specific click handling exists in this
 * file at all. A selected card is a selection, not a scope: it never
 * changes the trail.
 */
import { childrenOf, crumbs as scopeCrumbs, type ScopeChild } from "./scope";
import { worksHereLabel, type PlacesStrings } from "./strings";
import type { Place, Scope, Work } from "./types";

const SCOPE_OPTION = "moss-places-chip-scope-option";
const CRUMB = "moss-places-chip-crumb";

const CHEVRON_SVG =
  '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" ' +
  'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>';

export interface DisplayCrumb {
  label: string;
  /** Widen target. `null` only on the trail's own terminal crumb (current scope or an open ring's own trailing crumb) — every crumb before it always carries one. */
  target: Scope | null;
}

function labelForScope(scope: Scope, places: Place[], strings: PlacesStrings, switchable: boolean): string {
  // The root names places, except as the "All articles" half of an article's own scope switch.
  if (scope.kind === "all") return switchable ? strings.chipAllArticles : strings.chipAll;
  if (scope.kind === "article") return strings.chipThisArticle;
  return places.find((place) => place.id === scope.id)?.name ?? scope.id;
}

/**
 * The chip's crumb trail for the current scope/ring state. Pure —
 * no DOM, no callbacks — so narrowing/widening and the menu's own item list
 * can be tested without mounting the map.
 */
export function deriveCrumbs(
  scope: Scope,
  places: Place[],
  ringCount: number | null,
  strings: PlacesStrings,
  lang: string,
  switchable = false,
): DisplayCrumb[] {
  const trail: DisplayCrumb[] = scopeCrumbs(scope, places).map((crumb) => ({
    label: labelForScope(crumb, places, strings, switchable),
    target: crumb,
  }));
  if (ringCount != null) {
    trail.push({ label: worksHereLabel(strings, lang, ringCount), target: null });
    return trail;
  }
  // No ring: the scope's own last rung is the terminal crumb, not a widen target.
  trail[trail.length - 1] = { ...trail[trail.length - 1], target: null };
  return trail;
}

export interface ChipCallbacks {
  /** Widen or narrow — the one scope-change entry point this module ever calls (`map.ts`'s own `setScope`, which already closes any open ring). */
  setScope(scope: Scope): void;
  /** Switch between the current article alone (`true`) and every article (`false`) — `map.ts`'s own `setArticleMode`, which keeps the "all" side's scope and camera for the way back. */
  setArticleMode(articleOnly: boolean): void;
  /** Exempt a place's own works from the marker layer's dimming while a menu item is hovered/focused, `null` to lift it — the SAME `data-dimmed` attribute a bloomed ring already uses. */
  highlightPlace(placeId: string | null): void;
  /** Called right after the dig-down menu actually opens or closes (never on a `closeMenu()` call that found nothing open) — map.ts's own hook to re-run `reservedLabelRects()` and the label layer against it, the one thing opening or closing this menu changes that is not already one of `render()`'s own four keyed inputs (scope/ring/locale), so nothing else in this module's own render path would otherwise re-run it. */
  menuToggled(): void;
}

/** The breadcrumb chip: crumb trail, the terminal crumb's digging-down menu, and the phone-width collapse (CSS-driven; see places-explorer.css). */
export class ScopeChip {
  private readonly container: HTMLElement;
  private readonly callbacks: ChipCallbacks;
  private readonly strings: PlacesStrings;
  private readonly lang: string;
  private menuEl: HTMLElement | null = null;
  private menuTrigger: HTMLButtonElement | null = null;
  private menuItems: HTMLButtonElement[] = [];
  private focusIndex = -1;
  /** A key of the last inputs `render()` actually drew, so an unrelated camera settle (pan/zoom/resize, none of which touch scope, selection, ring count or locale) is a no-op instead of tearing down an open menu or a focused crumb. `null` until the first render. */
  private lastRenderKey: string | null = null;
  /** Set right before a click calls `setScope`/`setArticleMode`, which synchronously reaches back into `render()` through `map.ts`'s own settle path — the CSS selector of the element that render must focus, instead of leaving focus on the element `replaceChildren` just removed. */
  private focusOnNextRender: string | null = null;

  constructor(container: HTMLElement, callbacks: ChipCallbacks, strings: PlacesStrings, lang: string) {
    this.container = container;
    this.callbacks = callbacks;
    this.strings = strings;
    this.lang = lang;
    // Mirrors markers.ts's own ring-outside-click guard. Escape is not
    // handled here: map.ts's one Escape handler decides which open thing it
    // closes (see `dismissMenu`).
    document.addEventListener("pointerdown", (event) => {
      if (!this.menuEl) return;
      if (event.target instanceof Node && this.container.contains(event.target)) return;
      this.closeMenu();
    });
  }

  /**
   * Rebuild the chip for the current state — but only when the state is
   * actually different from the last render. `map.ts` calls this on every
   * settled camera application, including ones no scope/selection/ring
   * change caused (a resize, a zoom-button press, the ResizeObserver
   * firing): rebuilding on those too used to discard the trigger button an
   * open menu was anchored to and the focus on it, and could drop a
   * press-then-release click if a settle landed in between. Comparing a key
   * of the only four things a crumb trail actually depends on — places and
   * works never change for a mounted map, so they're not part of it — makes
   * every other settle a no-op: the open menu, and whatever has focus,
   * survive untouched.
   */
  render(scope: Scope, places: Place[], works: Work[], ringCount: number | null, currentArticleId: string | null): void {
    // One article on the site means both segments would show the same thing: no switch.
    const switchable = currentArticleId != null && works.length > 1;
    const key = JSON.stringify([scope, switchable, ringCount, this.lang]);
    if (key === this.lastRenderKey) return;
    this.lastRenderKey = key;

    this.closeMenu();
    const crumbList = deriveCrumbs(scope, places, ringCount, this.strings, this.lang, switchable);
    const terminalIsScope = ringCount == null;
    const children = terminalIsScope ? childrenOf(scope, places, works) : [];

    const trail = document.createElement("div");
    trail.className = "moss-places-chip-trail";

    if (switchable && scope.kind === "article") {
      trail.append(this.buildScopeGroup(this.buildAllOption(), true));
    } else {
      // In the switch, the root crumb is the "All articles" segment; otherwise a crumb as ever.
      const option = switchable;
      const root = crumbList.length === 1
        ? this.buildTerminalScope(crumbList[0], children, option)
        : this.buildWidenCrumb(crumbList[0], option);
      trail.append(option ? this.buildScopeGroup(root, false) : root);
      if (crumbList.length > 1) {
        const middle = crumbList.slice(1, -1);
        if (middle.length > 0) {
          const collapsible = document.createElement("span");
          collapsible.className = "moss-places-chip-collapsible";
          for (const crumb of middle) {
            collapsible.append(this.buildSeparator(), this.buildWidenCrumb(crumb));
          }
          trail.append(collapsible);
          trail.append(this.buildEllipsisToggle(collapsible));
        }
        trail.append(this.buildSeparator());
        const last = crumbList[crumbList.length - 1];
        trail.append(terminalIsScope ? this.buildTerminalScope(last, children) : this.buildLeaf(last));
      }
    }

    this.container.replaceChildren(trail);
    if (this.focusOnNextRender) {
      this.container.querySelector<HTMLElement>(this.focusOnNextRender)?.focus();
      this.focusOnNextRender = null;
    }
  }

  /** The two-segment switch: "This article" then `allOption`; whichever is the current scope carries `aria-current`. */
  private buildScopeGroup(allOption: HTMLElement, articleIsCurrent: boolean): HTMLElement {
    const group = document.createElement("div");
    group.className = "moss-places-chip-scope";
    group.setAttribute("role", "group");
    group.setAttribute("aria-label", this.strings.chipScopeGroup);
    const article = document.createElement("button");
    article.type = "button";
    article.className = SCOPE_OPTION;
    article.dataset.scope = "article";
    article.textContent = this.strings.chipThisArticle;
    if (articleIsCurrent) article.setAttribute("aria-current", "true");
    else {
      article.addEventListener("click", () => {
        this.focusOnNextRender = '[data-scope="article"]';
        this.callbacks.setArticleMode(true);
      });
    }
    group.append(article, allOption);
    return group;
  }

  /** The "All articles" segment while the article alone is shown: a plain button back to every article. */
  private buildAllOption(): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = SCOPE_OPTION;
    button.dataset.scope = "all";
    button.textContent = this.strings.chipAllArticles;
    button.addEventListener("click", () => {
      this.focusOnNextRender = '[data-scope="all"]';
      this.callbacks.setArticleMode(false);
    });
    return button;
  }

  private buildSeparator(): HTMLElement {
    const sep = document.createElement("span");
    sep.className = "moss-places-chip-sep";
    sep.setAttribute("aria-hidden", "true");
    sep.textContent = "›";
    return sep;
  }

  private buildLeaf(crumb: DisplayCrumb): HTMLElement {
    const span = document.createElement("span");
    span.className = "moss-places-chip-crumb";
    span.dataset.terminal = "";
    // Not in the tab order (it opens nothing, widens nothing) but still a
    // valid `.focus()` target, so closing the dig-down menu by choosing a
    // place that turns out to be a leaf can still land focus on the new
    // terminal crumb instead of losing it to `<body>`.
    span.tabIndex = -1;
    span.textContent = crumb.label;
    return span;
  }

  /** `option`: the crumb is the "All articles" segment of the switch rather than a plain crumb. */
  private buildWidenCrumb(crumb: DisplayCrumb, option = false): HTMLElement {
    if (!crumb.target) return this.buildLeaf(crumb);
    const button = document.createElement("button");
    button.type = "button";
    button.className = option ? SCOPE_OPTION : CRUMB;
    if (option) this.markAllOption(button);
    button.textContent = crumb.label;
    const target = crumb.target;
    button.addEventListener("click", () => {
      this.closeMenu();
      if (option) this.focusOnNextRender = '[data-scope="all"]';
      this.callbacks.setScope(target);
    });
    return button;
  }

  private markAllOption(button: HTMLElement): void {
    button.dataset.scope = "all";
    button.setAttribute("aria-current", "true");
  }

  private buildTerminalScope(crumb: DisplayCrumb, children: ScopeChild[], option = false): HTMLElement {
    if (children.length === 0 && !option) return this.buildLeaf(crumb);

    const button = document.createElement("button");
    button.type = "button";
    button.className = option ? SCOPE_OPTION : CRUMB;
    if (option) this.markAllOption(button);
    button.dataset.terminal = "";
    button.id = `moss-places-chip-trigger-${Math.random().toString(36).slice(2, 8)}`;
    const label = document.createElement("span");
    label.textContent = crumb.label;
    button.append(label);
    if (children.length === 0) return button;
    button.setAttribute("aria-haspopup", "menu");
    button.setAttribute("aria-expanded", "false");
    const chevron = document.createElement("span");
    chevron.className = "moss-places-chip-chevron";
    chevron.innerHTML = CHEVRON_SVG;
    button.append(chevron);
    button.addEventListener("click", () => {
      if (this.menuEl) this.closeMenu();
      else this.openMenu(button, children);
    });
    return button;
  }

  /**
   * The phone-width stand-in for the collapsed middle crumbs: a real button,
   * not decorative text, so a reader on a narrow screen can still reach
   * every crumb by keyboard or touch. Activating it reveals `collapsible`
   * in place (`[data-expanded]` in places-explorer.css overrides the
   * breakpoint's own `display: none`); activating it again hides it. A
   * later render always builds this button fresh — see `render()`'s own
   * doc — so any real scope/selection/ring change collapses it back to its
   * default closed state, same as the review asked for.
   */
  private buildEllipsisToggle(collapsible: HTMLElement): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-places-chip-ellipsis";
    button.setAttribute("aria-expanded", "false");
    button.setAttribute("aria-label", this.strings.chipShowHidden);
    button.append(this.buildSeparator(), document.createTextNode("…"));
    button.addEventListener("click", () => {
      const expanded = button.getAttribute("aria-expanded") === "true";
      button.setAttribute("aria-expanded", String(!expanded));
      collapsible.toggleAttribute("data-expanded", !expanded);
    });
    return button;
  }

  private openMenu(trigger: HTMLButtonElement, children: ScopeChild[]): void {
    const menu = document.createElement("div");
    menu.className = "moss-places-chip-menu";
    menu.setAttribute("role", "menu");
    menu.setAttribute("aria-labelledby", trigger.id);

    this.menuItems = children.map((child) => {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "moss-places-chip-menu-item";
      item.setAttribute("role", "menuitem");
      item.tabIndex = -1;
      const name = document.createElement("span");
      name.textContent = child.place.name;
      const count = document.createElement("span");
      count.className = "moss-places-chip-menu-count";
      count.textContent = `(${new Intl.NumberFormat(this.lang).format(child.count)})`;
      item.append(name, count);
      item.addEventListener("click", () => {
        this.focusOnNextRender = "[data-terminal]";
        this.closeMenu();
        this.callbacks.setScope({ kind: "place", id: child.place.id });
      });
      item.addEventListener("mouseenter", () => this.callbacks.highlightPlace(child.place.id));
      item.addEventListener("mouseleave", () => this.callbacks.highlightPlace(null));
      item.addEventListener("focus", () => this.callbacks.highlightPlace(child.place.id));
      item.addEventListener("blur", () => this.callbacks.highlightPlace(null));
      menu.append(item);
      return item;
    });

    menu.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        this.moveFocus(1);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        this.moveFocus(-1);
      }
    });

    this.menuEl = menu;
    this.menuTrigger = trigger;
    trigger.setAttribute("aria-expanded", "true");
    // Anchored to the CHIP's own left edge (CSS), not the trigger's offset —
    // the chip's own `max-inline-size` already keeps it clear of the zoom
    // controls at any viewport, so anchoring the menu there the same way
    // keeps it on-screen too, at the cost of not sitting flush under a
    // terminal crumb that follows a long trail.
    this.container.append(menu);
    this.focusIndex = 0;
    const firstItem = this.menuItems[0];
    if (firstItem) {
      firstItem.tabIndex = 0;
      firstItem.focus();
    }
    this.callbacks.menuToggled();
  }

  private moveFocus(delta: number): void {
    if (this.menuItems.length === 0) return;
    const current = this.menuItems[this.focusIndex];
    if (current) current.tabIndex = -1;
    this.focusIndex = (this.focusIndex + delta + this.menuItems.length) % this.menuItems.length;
    const next = this.menuItems[this.focusIndex];
    next.tabIndex = 0;
    next.focus();
  }

  /** Escape's half of the menu: close it and hand focus back to the crumb that opened it. `false` when no menu was open, so the caller moves on to the next thing Escape may close. */
  dismissMenu(): boolean {
    if (!this.menuEl) return false;
    const trigger = this.menuTrigger;
    this.closeMenu();
    trigger?.focus();
    return true;
  }

  private closeMenu(): void {
    if (!this.menuEl) return;
    this.menuEl.remove();
    this.menuEl = null;
    this.menuItems = [];
    this.focusIndex = -1;
    this.menuTrigger?.setAttribute("aria-expanded", "false");
    this.menuTrigger = null;
    this.callbacks.highlightPlace(null);
    this.callbacks.menuToggled();
  }
}
