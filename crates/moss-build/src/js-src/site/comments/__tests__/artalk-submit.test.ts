/**
 * Provider-level integration test for artalk: when a quote is open and the
 * user submits, the POST body's `content` field is prefixed with
 * `> {quote}\n\n`. This is the integration that would have caught the
 * original regression — quote-float opens but submit silently posts
 * unprefixed text. The contract test covers dispatch→listener; this covers
 * listener→submit.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import {
  installQuoteFloat,
  _resetQuoteFloatForTests,
} from "../quote-float";
import { dispatchQuoteComment } from "../quote-events";

/**
 * Mount the comment form fixture.
 *
 * @param artalkHighWaterId - when provided, embeds a `#moss-comments-data`
 *   JSON island with the given value (mirrors what the Rust bake emits). When
 *   omitted, no island is injected — artalk.ts falls back to -1 (no gate).
 */
function mountForm(artalkHighWaterId?: number): { form: HTMLFormElement; textarea: HTMLTextAreaElement } {
  const storeScript = artalkHighWaterId !== undefined
    ? `<script type="application/json" id="moss-comments-data">{"watermark":"2026-01-01T00:00:00Z","siteName":"smoke","pageKey":"abc12345","artalkHighWaterId":${artalkHighWaterId},"comments":[]}</script>`
    : "";
  document.body.innerHTML = `
    <section class="moss-comments" id="moss-comments">
      ${storeScript}
      <div class="comment-form-slot" id="default-form-slot">
        <form id="moss-comment-form"
              data-state="idle"
              data-server-url="http://example.test/"
              data-site-name="smoke"
              data-page-key="abc12345"
              data-page-title="A Post">
          <textarea id="moss-comment-text" placeholder="Say something"></textarea>
          <input name="name" value="Alice">
          <input name="email" value="alice@example.test">
          <input name="link" value="">
          <div class="moss-btn-slot">
            <button type="submit" class="moss-btn comment-form-submit" aria-busy="false">
              <span class="moss-btn__label">Reply</span>
              <span class="moss-btn__spinner" aria-hidden="true"></span>
              <span class="moss-btn__check" aria-hidden="true"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg></span>
            </button>
          </div>
          <div class="comment-form-status" id="moss-comment-status" aria-live="assertive" aria-atomic="true"></div>
        </form>
      </div>
      <ul class="comment-list"></ul>
    </section>
  `;
  return {
    form: document.getElementById("moss-comment-form") as HTMLFormElement,
    textarea: document.getElementById("moss-comment-text") as HTMLTextAreaElement,
  };
}

let fetchMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  _resetQuoteFloatForTests();
  vi.resetModules(); // re-execute the artalk IIFE against the fresh DOM each test
  document.body.innerHTML = "";
  fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    // A real Artalk POST returns the created comment with a server id; GETs read
    // `comments`/`count`. Including an id lets submit optimistically append the
    // comment (no extra revalidate fetch).
    json: async () => ({ comments: [], count: 0, id: 1, content_marked: "<p>ok</p>" }),
  });
  globalThis.fetch = fetchMock as unknown as typeof fetch;
});

afterEach(() => {
  document.body.innerHTML = "";
  window.history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});

