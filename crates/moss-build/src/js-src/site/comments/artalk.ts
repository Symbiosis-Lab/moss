import { installQuoteFloat } from "./quote-float";
import { widgetCopy } from "./i18n";
import { langBucket } from "../subscribe/i18n";

interface LiveComment {
  id: number | string;
  content: string;
  date?: string;
  nick?: string;
  link?: string;
  rid?: number | string;
}

(function () {
  "use strict";

  const form = document.getElementById("moss-comment-form") as HTMLFormElement | null;
  if (!form) return;

  const copy = widgetCopy();

  // Read serverUrl lazily at submit time (not cached here) so that the
  // preview-shim injected at end-of-body can rewrite data-server-url and
  // have it take effect for the first submit. siteName/pageKey/pageTitle
  // are stable metadata that don't need lazy reads.
  const siteName = form.dataset.siteName!;
  const pageKey = form.dataset.pageKey!;
  const pageTitle = form.dataset.pageTitle!;
  const status = document.getElementById("moss-comment-status")!;
  const textarea = document.getElementById("moss-comment-text") as HTMLTextAreaElement | null;
  const defaultSlot = document.getElementById("default-form-slot");
  const commentsList = document.querySelector(".comment-list") as HTMLOListElement | null;
  const commentsSection = document.getElementById("moss-comments");
  const STORAGE_KEY = "moss-commenter";
  const initialPlaceholder = textarea?.placeholder ?? "";
  let replyToId: string | null = null;

  // Parse the dehydrated comment store baked into the page ONCE. It carries the
  // high-water id AND the full set of comments the SSR list was rendered from
  // (post-moderation — exactly what the page shows on first paint). Returns null
  // when the island is absent or malformed, so downstream reads fall back to
  // pre-fix behavior (no gate, DOM-derived seed).
  const commentStore: Record<string, unknown> | null = (() => {
    try {
      const el = document.getElementById("moss-comments-data");
      if (!el) return null;
      return JSON.parse(el.textContent || "{}") as Record<string, unknown>;
    } catch {
      return null;
    }
  })();

  // The high-water Artalk comment id. A fetched Artalk comment with id ≤ this was
  // already known at build time (hidden ones included) and must NOT be re-appended
  // — that's the resurrection bug. A comment with id > this is genuinely new
  // (posted after the bake) and is appended normally. Fall back to -1 (no gate —
  // append anything not already in DOM) when the store is absent or malformed.
  const artalkHighWaterId: number = (() => {
    const hw = commentStore?.["artalkHighWaterId"];
    return typeof hw === "number" ? hw : -1;
  })();

  // The RESOLVED comment set — the single source of truth for BOTH the rendered
  // list and the displayed count. Seeded from the baked store (all sources,
  // post-moderation → exactly the SSR list), then grown by appendComment as
  // fetched/posted Artalk comments pass the high-water gate. The count is ALWAYS
  // resolvedKeys.size, NEVER the server's raw `data.count`: hidden comments are
  // never synced to Artalk (hide is local/build-time), so Artalk still counts
  // them while the gate suppresses them from the list — driving the count off the
  // server total would show more than are rendered. Identity key = "{source}:{id}"
  // so a fetched Artalk comment dedups against its baked twin. When the store is
  // absent, seed from the baked DOM so count and list still agree.
  const resolvedKeys = new Set<string>();
  (() => {
    const baked = Array.isArray(commentStore?.["comments"])
      ? (commentStore!["comments"] as unknown[])
      : null;
    if (baked) {
      for (const raw of baked) {
        if (!raw || typeof raw !== "object") continue;
        const c = raw as Record<string, unknown>;
        if (c.id === undefined || c.id === null) continue;
        const source = typeof c.source === "string" ? c.source : "";
        resolvedKeys.add(source + ":" + String(c.id));
      }
      return;
    }
    // No store → seed from the baked DOM (data-comment-source/-id on each <li>).
    document.querySelectorAll(".comment-item[data-comment-id]").forEach(el => {
      const li = el as HTMLElement;
      const id = li.dataset.commentId || "";
      if (id) resolvedKeys.add((li.dataset.commentSource || "") + ":" + id);
    });
  })();

  function cancelReply(): void {
    replyToId = null;
    if (textarea) textarea.placeholder = initialPlaceholder;
    // Drop the nested-reply styling so the form is full-width again at its home.
    form.classList.remove("comment-form--reply");
    if (defaultSlot && form.parentElement !== defaultSlot) {
      defaultSlot.appendChild(form);
    }
  }

  const quoteFloat = textarea && defaultSlot
    ? installQuoteFloat({ form, textarea, defaultSlot, onBeforeOpen: cancelReply })
    : null;

  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
    if (saved.name) (form.querySelector('[name="name"]') as HTMLInputElement).value = saved.name;
    if (saved.email) (form.querySelector('[name="email"]') as HTMLInputElement).value = saved.email;
    if (saved.link) (form.querySelector('[name="link"]') as HTMLInputElement).value = saved.link;
  } catch { /* ignore */ }

  if (textarea) {
    textarea.addEventListener("input", function (this: HTMLTextAreaElement) {
      this.style.height = "auto";
      this.style.height = this.scrollHeight + "px";
    });
  }

  // Delegated reply-button listener on the stable `#moss-comments` section.
  // Per-button binding at init (forEach) would leave dead buttons on comments
  // added by idiomorph morph-refresh (the morph inserts new DOM nodes that
  // were never seen at init time). One delegated listener on a stable ancestor
  // survives morphs: idiomorph keeps the `<section id="moss-comments">` node
  // as a matched structural element, so this listener persists across refreshes.
  if (commentsSection) {
    commentsSection.addEventListener("click", function (e: Event) {
      const btn = (e.target as Element).closest(".comment-reply-btn") as HTMLButtonElement | null;
      if (!btn) return;
      // Close any open quote-float so the slot we move is defaultSlot,
      // not .comment-float-inner with the form's grandparent dragged along.
      quoteFloat?.cancel();
      const id = btn.dataset.replyId!;
      const name = btn.dataset.replyName!;
      replyToId = id;
      // Move the FORM into the thread — NOT defaultSlot, which is the form's stable
      // home at the top of the section (moving the home would leave nothing to return
      // to). Place the form at the TOP of the thread, above existing replies, mirroring
      // the newest-first layout. cancelReply()/submit then return the form to defaultSlot.
      // Rendered DOM ids are namespaced per source (comment-{source}-{id}); this client only anchors artalk comments.
      const item = document.getElementById("comment-artalk-" + id);
      if (item) {
        const repliesSlot = item.querySelector(":scope > .comment-replies");
        if (repliesSlot) {
          item.insertBefore(form, repliesSlot);
        } else {
          item.appendChild(form);
        }
        // Nest the reply input under this comment with the same left gap +
        // vertical line as a displayed reply thread. cancelReply() removes it
        // when the form returns to its full-width home.
        form.classList.add("comment-form--reply");
      }
      if (textarea) {
        textarea.placeholder = "@" + name + " ";
        textarea.focus();
      }
    });
  }

  async function readServerError(response: Response): Promise<string | null> {
    const contentType = response.headers.get("content-type") || "";
    if (contentType.includes("application/json")) {
      try {
        const data = await response.json() as Record<string, unknown>;
        if (typeof data.msg === "string" && data.msg.trim()) return data.msg.trim();
        if (typeof data.message === "string" && data.message.trim()) return data.message.trim();
      } catch { /* ignore */ }
    }
    try {
      const text = (await response.text()).trim();
      if (text) return text;
    } catch { /* ignore */ }
    return null;
  }

  function normalizeOptionalLink(raw: string): string {
    const value = raw.trim();
    if (!value) return "";
    let url: URL;
    try {
      url = new URL(value);
    } catch {
      try {
        url = new URL(`https://${value}`);
      } catch {
        return "";
      }
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") return "";
    return url.toString();
  }

  function escapeHtml(raw: string): string {
    return raw
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  // Allowlist must stay byte-for-byte equivalent to ALLOWED_TAGS in sanitize.rs.
  const LIVE_ALLOWED_TAGS = new Set(["p", "a", "em", "strong", "code", "blockquote", "br", "ul", "ol", "li"]);

  function sanitizeAttrs(el: Element): void {
    const tag = el.tagName.toLowerCase();
    const keep: [string, string][] = [];
    if (tag === "a") {
      const href = el.getAttribute("href") ?? "";
      try {
        const parsed = new URL(href);
        if (parsed.protocol === "http:" || parsed.protocol === "https:") {
          keep.push(["href", href]);
        }
      } catch { /* drop non-URL or non-http(s) hrefs */ }
      keep.push(["rel", "nofollow ugc noopener"]);
      keep.push(["target", "_blank"]);
    }
    while (el.attributes.length > 0) el.removeAttribute(el.attributes[0].name);
    for (const [k, v] of keep) el.setAttribute(k, v);
  }

  function sanitizeNode(parent: Node): void {
    for (const child of Array.from(parent.childNodes)) {
      if (child.nodeType === Node.COMMENT_NODE ||
          child.nodeType === Node.PROCESSING_INSTRUCTION_NODE) {
        parent.removeChild(child);
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        const el = child as Element;
        sanitizeNode(el); // depth-first so inner disallowed elements are unwrapped first
        if (!LIVE_ALLOWED_TAGS.has(el.tagName.toLowerCase())) {
          while (el.firstChild) parent.insertBefore(el.firstChild, el);
          parent.removeChild(el);
        } else {
          sanitizeAttrs(el);
        }
      }
      // TEXT_NODE: leave untouched
    }
  }

  function sanitizeLiveHtml(html: string): string {
    const tpl = document.createElement("template");
    tpl.innerHTML = html;
    sanitizeNode(tpl.content);
    const wrap = document.createElement("div");
    wrap.appendChild(tpl.content);
    return wrap.innerHTML;
  }

  function formatDate(dateStr?: string): string {
    if (!dateStr) return copy.justNow;
    const clean = dateStr.replace("T", " ");
    const parts = clean.split(/[- ]/);
    if (parts.length < 3) return dateStr;

    const year = parts[0];
    const month = Number.parseInt(parts[1], 10) || 1;
    const day = Number.parseInt(parts[2], 10) || 1;
    const lang = langBucket(document.documentElement.lang);
    if (lang === "zh-hans" || lang === "zh-hant") {
      return `${year}年${month}月${day}日`;
    }

    const monthName = [
      "???", "Jan", "Feb", "Mar", "Apr", "May", "Jun",
      "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][month] ?? "???";
    return `${monthName} ${day}, ${year}`;
  }

  function formatCommentCount(count: number): string {
    const lang = langBucket(document.documentElement.lang);
    if (lang === "zh-hans") {
      if (count === 0) return "评论";
      if (count === 1) return "1条评论";
      return `${count}条评论`;
    }
    if (lang === "zh-hant") {
      if (count === 0) return "留言";
      if (count === 1) return "1則留言";
      return `${count}則留言`;
    }
    if (count === 0) return "Comments";
    if (count === 1) return "1 comment";
    return `${count} comments`;
  }

  function replyButtonLabel(): string {
    const lang = langBucket(document.documentElement.lang);
    if (lang === "zh-hant") return "回覆";
    if (lang === "en") return "Reply";
    return "回复";
  }

  function updateCommentCount(count: number): void {
    const summaryCount = document.querySelector(".comments-toggle span");
    if (summaryCount) summaryCount.textContent = formatCommentCount(count);
  }

  function normalizeComment(raw: Record<string, unknown>): LiveComment | null {
    const id = raw.id;
    if (id === undefined || id === null) return null;
    const contentMarked = typeof raw.content_marked === "string"
      ? raw.content_marked
      : typeof raw.content === "string"
        ? raw.content
        : "";
    if (!contentMarked) return null;
    return {
      id,
      content: sanitizeLiveHtml(contentMarked),
      date: typeof raw.date === "string" ? raw.date : undefined,
      nick: typeof raw.nick === "string" ? raw.nick : undefined,
      link: typeof raw.link === "string" ? raw.link : undefined,
      rid: typeof raw.rid === "number" || typeof raw.rid === "string" ? raw.rid : undefined,
    };
  }


  function renderCommentItem(comment: LiveComment, indent: number): string {
    const pad = " ".repeat(indent);
    const id = String(comment.id);
    const author = escapeHtml(comment.nick?.trim() || copy.anonymous);
    const date = escapeHtml(formatDate(comment.date));
    const body = comment.content || "";

    // Author name links to the commenter's website when they left a safe
    // http(s) one — matches the baked Rust renderer's author <a> (render.rs), so
    // a comment newer than the last deploy renders the same as a baked one. No
    // website → plain span. Email is never linked: Artalk hashes it, and a public
    // mailto would expose every commenter's address. normalizeOptionalLink returns
    // "" for empty/unsafe values (same gate the submit path uses); note it also
    // upgrades a bare domain to https, where the baked is_safe_http_url requires a
    // full scheme — a benign delta that only ever makes a link MORE clickable.
    // rel="nofollow ugc noopener" mirrors the body-link sanitizer (sanitizeAttrs).
    const safeUrl = normalizeOptionalLink(comment.link ?? "");
    const authorHtml = safeUrl
      ? `<a href="${escapeHtml(safeUrl)}" class="comment-author" rel="nofollow ugc noopener" target="_blank">${author}</a>`
      : `<span class="comment-author">${author}</span>`;

    // "↩︎": U+21A9 + VARIATION SELECTOR-15 (U+FE0E) forces text presentation, so
    // iOS/Android render a plain monochrome ↩ rather than a colored emoji.
    let html = `${pad}<li class="comment-item" id="comment-artalk-${escapeHtml(id)}" `
      + `data-comment-source="artalk" data-comment-id="${escapeHtml(id)}">\n`;
    html += `${pad}  <div class="comment-header">\n`;
    html += `${pad}    ${authorHtml}\n`;
    html += `${pad}    <time class="comment-date" datetime="${escapeHtml(comment.date || "")}">${date}</time>\n`;
    html += `${pad}    <button type="button" class="comment-reply-btn" data-reply-id="${escapeHtml(id)}" data-reply-name="${author}">↩︎ ${escapeHtml(replyButtonLabel())}</button>\n`;
    html += `${pad}  </div>\n`;
    html += `${pad}  <div class="comment-body">${body}</div>\n`;
    html += `${pad}</li>\n`;
    return html;
  }

  // Append-only reconcile: the baked SSR list (all sources,
  // Rust-rendered + sanitized at ingest) is the archive and is AUTHORITATIVE.
  // The live fetch ONLY ADDS Artalk comments newer than the bake — it never
  // removes baked comments. Moderation (hide/delete) is applied at build time
  // and reflected in the next bake; the client does not reflect live server-side
  // deletions. Syndicated (matters/douban) comments are left untouched —
  // this is why we don't clobber them by clearing the list.

  function bakedArtalkIds(): Set<string> {
    const ids = new Set<string>();
    document.querySelectorAll('.comment-item[id^="comment-artalk-"]').forEach(el => {
      ids.add(el.id.slice("comment-artalk-".length));
    });
    return ids;
  }

  // Render one Artalk comment <li> and insert it newest-first: a reply goes at the
  // TOP of its parent's .comment-replies, a top-level comment at the top of the list
  // — matching the baked SSR order (newest first, oldest last). Idempotent (skips ids
  // already in the DOM).
  //
  // High-water gate: if artalkHighWaterId ≥ 0, reject any fetched Artalk comment
  // whose id ≤ artalkHighWaterId that is not already in the baked DOM. This prevents
  // resurrection of hidden comments: the build computed the max id over the FULL
  // synced set (before moderation), so any comment the moderator hid has an id
  // ≤ high-water and is silently dropped here. Newly posted comments (id > high-water)
  // pass through and are appended as usual. The explicit DOM-id check still runs first
  // so baked-visible comments are never duplicated.
  function appendComment(c: LiveComment, bypassHighWaterGate = false): void {
    if (!commentsList) return;
    if (document.getElementById("comment-artalk-" + String(c.id))) return;
    // Apply the high-water gate only when the gate is active (≥ 0) and the
    // caller has not explicitly bypassed it (optimistic post-submit path).
    if (!bypassHighWaterGate && artalkHighWaterId >= 0 && Number(c.id) <= artalkHighWaterId) return;
    // Past the gate → this comment WILL be rendered. Record it in the resolved
    // set so the count (resolvedKeys.size) tracks the list one-for-one. Set-add
    // is idempotent, so a re-seen id never double-counts.
    resolvedKeys.add("artalk:" + String(c.id));
    const html = renderCommentItem(c, 6);
    const parentId = Number(c.rid ?? 0) > 0 ? String(c.rid) : "";
    if (parentId) {
      const parent = document.getElementById("comment-artalk-" + parentId);
      if (parent) {
        let slot = parent.querySelector(".comment-replies");
        if (!slot) {
          slot = document.createElement("ol");
          slot.className = "comment-replies";
          parent.appendChild(slot);
        }
        slot.insertAdjacentHTML("afterbegin", html);
        return;
      }
      // Parent not in the DOM (only the reply is new) → fall through to top-level.
    }
    commentsList.insertAdjacentHTML("afterbegin", html);
  }

  // Purely additive reconcile: append fetched comments not already in the DOM,
  // then update the count display. The baked SSR snapshot is authoritative for
  // all comments up to the high-water id — the client NEVER removes a baked
  // comment. Moderation is applied at bake time; the next build reflects it.
  function reconcile(fetched: LiveComment[]): void {
    if (!commentsList) return;
    const present = bakedArtalkIds();
    // Append new comments, parents before replies (Artalk ids are sequential).
    const fresh = fetched
      .filter(c => !present.has(String(c.id)))
      .sort((a, b) => Number(String(a.id)) - Number(String(b.id)));
    for (const c of fresh) appendComment(c);
    // Count == the resolved set the list was built from, NEVER the server's raw
    // `total` (data.count): the server still counts build-time-hidden comments
    // the high-water gate suppresses from the list, so the total would overcount.
    updateCommentCount(resolvedKeys.size);
  }

  async function revalidate(): Promise<void> {
    const serverUrl = form.dataset.serverUrl?.replace(/\/+$/, "") || "";
    if (!serverUrl || !commentsList) return;
    const url = serverUrl +
      "/api/v2/comments?limit=1000&offset=0&flat_mode=true&page_key=" +
      encodeURIComponent(pageKey) +
      "&site_name=" +
      encodeURIComponent(siteName);
    try {
      const response = await fetch(url, { method: "GET", headers: { Accept: "application/json" } });
      if (!response.ok) return;
      const data = await response.json() as Record<string, unknown>;
      const rawComments = Array.isArray(data.comments) ? data.comments : [];
      const fetched = rawComments
        .map((raw): LiveComment | null =>
          raw && typeof raw === "object" ? normalizeComment(raw as Record<string, unknown>) : null)
        .filter((comment): comment is LiveComment => comment !== null);
      // NB: `data.count` is intentionally ignored — see reconcile()/resolvedKeys.
      reconcile(fetched);
    } catch {
      // Keep the baked archive on any failure.
    }
  }

  // Stale-while-revalidate: the baked SSR shows instantly; refresh after any
  // preview-time server-url rewrite has run.
  window.setTimeout(() => {
    void revalidate();
  }, 0);

  // Solve Artalk's captcha challenge: open its captcha page in a modal iframe and
  // resolve once the server reports the visitor passed. The iframe (served from
  // the comment-server origin) renders Turnstile and POSTs the token to its own
  // relative `./verify` — the TRAILING SLASH on the src is required so that
  // ./verify resolves under the /comments proxy prefix
  // (…/comments/api/v2/captcha/verify). moss never sees the site_key or token; we
  // only poll /captcha/status. (Artalk's X-Frame headers are dropped by the seta
  // proxy, so cross-origin framing from the blog page works.) Verification is
  // keyed on the real client IP, which the seta proxy now forwards.
  function solveCaptcha(server: string): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const overlay = document.createElement("div");
      overlay.setAttribute("role", "dialog");
      overlay.setAttribute("aria-modal", "true");
      overlay.setAttribute("aria-label", "Verification");
      overlay.style.cssText =
        "position:fixed;inset:0;z-index:2147483646;display:flex;align-items:center;" +
        "justify-content:center;background:rgba(0,0,0,.5)";
      const frame = document.createElement("iframe");
      frame.title = "Verification";
      frame.src = server + "/api/v2/captcha/?t=" + Date.now();
      frame.style.cssText =
        "width:340px;max-width:92vw;height:240px;border:0;border-radius:12px;" +
        "background:var(--moss-color-surface,#fff);box-shadow:var(--moss-elevation-3)";
      overlay.appendChild(frame);
      document.body.appendChild(overlay);

      let settled = false;
      const finish = (act: () => void): void => {
        if (settled) return;
        settled = true;
        window.clearInterval(poll);
        window.clearTimeout(timer);
        overlay.removeEventListener("click", onClick);
        document.removeEventListener("keydown", onKey);
        overlay.remove();
        act();
      };
      const cancel = (): void => finish(() => reject(new Error("cancelled")));
      const onClick = (ev: Event): void => {
        if (ev.target === overlay) cancel();
      };
      // Escape dismisses the modal (keyboard parity with the backdrop click).
      const onKey = (ev: KeyboardEvent): void => {
        if (ev.key === "Escape") cancel();
      };
      overlay.addEventListener("click", onClick);
      document.addEventListener("keydown", onKey);

      const poll = window.setInterval(() => {
        void fetch(server + "/api/v2/captcha/status", { headers: { Accept: "application/json" } })
          .then(r => (r.ok ? r.json() : null))
          .then((d: Record<string, unknown> | null) => {
            if (d && d.is_pass === true) finish(resolve);
          })
          .catch(() => { /* transient — keep polling */ });
      }, 1500);
      const timer = window.setTimeout(
        () => finish(() => reject(new Error("captcha timeout"))),
        120000,
      );
    });
  }

  type FormState = "idle" | "loading" | "success" | "error";

  const btn = form.querySelector<HTMLButtonElement>('[type="submit"]')!;

  // Move the form's data-state and the button's aria-busy/disabled together.
  // Also clears the status line on non-error transitions so errors are
  // never stale when the form returns to idle or loading.
  function setState(s: FormState): void {
    form.dataset.state = s;
    btn.setAttribute("aria-busy", s === "loading" ? "true" : "false");
    btn.disabled = s === "loading";
    if (s !== "error") {
      status.textContent = "";
      status.classList.remove("comment-form-status--error");
    }
  }

  // Freeze the button-slot width once so the surrounding grid never sees the
  // button resize during state transitions. Safe to call repeatedly — the
  // guard on slot.style.width makes it a no-op after the first measurement.
  function lockSlotWidth(): void {
    const slot = btn.parentElement;
    if (!slot || !slot.classList.contains("moss-btn-slot") || slot.style.width) return;
    const w = btn.offsetWidth;
    if (w > 0) slot.style.width = `${w}px`;
  }

  // Auto-revert from success back to idle after 3s.
  let revertTimer: number | null = null;
  function scheduleAutoRevert(): void {
    if (revertTimer !== null) clearTimeout(revertTimer);
    revertTimer = window.setTimeout(() => {
      setState("idle");
      revertTimer = null;
    }, 3000);
  }

  form.addEventListener("submit", function (e: Event) {
    e.preventDefault();
    // Guard against double-submit: ignore re-entry while a POST (or the captcha
    // modal) is in flight. Mirrors the subscribe form's loading guard — without
    // it, pressing Enter in a text field during loading fires a second POST
    // (duplicate comment) and can stack a second captcha modal.
    if (form.dataset.state === "loading") return;
    if (!textarea) return;
    const content = textarea.value.trim();
    if (!content) return;

    const nameVal = (form.querySelector('[name="name"]') as HTMLInputElement).value.trim();
    const emailVal = (form.querySelector('[name="email"]') as HTMLInputElement).value.trim();
    const linkVal = (form.querySelector('[name="link"]') as HTMLInputElement).value.trim();
    const normalizedLink = normalizeOptionalLink(linkVal);

    if (!nameVal) {
      status.textContent = copy.nameRequired;
      status.classList.add("comment-form-status--error");
      setState("error");
      return;
    }

    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ name: nameVal, email: emailVal, link: normalizedLink }));
    } catch { /* ignore */ }

    const quotePrefix = quoteFloat?.getQuotePrefix() ?? "";
    const finalContent = quotePrefix + content;

    lockSlotWidth();
    setState("loading");

    const body: Record<string, unknown> = {
      content: finalContent,
      name: nameVal,
      nick: nameVal,
      email: emailVal,
      link: normalizedLink,
      page_key: pageKey,
      page_title: pageTitle,
      site_name: siteName,
    };
    if (replyToId) body.rid = parseInt(replyToId, 10);

    const serverUrl = form.dataset.serverUrl!.replace(/\/+$/, "");

    // POST the comment, transparently solving Artalk's captcha if the server
    // demands one. Artalk replies 403 { need_captcha:true, iframe:true } when a
    // (Turnstile) challenge is required; solveCaptcha() runs Artalk's own captcha
    // iframe, then we retry. This whole branch is inert until captcha is enabled
    // server-side — otherwise the first POST returns 200 directly.
    async function submitComment(): Promise<Record<string, unknown>> {
      const post = () => fetch(serverUrl + "/api/v2/comments", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      let r = await post();
      if (r.status === 403) {
        const info = await r.json().catch(() => ({})) as Record<string, unknown>;
        if (info.need_captcha) {
          // Verifying stays on loading state — the button spinner keeps spinning
          // while the captcha modal is open; no separate status line needed.
          await solveCaptcha(serverUrl);
          r = await post();
        } else {
          throw new Error((typeof info.msg === "string" && info.msg) || "HTTP 403");
        }
      }
      if (!r.ok) throw new Error(await readServerError(r) || ("HTTP " + r.status));
      return await r.json() as Record<string, unknown>;
    }

    submitComment()
      .then((data: Record<string, unknown>) => {
        // Success: morph button to ✓, auto-revert after 3s.
        setState("success");
        scheduleAutoRevert();
        textarea!.value = "";
        // Capture before resetting — the rid fallback below reads it.
        const capturedReplyToId = replyToId;
        // Return the form to its home slot at the top of the section. quoteFloat.cancel()
        // closes the float shell when one is open; cancelReply() re-homes the form for the
        // reply case (the shell is NOT open then, so cancel() alone no-ops and would strand
        // the form inside the thread) and clears the reply placeholder + replyToId.
        quoteFloat?.cancel();
        cancelReply();

        const created = normalizeComment({
          id: typeof data.id === "number" || typeof data.id === "string" ? data.id : Date.now(),
          content_marked: typeof data.content_marked === "string" && data.content_marked.trim()
            ? data.content_marked
            : typeof data.content === "string" && data.content.trim()
              ? data.content
              : escapeHtml(finalContent),
          date: typeof data.date === "string" ? data.date : new Date().toISOString(),
          nick: typeof data.nick === "string" && data.nick.trim() ? data.nick : nameVal,
          rid: typeof data.rid === "number" || typeof data.rid === "string" ? data.rid : capturedReplyToId ?? undefined,
        });
        // Optimistically show the new comment ONLY if the server returned a real
        // id (so a later revalidate dedups it by id). Without a real id, just
        // revalidate to fetch it — a Date.now() placeholder would never match on
        // refetch and would duplicate. (distributed-correctness review.)
        // bypassHighWaterGate=true: a freshly posted comment's real server id is
        // always > high-water (it was just created), so gating here would be wrong
        // and confusing. The DOM-id dedup check still runs inside appendComment.
        if (created && (typeof data.id === "number" || typeof data.id === "string")) {
          appendComment(created, /* bypassHighWaterGate */ true);
          // The optimistic append grew the resolved set — reflect it in the count
          // now (a later revalidate dedups by id, so the count stays correct).
          updateCommentCount(resolvedKeys.size);
        } else {
          void revalidate();
        }
      })
      .catch((err: Error) => {
        // A visitor who dismisses the captcha modal resets quietly to idle.
        if (err.message === "cancelled") {
          setState("idle");
        } else {
          status.textContent = copy.errorPrefix + err.message;
          status.classList.add("comment-form-status--error");
          setState("error");
        }
      })
      .finally(() => {
        // setState("loading") already disables the button; setState("success"|"error"|"idle")
        // re-enables it. The finally only matters if an unexpected throw bypasses the
        // catch — belt-and-suspenders re-enable so the form is never permanently stuck.
        if (form.dataset.state === "loading") setState("idle");
      });
  });
})();
