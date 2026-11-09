"use strict";
// A build's run returns while a promise chain it started is still waiting on
// the host; the chain writes into the folder once the person's run of the
// same plugin is going. The person's run then writes its own file.
var DetachedWriter = (() => ({
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    if (ctx.trigger === "background") {
      (async () => {
        await invoke("__test_slow__", { ms: 400 });
        await invoke("write_project_file", { relativePath: "notes/late.md", data: "# late" });
      })().catch(() => {});
    } else {
      await invoke("__test_slow__", { ms: 900 });
      await invoke("write_project_file", { relativePath: "notes/person.md", data: "# person" });
    }
    return { success: true };
  },
}))();
