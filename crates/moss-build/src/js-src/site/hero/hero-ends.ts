/**
 * hero-ends.ts — a hero drawn from an event steps aside once the event has
 * ended. The build renders the drawn hero followed by
 * `<template data-hero-ends>` holding the hero the page falls back to (empty
 * when it has none); a template is never drawn, so the fallback costs the page
 * nothing until it is swapped in here.
 *
 * `data-hero-ends` is the event's end as wall-clock time with no zone, read in
 * the visitor's own zone, as the build compares it with its own clock. The
 * event is over from that minute on, as it is for the build. Without the
 * script the hero as of the last build stays.
 */
export function initHeroEnds(root: ParentNode = document, now: number = Date.now()): void {
  root.querySelectorAll<HTMLTemplateElement>("template[data-hero-ends]").forEach((template) => {
    const ends = Date.parse(template.dataset.heroEnds ?? "");
    if (Number.isNaN(ends) || now < ends) return;
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
}

const run = () => initHeroEnds();
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", run);
} else {
  run();
}
document.addEventListener("moss-morph-patched", run);