describe("artalk submit + quote-float", () => {
  test("live loader fetches comments and renders newest first", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 1, content: "<p>older</p>", date: "2026-06-12 10:00:00", nick: "Old", rid: 0 },
          { id: 2, content: "<p>newer</p>", date: "2026-06-12 11:00:00", nick: "New", rid: 0 },
        ],
        count: 2,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const items = Array.from(document.querySelectorAll(".comment-item"));
    expect(items).toHaveLength(2);
    expect(items[0].querySelector(".comment-author")?.textContent).toBe("New");
    expect(items[1].querySelector(".comment-author")?.textContent).toBe("Old");
  });

  test("renderCommentItem emits data-comment-source and data-comment-id on each live <li>", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 99, content: "<p>test</p>", date: "2026-06-12 10:00:00", nick: "Tester", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const li = document.getElementById("comment-artalk-99") as HTMLElement | null;
    expect(li).not.toBeNull();
    expect(li!.dataset.commentSource).toBe("artalk");
    expect(li!.dataset.commentId).toBe("99");
  });

  test("revalidate appends new artalk comments without clobbering baked syndicated", async () => {
    mountForm();
    const list = document.querySelector(".comment-list") as HTMLElement;
    // Baked SSR list: a syndicated (matters) comment + one existing artalk comment.
    list.innerHTML = `
      <li class="comment-item" id="comment-matters-m1"><div class="comment-body">from matters</div></li>
      <li class="comment-item" id="comment-artalk-5"><div class="comment-body">existing artalk</div></li>
    `;
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 5, content: "<p>existing artalk</p>", date: "2026-06-12 10:00:00", nick: "A", rid: 0 },
          { id: 6, content: "<p>new artalk</p>", date: "2026-06-12 11:00:00", nick: "B", rid: 0 },
        ],
        count: 2,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // Syndicated comment preserved (the old full re-render clobbered it).
    expect(document.getElementById("comment-matters-m1")).not.toBeNull();
    // Existing artalk comment not duplicated.
    expect(document.querySelectorAll("#comment-artalk-5")).toHaveLength(1);
    // New artalk comment appended.
    expect(document.getElementById("comment-artalk-6")).not.toBeNull();
  });

  test("submit prepends '> {quote}\\n\\n' when quote shell is open", async () => {
    mountForm();
    window.history.pushState({}, "", "/totally-different-path/");
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    dispatchQuoteComment("the quoted passage");

    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "my reply text";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    const expectedPageKey = form.dataset.pageKey!;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.content).toBe("> the quoted passage\n\nmy reply text");
    expect(body.page_key).toBe(expectedPageKey);
    expect(body.page_key).not.toBe(window.location.pathname);
  });

  test("submit posts plain content when no quote is open", async () => {
    mountForm();
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "just a comment";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    const expectedPageKey = form.dataset.pageKey!;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.content).toBe("just a comment");
    expect(body.page_key).toBe(expectedPageKey);
  });

  test("multi-line quote prefixes '> ' per line in submit body", async () => {
    mountForm();
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    dispatchQuoteComment("line one\nline two\nline three");

    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "reply";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    const expectedPageKey = form.dataset.pageKey!;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.content).toBe(
      "> line one\n> line two\n> line three\n\nreply"
    );
    expect(body.page_key).toBe(expectedPageKey);
  });

  test("submit includes both name and nick", async () => {
    mountForm();
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "name check";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.name).toBe("Alice");
    expect(body.nick).toBe("Alice");
  });

  test("submit normalizes a bare website into a valid URL", async () => {
    mountForm();
    const link = document.querySelector('[name="link"]') as HTMLInputElement;
    link.value = "example.com";
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "link check";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.link).toBe("https://example.com/");
  });

  test("submit appends a newly created reply under its parent in the live store", async () => {
    mountForm();
    fetchMock = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({
          comments: [
            { id: 1, content: "<p>parent</p>", date: "2026-06-12 10:00:00", nick: "Parent", rid: 0 },
            { id: 2, content: "<p>older reply</p>", date: "2026-06-12 10:30:00", nick: "Older", rid: 1 },
          ],
          count: 2,
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ id: 3, rid: 1, content_marked: "<p>new reply</p>", date: "2026-06-12 11:00:00", nick: "Alice" }),
      });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    fetchMock.mockClear();

    const parentReplyBtn = document.querySelector('[data-reply-id="1"]') as HTMLButtonElement;
    parentReplyBtn.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement;
    textarea.value = "new reply";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    expect(document.querySelectorAll(".comment-item")).toHaveLength(3);
    expect(document.getElementById("comment-artalk-3")?.closest(".comment-replies")).not.toBeNull();
  });

  test("replies render newest-first within a thread (newest at top, oldest last)", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 1, content: "<p>parent</p>", date: "2026-06-12 10:00:00", nick: "Parent", rid: 0 },
          { id: 2, content: "<p>older reply</p>", date: "2026-06-12 10:30:00", nick: "Older", rid: 1 },
          { id: 3, content: "<p>newer reply</p>", date: "2026-06-12 11:00:00", nick: "Newer", rid: 1 },
        ],
        count: 3,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const replies = document.querySelector("#comment-artalk-1 .comment-replies");
    expect(replies).not.toBeNull();
    // Newest first: id 3 (11:00) above id 2 (10:30) — same logic as the top-level list.
    const order = Array.from(replies!.children).map(li => (li as HTMLElement).id);
    expect(order).toEqual(["comment-artalk-3", "comment-artalk-2"]);
  });

  test("reply box moves into the thread, then returns to the top-of-section home after sending", async () => {
    mountForm();
    fetchMock = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({
          comments: [{ id: 1, content: "<p>parent</p>", date: "2026-06-12 10:00:00", nick: "Parent", rid: 0 }],
          count: 1,
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ id: 5, rid: 1, content_marked: "<p>my reply</p>", date: "2026-06-12 11:00:00", nick: "Alice" }),
      });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));

    const slot = document.getElementById("default-form-slot")!;
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;

    // Clicking reply moves the FORM into the thread (not the defaultSlot home).
    (document.querySelector('[data-reply-id="1"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(form.parentElement).not.toBe(slot);
    expect(document.getElementById("comment-artalk-1")!.contains(form)).toBe(true);
    // The home slot itself never leaves its place at the top of the section.
    expect(slot.parentElement).toBe(document.getElementById("moss-comments"));

    // After sending, the form returns to its home slot at the top of the section.
    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "my reply";
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    expect(form.parentElement).toBe(slot);
    // The reply landed at the top of the parent's thread.
    expect(document.getElementById("comment-artalk-5")?.closest(".comment-replies")).not.toBeNull();
  });

});

