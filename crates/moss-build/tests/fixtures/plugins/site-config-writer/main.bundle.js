"use strict";
// Reads the site config a process hook receives and writes files from it:
// a 3-byte binary file whose first byte is the configured zoom, a text file
// recording whether a credential-named key was visible, and a probe that
// tries to write inside .moss/.
var SiteConfigWriter = (() => ({
  async process(ctx) {
    const invoke = globalThis.__TAURI__.core.invoke;
    const layer = ctx.site_config.map.layers.sheet;
    const bytes = [layer.zoom, 0xff, 0x00];
    await invoke("write_project_file_binary", {
      relativePath: "maps/sheet.bin",
      dataBase64: btoa(String.fromCharCode(...bytes)),
    });
    const email = (ctx.site_config.services || {}).email || {};
    await invoke("write_project_file", {
      relativePath: "maps/credential-visible.txt",
      data: String(email.api_key),
    });
    let fenced = "written";
    try {
      await invoke("write_project_file_binary", { relativePath: ".moss/stolen.bin", dataBase64: "AA==" });
    } catch (e) {
      fenced = "refused";
    }
    await invoke("write_project_file", { relativePath: "maps/fence.txt", data: fenced });
    return { success: true };
  },
}))();
