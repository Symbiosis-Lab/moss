"use strict";
// Writes the comments a page shows into its own social-data file, as a sync
// plugin does with what it fetched. The text stands in for the server's answer:
// MOSS_MATTERS_TEST_PROFILE when set, which no folder file and no recorded
// build input shows. It writes only when the text changed, so builds settle.
var CommentWriter = (() => ({
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    const text = (await invoke("get_plugin_env_var", { name: "MOSS_MATTERS_TEST_PROFILE" })) || "First reader comment";
    const comment = { id: "c1", content: text, createdAt: "2026-01-02", author: { displayName: "Reader" } };
    const data = JSON.stringify({ schemaVersion: "1.1.0", articles: { ab12cd34: { comments: [comment] } } });
    const relativePath = ".moss/data/social/comment-writer.json";
    const old = await invoke("read_project_file", { relativePath }).catch(() => null);
    if (old !== data) await invoke("write_project_file", { relativePath, data });
    return { success: true };
  },
}))();
