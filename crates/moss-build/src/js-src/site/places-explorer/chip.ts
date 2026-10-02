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
 * Precedence between the three things a crumb trail can show, matching the
 * design: a selected work collapses the trail to exactly `all > This
 * article`, regardless of place scope or an open ring (the same "survives
 * independent of the ring" precedence `cards.ts`'s own `worksForRow` already
 * gives a selection over a ring's row restriction) — the chip never shows
 * two terminal states at once. Short of a selection, an open ring appends
 * one more, non-widenable crumb after the scope's own trail (`scope.ts`'s
 * `crumbs`, reused whole, never re-derived here); the crumb just before it
 * is what closes the ring, because clicking ANY non-terminal crumb calls
 * `setScope` on its own (possibly unchanged) scope, and `setScope` already
 * closes the ring as its first step (`map.ts`) — no ring-specific click
 * handling exists in this file at all.
 */
import { childrenOf, crumbs as scopeCrumbs, type ScopeChild } from "./scope";
import { worksHereLabel, type PlacesStrings } from "./strings";
import type { Place, Scope, Work } from "./types";

const CHEVRON_SVG =
  '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" ' +
  'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>';

export interface DisplayCrumb {
  label: string;
  /** Widen target. `null` only on the trail's own terminal crumb (current scope, the selected work, or an open ring's own trailing crumb) — every crumb before it always carries one. */
  target: Scope | null;
}

function labelForScope(scope: Scope, places: Place[], strings: PlacesStrings): string {
  if (scope.kind === "all") return strings.chipAll;
  if (scope.kind === "article") return strings.chipThisArticle;
  return places.find((place) => place.id === scope.id)?.name ?? scope.id;
}

/**
 * The chip's crumb trail for the current scope/selection/ring state. Pure —
 * no DOM, no callbacks — so narrowing/widening and the menu's own item list
 * can be tested without mounting the map.
 */
export function deriveCrumbs(
  scope: Scope,
  places: Place[],
  selectedWork: Work | null,
  ringCount: number | null,
  strings: PlacesStrings,
  lang: string,
): DisplayCrumb[] {
  if (selectedWork) {
    return [
      { label: strings.chipAll, target: { kind: "all" } },
      { label: strings.chipThisArticle, target: null },
    ];
  }
  const trail: DisplayCrumb[] = scopeCrumbs(scope, places).map((crumb) => ({
    label: labelForScope(crumb, places, strings),
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
  /** Clear the selected work; called only when widening away from the `This article` crumb. */
  selectWork(id: string | null): void;
  /** Exempt a place's own works from the marker layer's dimming while a menu item is hovered/focused, `null` to lift it — the SAME `data-dimmed` attribute a bloomed ring already uses. */
  highlightPlace(placeId: string | null): void;
  /** Called right after the dig-down menu actually opens or closes (never on a `closeMenu()` call that found nothing open) — map.ts's own hook to re-run `reservedLabelRects()` and the label layer against it, the one thing opening or closing this menu changes that is not already one of `render()`'s own four keyed inputs (scope/selection/ring/locale), so nothing else in this module's own render path would otherwise re-run it. */
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
  /** Set right before a dig-down menu item's click calls `setScope`, which synchronously reaches back into `render()` through `map.ts`'s own settle path — tells that render to land focus on the new trail's terminal crumb instead of leaving it on the menu item `replaceChildren` just removed. */
  private focusTerminalOnNextRender = false;

  constructor(container: HTMLElement, callbacks: ChipCallbacks, strings: PlacesStrings, lang: string) {
    this.container = container;
    this.callbacks = callbacks;
    this.strings = strings;
    this.lang = lang;
    // Mirrors markers.ts's own ring-outside-click guard: a menu item takes
    // real focus (unlike a clicked marker in WebKit), so Escape is handled
    // by the menu's own keydown listener below — only the outside-click
    // path needs a document-level reach.
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
  render(scope: Scope, places: Place[], works: Work[], selectedWork: Work | null, ringCount: number | null): void {
    const key = JSON.stringify([scope, selectedWork?.id ?? null, ringCount, this.lang]);
    if (key === this.lastRenderKey) return;
    this.lastRenderKey = key;

    this.closeMenu();
    const crumbList = deriveCrumbs(scope, places, selectedWork, ringCount, this.strings, this.lang);
    const terminalIsScope = selectedWork == null && ringCount == null;
    const children = terminalIsScope ? childrenOf(scope, places, works) : [];

    const trail = document.createElement("div");
    trail.className = "moss-places-chip-trail";

    if (crumbList.length === 1) {
      // The root crumb is both the trail's first and its only one — still
      // needs its own chevron/menu wiring when top-level places exist.
      trail.append(terminalIsScope ? this.buildTerminalScope(crumbList[0], children) : this.buildLeaf(crumbList[0]));
    } else {
      trail.append(this.buildWidenCrumb(crumbList[0]));
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

    this.container.replaceChildren(trail);
    if (this.focusTerminalOnNextRender) {
      this.focusTerminalOnNextRender = false;
      this.container.querySelector<HTMLElement>(".moss-places-chip-crumb[data-terminal]")?.focus();
    }
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

  private buildWidenCrumb(crumb: DisplayCrumb): HTMLElement {
    if (!crumb.target) return this.buildLeaf(crumb);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-places-chip-crumb";
    button.textContent = crumb.label;
    const target = crumb.target;
    button.addEventListener("click", () => {
      this.closeMenu();
      if (target.kind === "all") this.callbacks.selectWork(null);
      this.callbacks.setScope(target);
    });
    return button;
  }

  private buildTerminalScope(crumb: DisplayCrumb, children: ScopeChild[]): HTMLElement {
    if (children.length === 0) return this.buildLeaf(crumb);

    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-places-chip-crumb";
    button.dataset.terminal = "";
    button.id = `moss-places-chip-trigger-${Math.random().toString(36).slice(2, 8)}`;
    button.setAttribute("aria-haspopup", "menu");
    button.setAttribute("aria-expanded", "false");
    const label = document.createElement("span");
    label.textContent = crumb.label;
    const chevron = document.createElement("span");
    chevron.className = "moss-places-chip-chevron";
    chevron.innerHTML = CHEVRON_SVG;
    button.append(label, chevron);
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
        this.focusTerminalOnNextRender = true;
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
      } else if (event.key === "Escape") {
        event.preventDefault();
        this.closeMenu();
        trigger.focus();
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
