import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { installSelectionSync } from "../source-target";

let posted: unknown[] = [];

beforeEach(() => {
  vi.useFakeTimers();
  posted = [];
  document.body.innerHTML = `
    <p data-source-line="3">The beds are covered.</p>
    <div data-source-range="5-9"><span id="in-range">Inside the hero</span></div>
    <nav data-source-none><a id="chrome">Home</a></nav>`;
  vi.spyOn(window.parent, "postMessage").mockImplementation((m: unknown) => void posted.push(m));
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  window.getSelection()?.removeAllRanges();
});

// One listener for the file; each test leaves no timer pending.
beforeAll(() => installSelectionSync(window));

function select(el: Element): void {
  const range = document.createRange();
  range.selectNodeContents(el);
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  // jsdom delivers selectionchange on a timer.
  vi.advanceTimersByTime(0);
}

describe("selection sync", () => {
  it("posts the selected words with their line even when the selection collapses before the post", () => {
    select(document.querySelector("p")!);
    // WKWebView collapses the selection on mouse-up, with no selectionchange the handler could act on.
    vi.spyOn(window, "getSelection").mockReturnValue({ isCollapsed: true, rangeCount: 0, toString: () => "" } as unknown as Selection);
    vi.advanceTimersByTime(50);
    expect(posted).toEqual([
      { type: "moss-source-target", target: { kind: "body-line", line: 3, text: "The beds are covered." }, interaction: "select" },
    ]);
  });

  it("a selection that collapses while the user is still dragging posts nothing", () => {
    select(document.querySelector("p")!);
    window.getSelection()!.removeAllRanges();
    vi.advanceTimersByTime(50);
    expect(posted).toEqual([]);
  });

  it("a selection of only spaces, or one long enough to be a copy, posts nothing", () => {
    document.body.insertAdjacentHTML("beforeend", `<p id="blank" data-source-line="11">   </p><p id="long" data-source-line="13">${"word ".repeat(120)}</p>`);
    select(document.getElementById("blank")!);
    vi.advanceTimersByTime(50);
    select(document.getElementById("long")!);
    vi.advanceTimersByTime(50);
    expect(posted).toEqual([]);
  });

  it("names the range around the words, and posts nothing for template chrome", () => {
    select(document.getElementById("in-range")!);
    vi.advanceTimersByTime(50);
    select(document.getElementById("chrome")!);
    vi.advanceTimersByTime(50);
    expect(posted).toEqual([
      { type: "moss-source-target", target: { kind: "body-range", startLine: 5, endLine: 9, text: "Inside the hero" }, interaction: "select" },
    ]);
  });
});
