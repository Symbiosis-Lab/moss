"use strict";
// Writes its own state under `.moss/plugins/stamp-writer/` on every run, with
// bytes that differ each time, as a sync plugin's cursor does.
var StampWriter = (() => ({
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    const stamp = `${Date.now()}-${Math.random()}`;
    await invoke("write_plugin_file", { relativePath: "state.json", content: JSON.stringify({ stamp }) });
    return { success: true };
  },
}))();
