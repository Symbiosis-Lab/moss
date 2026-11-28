/**
 * Tests for event-ends.ts: once an event ends, a hero drawn from it steps
 * aside and its meta line, cards and rows read as past.
 */

import { describe, test, expect } from "vitest";
import { initEventEnds } from "../event-ends";

const NOW = new Date("2030-06-01T12:00").getTime();

const CAPTION = `<p class="moss-hero-caption">Drawn caption</p>`;
const SLIDESHOW =
  `<section class="moss-hero" data-slides="2"><div class="moss-hero-slides"><div class="moss-hero-slide"></div><div class="moss-hero-slide"></div></div>` +
  `<div class="moss-hero-captions"><p class="moss-hero-caption" data-slide="0">Slide one</p></div></section>`;

function page(ends: string, opts: { fallback?: string | null; caption?: boolean } = {}) {
  const fallback = opts.fallback === undefined ? `<section class="moss-hero"><h1>Fallback</h1></section>` : opts.fallback;
  document.body.innerHTML =
    `<header></header>` +
    `<section class="moss-hero" data-header="over"><h1>Drawn</h1></section>` +
    (opts.caption ? CAPTION : "") +
    `<template data-hero-ends="${ends}">${fallback ?? ""}</template>` +
    `<main>Body</main>`;
}

const heroes = () => [...document.querySelectorAll(".moss-hero")];
const captions = () => [...document.querySelectorAll("p.moss-hero-caption")];

describe("initEventEnds", () => {
  test("an ended hero is replaced by its fallback", () => {
    page("2030-05-31T22:00");
    initEventEnds(document, NOW);
    expect(heroes()).toHaveLength(1);
    expect(heroes()[0].textContent).toBe("Fallback");
    expect(document.querySelector("template")).toBeNull();
    expect(document.querySelector("header + .moss-hero")).not.toBeNull();
  });

  test("the drawn hero's caption goes with it", () => {
    page("2030-05-31T22:00", { caption: true, fallback: `<section class="moss-hero"></section><p class="moss-hero-caption">Fallback caption</p>` });
    initEventEnds(document, NOW);
    expect(captions().map((c) => c.textContent)).toEqual(["Fallback caption"]);
    expect(document.body.textContent).not.toContain("Drawn");
  });

  test("a slideshow fallback arrives whole", () => {
    page("2030-05-31T22:00", { caption: true, fallback: SLIDESHOW });
    initEventEnds(document, NOW);
    expect(heroes()).toHaveLength(1);
    expect(heroes()[0].getAttribute("data-slides")).toBe("2");
    expect(document.querySelectorAll(".moss-hero-slide")).toHaveLength(2);
    expect(captions().map((c) => c.textContent)).toEqual(["Slide one"]);
  });

  test("a hero that has not ended stays, and the fallback stays unrendered", () => {
    page("2030-06-01T12:01", { caption: true });
    initEventEnds(document, NOW);
    expect(heroes()).toHaveLength(1);
    expect(captions()).toHaveLength(1);
    expect(document.querySelector("template")).not.toBeNull();
  });

  test("the hero is over from the minute it ends", () => {
    page("2030-06-01T12:00");
    initEventEnds(document, NOW);
    expect(heroes()[0].textContent).toBe("Fallback");

    page("2030-06-01T12:00");
    initEventEnds(document, NOW - 1);
    expect(heroes()[0].textContent).toBe("Drawn");
  });

  test("with no fallback the ended hero and its caption are removed", () => {
    page("2030-05-31T22:00", { fallback: null, caption: true });
    initEventEnds(document, NOW);
    expect(heroes()).toHaveLength(0);
    expect(captions()).toHaveLength(0);
    expect(document.querySelector("main")).not.toBeNull();
  });

  test("an unreadable end changes nothing", () => {
    page("soon");
    initEventEnds(document, NOW);
    expect(heroes()[0].textContent).toBe("Drawn");
    expect(document.querySelector("template")).not.toBeNull();
  });

  test("a template that does not follow a drawn hero stays inert", () => {
    document.body.innerHTML =
      `<header></header><p>Intro</p>` +
      `<template data-hero-ends="2030-05-31T22:00"><section class="moss-hero"><h1>Fallback</h1></section></template>` +
      `<main>Body</main>`;
    initEventEnds(document, NOW);
    expect(heroes()).toHaveLength(0);
    expect(document.querySelector("template")).not.toBeNull();
  });
});

const ACTIONS =
  `<span class="moss-event-upcoming">Upcoming</span>` +
  `<a class="moss-event-link moss-event-tickets" href="https://example.test/t">Tickets</a>` +
  `<a class="moss-event-link moss-event-online" href="https://example.test/o">Online</a>` +
  `<a class="moss-event-link moss-event-calendar" href="/e.ics">Add to calendar</a>`;

function events(ends: string) {
  document.body.innerHTML =
    `<span class="date moss-event-meta" data-when="upcoming" data-ends="${ends}">${ACTIONS}</span>` +
    `<div class="moss-card" data-when="upcoming" data-ends="${ends}"><h3>Card</h3>${ACTIONS}</div>` +
    `<li data-when="upcoming" data-ends="${ends}">Row${ACTIONS}</li>`;
}

const flipped = () => [...document.querySelectorAll("[data-when]")] as HTMLElement[];

describe("an event's meta line, card and row", () => {
  test("turn past at the end and drop Tickets, Add to calendar and the Upcoming mark", () => {
    events("2030-06-01T12:00");
    initEventEnds(document, NOW);
    expect(flipped()).toHaveLength(3);
    for (const el of flipped()) {
      expect(el.dataset.when).toBe("past");
      expect(el.hasAttribute("data-ends")).toBe(false);
      expect(el.querySelector(".moss-event-tickets, .moss-event-calendar, .moss-event-upcoming")).toBeNull();
      expect(el.querySelector(".moss-event-online")).not.toBeNull();
    }
  });

  test("stay as built a minute before the end", () => {
    events("2030-06-01T12:01");
    const before = document.body.innerHTML;
    initEventEnds(document, NOW);
    expect(document.body.innerHTML).toBe(before);
  });

  test("an unreadable end changes nothing", () => {
    events("soon");
    const before = document.body.innerHTML;
    initEventEnds(document, NOW);
    expect(document.body.innerHTML).toBe(before);
  });

  test("a past element the build left has no data-ends and is not touched", () => {
    document.body.innerHTML = `<span class="moss-event-meta" data-when="past"><a class="moss-event-link moss-event-online" href="/o">Online</a></span>`;
    const before = document.body.innerHTML;
    initEventEnds(document, NOW);
    expect(document.body.innerHTML).toBe(before);
  });
});
