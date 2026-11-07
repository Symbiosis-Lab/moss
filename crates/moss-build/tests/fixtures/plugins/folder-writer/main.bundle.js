"use strict";
// Tries each way a hook can write into the folder and records, in its own
// storage, whether each was allowed or the error it got. Always returns an
// offer; moss reads it only from a run a build started. A build's run first
// parks for a moment, so a test can start the person's run while it is going.
var FolderWriter = (() => ({
  async listing() {
    const invoke = globalThis.__TAURI__.core.invoke;
    try {
      await invoke("write_project_file", { relativePath: "notes/listing.md", data: "# l" });
      return { html: "ok" };
    } catch (e) {
      return { html: String(e && e.message ? e.message : e) };
    }
  },
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    if (ctx.trigger === "background") await invoke("__test_slow__", { ms: 400 });
    const outcome = {};
    const attempt = async (name, cmd, args) => {
      try {
        await invoke(cmd, args);
        outcome[name] = "ok";
      } catch (e) {
        outcome[name] = String(e && e.message ? e.message : e);
      }
    };
    await attempt("page", "write_project_file", { relativePath: "notes/a.md", data: "# a" });
    await attempt("bytes", "write_project_file_binary", { relativePath: "notes/b.bin", dataBase64: "AA==" });
    await attempt("comments", "write_project_file", { relativePath: ".moss/data/social/folder-writer.json", data: "{}" });
    await attempt("download", "download_asset", { url: "http://127.0.0.1:9/a.jpg", targetDir: "assets", timeoutMs: 2000 });
    await attempt("program", "execute_binary", { binaryPath: "git", args: ["status"], timeoutMs: 2000 });
    await invoke("write_plugin_file", { relativePath: "outcome.json", content: JSON.stringify(outcome) });
    return { success: true, offer: { title: "2 new notes" } };
  },
}))();
