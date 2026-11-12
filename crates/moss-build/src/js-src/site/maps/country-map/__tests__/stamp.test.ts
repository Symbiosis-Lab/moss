/** `as_of: today` rewrites the stamp and the panels' copies to the visitor's day, in the stamp's language. */
import { describe, expect, test } from "vitest";
import { stampToday } from "../stamp";

const figure = (lang: string, today: string | null, stampLang = ""): HTMLElement => {
  document.documentElement.lang = lang;
  const attr = (today === null ? "" : ` data-today="${today}"`) + (stampLang ? ` lang="${stampLang}"` : "");
  document.body.innerHTML = `<figure><p class="moss-country-stamp"${attr}><time datetime="2026-11-03">built</time></p><template data-slug="japan"><p class="moss-country-stamp">built</p></template></figure>`;
  return document.querySelector("figure")!;
};
const time = () => document.querySelector("time")!;
const panelStamp = () => document.querySelector("template")!.content.querySelector(".moss-country-stamp")!.textContent;
const DAY = new Date(2030, 4, 20, 23, 30);

describe("as-of stamp", () => {
  test("shows the visitor's local day and the panels say the same", () => {
    stampToday(figure("en", "As of {date}"), DAY);
    expect(time().textContent).toBe("As of May 20, 2030");
    expect(time().dateTime).toBe("2030-05-20");
    expect(panelStamp()).toBe("As of May 20, 2030");
  });

  test("writes the date in the page's language", () => {
    stampToday(figure("de", "Stand: {date}"), DAY);
    expect(time().textContent).toBe("Stand: 20. Mai 2030");
  });

  test("writes the date in the stamp's language where the page's differs", () => {
    stampToday(figure("ja", "As of {date}", "en"), DAY);
    expect(time().textContent).toBe("As of May 20, 2030");
  });

  test("keeps the built date when the language tag is malformed", () => {
    stampToday(figure("en", "As of {date}", "en_US"), DAY);
    expect(time().textContent).toBe("built");
    expect(time().dateTime).toBe("2026-11-03");
  });

  test("leaves a stamp without as_of: today alone", () => {
    stampToday(figure("en", null), DAY);
    expect(time().textContent).toBe("built");
    expect(time().dateTime).toBe("2026-11-03");
    expect(panelStamp()).toBe("built");
  });
});
