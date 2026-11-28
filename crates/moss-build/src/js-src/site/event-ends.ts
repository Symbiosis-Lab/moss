/**
 * event-ends.ts — what the build drew for an event that had not ended turns
 * to past once it ends, without a rebuild.
 *
 * Two marks, both carrying the event's end as wall-clock time with no zone,
 * read in the visitor's own zone as the build reads its own clock. The event
 * is over from that minute on, as it is for the build.
 *
 * - `<template data-hero-ends>` follows a hero drawn from the event and holds
 *   the hero the page falls back to (empty when it has none); a template is
 *   never drawn, so the fallback costs nothing until it is swapped in.
 * - `data-ends` sits beside `data-when="upcoming"` on an event's meta line,
 *   card or row. At its end the element becomes `data-when="past"` and loses
 *   what a past event has no use for: Tickets, Add to calendar and the
 *   Upcoming mark.
 *
 * Without the script the page stays as of the last build.
 */

const PAST_DROPS = ".moss-event-tickets, .moss-event-calendar, .moss-event-upcoming";

const ended = (end: string | undefined, now: number) => {
  const at = Date.parse(end ?? "");
  return !Number.isNaN(at) && now >= at;
};

export function initEventEnds(root: ParentNode = document, now: number = Date.now()): void {
  root.querySelectorAll<HTMLTemplateElement>("template[data-hero-ends]").forEach((template) => {
    if (!ended(template.dataset.heroEnds, now)) return;
    // The drawn hero is the section before the template, and its caption, when
    // it has one, is the paragraph between them. Without that section the
    // template stays inert: the fallback is never added beside a drawn hero.
    let drawn = template.previousElementSibling;
    const caption = drawn?.matches("p.moss-hero-caption") ? drawn : null;
    if (caption) drawn = caption.previousElementSibling;
    if (!drawn?.matches(".moss-hero")) return;
    caption?.remove();
    drawn.remove();
    template.replaceWith(template.content);
  });
  root.querySelectorAll<HTMLElement>('[data-when="upcoming"][data-ends]').forEach((event) => {
    if (!ended(event.dataset.ends, now)) return;
    event.dataset.when = "past";
    delete event.dataset.ends;
    event.querySelectorAll(PAST_DROPS).forEach((drop) => drop.remove());
  });
}

const run = () => initEventEnds();
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", run);
} else {
  run();
}
document.addEventListener("moss-morph-patched", run);
