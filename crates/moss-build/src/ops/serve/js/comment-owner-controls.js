// Owner-only comment Hide controls. Injected ONLY into the preview
// (shell-marker gated) by iframe_bridge.rs — NEVER present in deployed output.
(function () {
  if (window.__mossCommentOwnerControls) return;
  window.__mossCommentOwnerControls = true;

  function store() {
    try {
      var el = document.getElementById("moss-comments-data");
      return el ? JSON.parse(el.textContent || "{}") : {};
    } catch (_) { return {}; }
  }

  function ensureButton(li) {
    if (li.querySelector("[data-moss-hide-comment]")) return;
    var btn = document.createElement("button");
    btn.type = "button";
    btn.setAttribute("data-moss-hide-comment", "");
    btn.setAttribute("aria-label", "Hide comment");
    btn.textContent = "Hide";
    li.appendChild(btn);
  }

  function decorate() {
    var items = document.querySelectorAll("li.comment-item[data-comment-id]");
    for (var i = 0; i < items.length; i++) ensureButton(items[i]);
  }

  document.addEventListener("click", function (e) {
    var btn = e.target.closest && e.target.closest("[data-moss-hide-comment]");
    if (!btn) return;
    var li = btn.closest("li.comment-item[data-comment-id]");
    if (!li) return;
    e.preventDefault();
    var s = store();
    window.parent.postMessage({
      type: "moss-hide-comment",
      source: li.getAttribute("data-comment-source"),
      id: li.getAttribute("data-comment-id"),
      pageKey: s.pageKey || "",
      siteName: s.siteName || ""
    }, "*");
  });

  // Initial decoration pass — always runs.
  decorate();

  // Only attach the observer when a comment list is actually present.
  // On pages with no comments there is nothing dynamic to watch, so we skip
  // the body-subtree observer entirely to avoid firing on every DOM mutation.
  var commentList = document.querySelector(".comment-list");
  if (commentList) {
    var obs = new MutationObserver(function () {
      obs.disconnect();
      decorate();
      // Re-query in case the list itself was replaced by a morph.
      var list = document.querySelector(".comment-list");
      if (list) obs.observe(list, { childList: true, subtree: true });
    });
    obs.observe(commentList, { childList: true, subtree: true });
  }
})();
