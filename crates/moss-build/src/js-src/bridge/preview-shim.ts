// Preview-only comment shim, injected by the preview server (iframe_bridge).
// Never present in the published artifact. Reroutes the comment form to the
// local stub and labels the section so the user knows submissions stay local.
//
// PRIMARY safety mechanism: the server-side rewrite in iframe_bridge.rs
// (rewrite_comment_form_server_url) ensures that every served HTML response
// already carries data-server-url="/__moss/comments" BEFORE any script runs.
// This is critical for the idiomorph in-place morph path: morphs fetch fresh
// HTML and sync attributes from served bytes onto the live DOM — so the served
// bytes must never carry the production URL, or one watch-rebuild reverts the
// form. The "local preview" hint is now a preview-server CSS hover tooltip
// injected by iframe_bridge, not a hint div.
//
// This shim is DEFENSE-IN-DEPTH for the attribute rewrite (handles any path
// that bypasses the server-side transform). The "local preview" hint is now a
// hover tooltip injected as preview-only CSS by iframe_bridge — not here.
//
// Timing: this script runs at end-of-body, after the artalk client script.
// artalk.ts reads serverUrl lazily at submit time (form.dataset.serverUrl
// inside the submit handler), so rewriting the data attribute here is
// sufficient — no caching race.

const form = document.getElementById("moss-comment-form") as HTMLFormElement | null;
if (form) {
  // Reroute the comment form to the local preview stub. Defense-in-depth for the
  // server-side rewrite (rewrite_comment_form_server_url) on any path that
  // bypasses the serve-time transform. The "local preview" hint is now a
  // hover tooltip injected as preview-only CSS by iframe_bridge — not here.
  form.dataset.serverUrl = "/__moss/comments";
}
