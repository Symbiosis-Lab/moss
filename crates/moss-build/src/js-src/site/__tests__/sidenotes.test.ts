import { describe, expect, it } from "vitest";
import { initSidenotes } from "../sidenotes";

// Scope: what jsdom can actually see, which is the DOM the script BUILDS —
// not where any of it lands. Placement, the sticky sheet, the drag and the
// dock are engine questions, proven in
// tests/render-gates/site/footnote-target.spec.ts.
//
// The clone assertions cover the defects the earlier id-namespacing design
// actually shipped: a clone whose root id was never rewritten, and a
// back-link pointing at an id that existed nowhere.

/** The endnote markup `footnotes.rs` emits, for one note. */
function pageWithOneNote(): void {
  document.body.innerHTML = `
    <p>Body<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1"><a href="#fn-1" role="doc-noteref">1</a></sup>.</p>
    <section class="moss-footnotes" role="doc-endnotes"><ol>
      <li id="fn-1" tabindex="-1"><p>The note. <a class="moss-footnote-backref" href="#fnref-1">&#8617;</a></p></li>
    </ol></section>`;
}


describe("initSidenotes", () => {
  it("puts a copy of the note's text right after its marker, described from it", () => {
    pageWithOneNote();
    initSidenotes();

    const marker = document.getElementById("fnref-1")!;
    const aside = marker.nextElementSibling as HTMLElement;
    expect(aside.tagName).toBe("ASIDE");
    expect(aside.classList.contains("moss-sidenote")).toBe(true);
    expect(aside.textContent).toContain("The note.");
    expect(aside.querySelector(".moss-sidenote-number")?.textContent).toBe("1");
    // The design's marker-driven pairing: the note is the marker's
    // description, announced on focus; focus itself never moves.
    expect(marker.querySelector("a")!.getAttribute("aria-describedby")).toBe(
      aside.id,
    );
  });

  it("leaves no note id anywhere in the copy", () => {
    // A duplicate id breaks every `#fn-N` jump on the page — the navigation
    // the endnote section exists for. Asserted as "no fn ids at all" rather
    // than "no id equal to fn-1", because the failure that shipped was a
    // `querySelectorAll('[id]')` that could not match the element it was
    // called on. The aside's own generated id is fresh, so it collides with
    // nothing.
    pageWithOneNote();
    initSidenotes();

    const aside = document.querySelector<HTMLElement>(".moss-sidenote")!;
    expect(aside.querySelectorAll("[id]")).toHaveLength(0);
    expect(aside.id).toMatch(/^moss-sidenote-/);
    expect(document.querySelectorAll("#fn-1")).toHaveLength(1);
  });

  it("drops the return arrow, which belongs to the endnote list", () => {
    pageWithOneNote();
    initSidenotes();

    expect(
      document.querySelectorAll(".moss-sidenote .moss-footnote-backref"),
    ).toHaveLength(0);
    expect(
      document.querySelectorAll(".moss-footnotes .moss-footnote-backref"),
    ).toHaveLength(1);
  });

  it("ignores a marker whose note is not on the page", () => {
    document.body.innerHTML = `<p>Body<sup class="moss-footnote-ref" id="fnref-9"><a href="#fn-9">9</a></sup>.</p>`;
    initSidenotes();
    expect(document.querySelectorAll(".moss-sidenote")).toHaveLength(0);
  });

  it("carries no aria-hidden: which copy is accessible is CSS's call", () => {
    // Exactly one presentation of a note is rendered at a time — margin,
    // peek sheet, or the list — so exactly one is in the accessibility
    // tree. An aria-hidden here would leave the margin presentation with
    // no accessible note at all.
    pageWithOneNote();
    initSidenotes();
    expect(
      document.querySelector(".moss-sidenote")!.hasAttribute("aria-hidden"),
    ).toBe(false);
  });

  it("retires the endnote section only when every note was cloned", () => {
    // The class is the licence to hide the list at widths where the margin
    // shows, so it must assert what it claims: a note with no surviving
    // copy — here, one with no marker at all — keeps the section an
    // ordinary part of the page.
    pageWithOneNote();
    initSidenotes();
    expect(
      document
        .querySelector(".moss-footnotes")!
        .classList.contains("moss-footnotes-cloned"),
    ).toBe(true);

    document.body.innerHTML = `
      <p>Body<sup class="moss-footnote-ref" id="fnref-1"><a href="#fn-1">1</a></sup>.</p>
      <section class="moss-footnotes" role="doc-endnotes"><ol>
        <li id="fn-1" tabindex="-1"><p>Cited.</p></li>
        <li id="fn-2" tabindex="-1"><p>Never cited.</p></li>
      </ol></section>`;
    initSidenotes();
    expect(
      document
        .querySelector(".moss-footnotes")!
        .classList.contains("moss-footnotes-cloned"),
    ).toBe(false);
  });

  it("gives the section its sheet anatomy: heading, grabber, scroll wrapper", () => {
    // All JS-built (the pure renderer has no language for the heading), and
    // in FLOW from init — the docked list and the lifted sheet must be the
    // same pixels, so nothing may appear only on lift.
    pageWithOneNote();
    initSidenotes();
    const section = document.querySelector(".moss-footnotes")!;
    const title = section.querySelector(".moss-footnotes-title")!;
    expect(title.textContent).toBe("Notes");
    const scroller = section.querySelector(".moss-footnote-scroller")!;
    expect(scroller.contains(title)).toBe(true);
    expect(scroller.querySelector("ol")).not.toBeNull();
    const grab = section.querySelector("button.moss-footnote-grab")!;
    expect(grab.getAttribute("aria-label")).toBe("All notes");
  });
});