describe("artalk high-water-id resurrection prevention", () => {
  // These tests verify the core fix: a fetched Artalk comment with id ≤ the
  // baked high-water id must NOT be appended even when it's not in the baked DOM
  // (i.e., it was hidden by moderation). A comment with id > high-water IS new
  // and must be appended.

  test("a fetched comment with id ≤ highWater that is not in the baked DOM is NOT appended (resurrection prevented)", async () => {
    // high-water = 10: the build knew about ids up to 10 at bake time.
    mountForm(10);
    // id 7 is ≤ 10 and absent from the DOM (the moderator hid it).
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 7, content: "<p>hidden comment</p>", date: "2026-01-01 00:00:00", nick: "Spammer", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    expect(document.getElementById("comment-artalk-7")).toBeNull();
  });

  test("a fetched comment with id > highWater IS appended (genuinely new comment)", async () => {
    // high-water = 10: comment id 11 was posted after the bake.
    mountForm(10);
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 11, content: "<p>new comment</p>", date: "2026-01-02 00:00:00", nick: "Reader", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    expect(document.getElementById("comment-artalk-11")).not.toBeNull();
  });

  test("a baked comment with id ≤ highWater that IS in the DOM is not duplicated", async () => {
    // Bake emitted id 5; high-water = 5; fetch returns id 5 again.
    mountForm(5);
    const list = document.querySelector(".comment-list") as HTMLElement;
    list.innerHTML = `<li class="comment-item" id="comment-artalk-5"><div class="comment-body">baked</div></li>`;
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 5, content: "<p>baked</p>", date: "2026-01-01 00:00:00", nick: "A", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // Exactly one — the baked one — not a duplicate.
    expect(document.querySelectorAll("#comment-artalk-5")).toHaveLength(1);
  });

  test("a baked comment absent from the live fetch is RETAINED (client is purely additive)", async () => {
    // The bake rendered id 3 (it was visible at build time). After the bake,
    // a moderator deletes it on the server — the live fetch no longer returns it.
    // The client must NOT remove it; the next bake will reflect the deletion.
    mountForm(10);
    const list = document.querySelector(".comment-list") as HTMLElement;
    list.innerHTML = `<li class="comment-item" id="comment-artalk-3"><div class="comment-body">baked visible</div></li>`;
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        // id 3 has been deleted server-side — it does not appear in the fetch.
        comments: [
          { id: 11, content: "<p>new comment</p>", date: "2026-01-02 00:00:00", nick: "Reader", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // Baked comment must still be present.
    expect(document.getElementById("comment-artalk-3")).not.toBeNull();
    // New comment is also appended.
    expect(document.getElementById("comment-artalk-11")).not.toBeNull();
  });

  test("when no store is embedded (missing island), falls back to id-not-in-DOM dedup only", async () => {
    // No artalkHighWaterId argument → no moss-comments-data island injected.
    // artalk.ts treats this as highWater = -1 (no gate).
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 3, content: "<p>old comment</p>", date: "2026-01-01 00:00:00", nick: "B", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // With no store, the gate is inactive — id 3 gets appended.
    expect(document.getElementById("comment-artalk-3")).not.toBeNull();
  });
});

describe("comment count agrees with the rendered list", () => {
  // The header count must equal the number of comments actually RENDERED (baked
  // SSR set + fetched comments that pass the high-water gate), never the server's
  // raw `data.count`. Hidden comments (moderated at build time) are never synced
  // to Artalk, so the server still counts them while the client's gate suppresses
  // them from the list — the count and list would disagree if driven by data.count.

  /**
   * Mount a comment section with a baked SSR list of `baked` artalk ids, a store
   * seeded with those same ids, and a header count span. The header count text is
   * initialised to the correct SSR value so the test proves revalidate does not
   * INFLATE it, not merely that it sets some value.
   */
  function mountWithBaked(highWater: number, baked: number[]): void {
    const storeComments = baked.map(id => ({
      id, source: "artalk", author: "Baked", content: "<p>c</p>", date: "2026-01-01 00:00:00",
    }));
    const store = {
      watermark: "2026-01-01T00:00:00Z", siteName: "smoke", pageKey: "abc12345",
      artalkHighWaterId: highWater, comments: storeComments,
    };
    const items = baked.map(id =>
      `<li class="comment-item" id="comment-artalk-${id}" data-comment-source="artalk" data-comment-id="${id}"><div class="comment-body">baked ${id}</div></li>`
    ).join("\n");
    document.body.innerHTML = `
      <section class="moss-comments" id="moss-comments">
        <details open>
          <summary class="comments-toggle"><span>${baked.length} comments</span></summary>
          <script type="application/json" id="moss-comments-data">${JSON.stringify(store)}</script>
          <div class="comment-form-slot" id="default-form-slot">
            <form id="moss-comment-form" data-state="idle"
                  data-server-url="http://example.test/" data-site-name="smoke"
                  data-page-key="abc12345" data-page-title="A Post">
              <textarea id="moss-comment-text" placeholder="Say something"></textarea>
              <input name="name" value="Alice"><input name="email" value="a@b.test"><input name="link" value="">
              <div class="moss-btn-slot"><button type="submit" class="moss-btn comment-form-submit" aria-busy="false"><span class="moss-btn__label">Reply</span><span class="moss-btn__spinner" aria-hidden="true"></span><span class="moss-btn__check" aria-hidden="true"></span></button></div>
              <div class="comment-form-status" id="moss-comment-status" aria-live="assertive" aria-atomic="true"></div>
            </form>
          </div>
          <ol class="comment-list">${items}</ol>
        </details>
      </section>`;
  }

  const countText = (): string | null =>
    document.querySelector(".comments-toggle span")!.textContent;

  test("a higher server data.count (build-time-hidden comments ≤ high-water) does NOT leak into the header — count stays at the rendered size", async () => {
    // Baked: 3 visible comments (ids 1,2,3). high-water = 5 → the build knew ids
    // up to 5. The server still returns count=5 and surfaces the 2 hidden ids
    // (4,5 ≤ high-water) in its list — the gate must drop them from BOTH.
    mountWithBaked(5, [1, 2, 3]);
    fetchMock = vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({
        comments: [
          { id: 1, content: "<p>c1</p>", date: "2026-01-01 00:00:00", nick: "A", rid: 0 },
          { id: 2, content: "<p>c2</p>", date: "2026-01-01 00:00:00", nick: "B", rid: 0 },
          { id: 3, content: "<p>c3</p>", date: "2026-01-01 00:00:00", nick: "C", rid: 0 },
          { id: 4, content: "<p>hidden</p>", date: "2026-01-01 00:00:00", nick: "H1", rid: 0 },
          { id: 5, content: "<p>hidden</p>", date: "2026-01-01 00:00:00", nick: "H2", rid: 0 },
        ],
        count: 5,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // Hidden ids 4,5 are gated out of the list…
    expect(document.getElementById("comment-artalk-4")).toBeNull();
    expect(document.getElementById("comment-artalk-5")).toBeNull();
    const rendered = document.querySelectorAll(".comment-item").length;
    expect(rendered).toBe(3);
    // …and the header count matches the rendered size, NOT the server's 5.
    expect(countText()).toBe("3 comments");
  });

  test("a genuinely new comment (id > high-water) increments count in step with the list; a noisy server total is ignored", async () => {
    mountWithBaked(5, [1, 2, 3]);
    fetchMock = vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({
        comments: [
          { id: 6, content: "<p>new</p>", date: "2026-01-02 00:00:00", nick: "New", rid: 0 },
        ],
        count: 99, // server total is noise — the client counts what it renders
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    expect(document.getElementById("comment-artalk-6")).not.toBeNull();
    expect(document.querySelectorAll(".comment-item").length).toBe(4);
    expect(countText()).toBe("4 comments");
  });

  test("baked-only (revalidate returns exactly the baked set): count stays at the baked size", async () => {
    mountWithBaked(3, [1, 2, 3]);
    fetchMock = vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({
        comments: [
          { id: 1, content: "<p>c1</p>", date: "2026-01-01 00:00:00", nick: "A", rid: 0 },
          { id: 2, content: "<p>c2</p>", date: "2026-01-01 00:00:00", nick: "B", rid: 0 },
          { id: 3, content: "<p>c3</p>", date: "2026-01-01 00:00:00", nick: "C", rid: 0 },
        ],
        count: 3,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    expect(document.querySelectorAll(".comment-item").length).toBe(3);
    expect(countText()).toBe("3 comments");
  });
});

describe("artalk captcha handshake (Artalk iframe flow)", () => {
  test("403 need_captcha → opens Artalk's captcha iframe, polls status, retries, appends", async () => {
    vi.useFakeTimers();
    mountForm();
    let postCount = 0;
    fetchMock = vi.fn((url: string, opts?: { method?: string }) => {
      const u = String(url);
      if (u.includes("/api/v2/captcha/status")) {
        return Promise.resolve({ ok: true, status: 200, json: async () => ({ is_pass: true }) });
      }
      if (u.includes("/api/v2/comments") && opts?.method === "POST") {
        postCount += 1;
        // First attempt is challenged; after verify the retry succeeds.
        return postCount === 1
          ? Promise.resolve({ ok: false, status: 403, json: async () => ({ need_captcha: true, iframe: true }) })
          : Promise.resolve({ ok: true, status: 200, json: async () => ({ id: 42, content_marked: "<p>hi</p>", nick: "Alice" }) });
      }
      return Promise.resolve({ ok: true, status: 200, json: async () => ({ comments: [], count: 0 }) });
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await vi.advanceTimersByTimeAsync(0); // run the initial revalidate

    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "hi";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.advanceTimersByTimeAsync(0); // first POST → 403 → captcha modal opens

    // The modal iframe targets the captcha page WITH a trailing slash — required
    // so Artalk's relative ./verify resolves under the /comments proxy prefix.
    const frame = document.querySelector('iframe[title="Verification"]') as HTMLIFrameElement | null;
    expect(frame).not.toBeNull();
    expect(frame!.src).toContain("/api/v2/captcha/?t=");

    // Poll fires after 1.5s → is_pass → retry POST → comment appended, modal gone.
    await vi.advanceTimersByTimeAsync(1600);

    expect(postCount).toBe(2);
    expect(document.getElementById("comment-artalk-42")).not.toBeNull();
    expect(document.querySelector('iframe[title="Verification"]')).toBeNull();
    vi.useRealTimers();
  });

  test("dismissing the captcha modal cancels quietly (no error, submit re-enabled)", async () => {
    vi.useFakeTimers();
    mountForm();
    fetchMock = vi.fn((url: string, opts?: { method?: string }) => {
      const u = String(url);
      if (u.includes("/api/v2/comments") && opts?.method === "POST") {
        return Promise.resolve({ ok: false, status: 403, json: async () => ({ need_captcha: true, iframe: true }) });
      }
      return Promise.resolve({ ok: true, status: 200, json: async () => ({ comments: [], count: 0 }) });
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await vi.advanceTimersByTimeAsync(0);

    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "hi";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.advanceTimersByTimeAsync(0);

    const overlay = document.querySelector('[role="dialog"]') as HTMLElement | null;
    expect(overlay).not.toBeNull();
    overlay!.dispatchEvent(new MouseEvent("click", { bubbles: true })); // click the backdrop
    await vi.advanceTimersByTimeAsync(0);

    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.getElementById("moss-comment-status")!.textContent).toBe("");
    expect((form.querySelector('[type="submit"]') as HTMLButtonElement).disabled).toBe(false);
    vi.useRealTimers();
  });

  test("Escape dismisses the captcha modal (keyboard parity with the backdrop click)", async () => {
    vi.useFakeTimers();
    mountForm();
    fetchMock = vi.fn((url: string, opts?: { method?: string }) => {
      const u = String(url);
      if (u.includes("/api/v2/comments") && opts?.method === "POST") {
        return Promise.resolve({ ok: false, status: 403, json: async () => ({ need_captcha: true, iframe: true }) });
      }
      return Promise.resolve({ ok: true, status: 200, json: async () => ({ comments: [], count: 0 }) });
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await vi.advanceTimersByTimeAsync(0);

    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "hi";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.advanceTimersByTimeAsync(0);

    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await vi.advanceTimersByTimeAsync(0);

    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect((form.querySelector('[type="submit"]') as HTMLButtonElement).disabled).toBe(false);
    vi.useRealTimers();
  });

  test("double-submit while loading fires only one POST (loading guard)", async () => {
    vi.useFakeTimers();
    mountForm();
    let postCount = 0;
    fetchMock = vi.fn((url: string, opts?: { method?: string }) => {
      const u = String(url);
      if (u.includes("/api/v2/comments") && opts?.method === "POST") {
        postCount += 1;
        return new Promise(() => {}); // never resolves → form stays in "loading"
      }
      return Promise.resolve({ ok: true, status: 200, json: async () => ({ comments: [], count: 0 }) });
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await vi.advanceTimersByTimeAsync(0);

    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "hi";
    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.advanceTimersByTimeAsync(0);
    // Second submit while the first POST is still in flight must be ignored.
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.advanceTimersByTimeAsync(0);

    expect(postCount).toBe(1);
    vi.useRealTimers();
  });
});

describe("artalk live-comment author link + reply-arrow text presentation", () => {
  // The baked Rust renderer already wraps the author name in an <a href="website">
  // (render.rs is_safe_http_url path). The live renderer must match it so a
  // comment newer than the last deploy (fetched, not baked) is rendered the same.

  test("live comment with a website renders the author name as a link to that site", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 50, content: "<p>hi</p>", date: "2026-06-22 17:50:05", nick: "扳布", link: "https://bambooo.top/", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const author = document.querySelector("#comment-artalk-50 .comment-author") as HTMLElement | null;
    expect(author).not.toBeNull();
    expect(author!.tagName).toBe("A");
    expect(author!.getAttribute("href")).toBe("https://bambooo.top/");
    // ugc marks it as a user-generated link, matching the body-link sanitizer.
    expect(author!.getAttribute("rel")).toBe("nofollow ugc noopener");
    expect(author!.getAttribute("target")).toBe("_blank");
    expect(author!.textContent).toBe("扳布");
  });

  test("live comment without a website renders the author name as a plain span", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 51, content: "<p>hi</p>", date: "2026-06-22 17:50:05", nick: "NoSite", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const author = document.querySelector("#comment-artalk-51 .comment-author") as HTMLElement | null;
    expect(author).not.toBeNull();
    expect(author!.tagName).toBe("SPAN");
    expect(author!.textContent).toBe("NoSite");
  });

  test("live comment with an unsafe (non-http) website does not become a link", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 52, content: "<p>hi</p>", date: "2026-06-22 17:50:05", nick: "Sneaky", link: "javascript:alert(1)", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const author = document.querySelector("#comment-artalk-52 .comment-author") as HTMLElement | null;
    expect(author).not.toBeNull();
    expect(author!.tagName).toBe("SPAN");
    const header = document.querySelector("#comment-artalk-52 .comment-header") as HTMLElement;
    expect(header.innerHTML).not.toContain("javascript:");
  });

  test("reply button uses the text-presentation arrow (U+21A9 U+FE0E), not the bare emoji-default form", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        comments: [
          { id: 53, content: "<p>hi</p>", date: "2026-06-22 17:50:05", nick: "R", rid: 0 },
        ],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const btn = document.querySelector("#comment-artalk-53 .comment-reply-btn") as HTMLElement | null;
    expect(btn).not.toBeNull();
    expect(btn!.textContent).toContain("↩︎");
  });
});

describe("reply input box nests under the target comment (indent + left line)", () => {
  // Design intention: the reply INPUT form should be nested/indented under the
  // comment it replies to — a left gap plus a left vertical line — visually
  // matching how a posted reply is DISPLAYED (the `.comment-replies` thread
  // indent). The class `.comment-form--reply` carries that indent CSS. The
  // top-level compose box (in its home slot) stays full-width (no class).

  test("opening reply nests the form under the target comment and tags it .comment-form--reply; the top-level box has no tag", async () => {
    mountForm();
    fetchMock = vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({
        comments: [{ id: 1, content: "<p>parent</p>", date: "2026-06-12 10:00:00", nick: "Parent", rid: 0 }],
        count: 1,
      }),
    });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    const slot = document.getElementById("default-form-slot")!;

    // Top-level compose box: full-width home slot, NOT tagged as a reply.
    expect(form.parentElement).toBe(slot);
    expect(form.classList.contains("comment-form--reply")).toBe(false);

    // Click reply → the form nests inside the target comment AND is tagged so the
    // indent + left vertical line apply (mirroring the reply-thread display).
    (document.querySelector('[data-reply-id="1"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(document.getElementById("comment-artalk-1")!.contains(form)).toBe(true);
    expect(form.classList.contains("comment-form--reply")).toBe(true);
  });

  test("the reply tag is cleared when the form returns to its full-width home after sending", async () => {
    mountForm();
    fetchMock = vi.fn()
      .mockResolvedValueOnce({
        ok: true, status: 200,
        json: async () => ({
          comments: [{ id: 1, content: "<p>parent</p>", date: "2026-06-12 10:00:00", nick: "Parent", rid: 0 }],
          count: 1,
        }),
      })
      .mockResolvedValueOnce({
        ok: true, status: 200,
        json: async () => ({ id: 5, rid: 1, content_marked: "<p>my reply</p>", date: "2026-06-12 11:00:00", nick: "Alice" }),
      });
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    await import("../artalk");
    await new Promise(r => setTimeout(r, 0));

    const form = document.getElementById("moss-comment-form") as HTMLFormElement;
    const slot = document.getElementById("default-form-slot")!;
    (document.querySelector('[data-reply-id="1"]') as HTMLButtonElement)
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(form.classList.contains("comment-form--reply")).toBe(true);

    (document.getElementById("moss-comment-text") as HTMLTextAreaElement).value = "my reply";
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise(r => setTimeout(r, 0));

    // Home again, full-width — the reply tag is gone.
    expect(form.parentElement).toBe(slot);
    expect(form.classList.contains("comment-form--reply")).toBe(false);
  });
});
