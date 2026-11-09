"use strict";
// A build's run leaves a timer that writes into the folder after the hook has
// returned; the person's run is still going when it fires, and then writes
// its own file.
var LateWriter = (() => ({
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    if (ctx.trigger === "background") {
      setTimeout(() => {
        invoke("write_project_file", { relativePath: "notes/late.md", data: "# late" }).catch(() => {});
      }, 300);
    } else {
      await invoke("__test_slow__", { ms: 900 });
      await invoke("write_project_file", { relativePath: "notes/person.md", data: "# person" });
    }
    return { success: true };
  },
}))();
