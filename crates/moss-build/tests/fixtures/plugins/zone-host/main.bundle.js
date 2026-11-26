"use strict";
// A DNS host whose verbs answer from their argument. dns_zones and dns_set
// park for a moment, so a test can start a second run while the first is going.
var ZoneHost = (() => ({
  async dns_zones(ctx) {
    await globalThis.__TAURI__.core.invoke("__test_slow__", { ms: 300 });
    return [{ id: "z1", name: ctx.name || "example.org" }];
  },
  async dns_records(ctx) {
    return { refused: "missing_permission" };
  },
  async dns_set(ctx) {
    await globalThis.__TAURI__.core.invoke("__test_slow__", { ms: 300 });
    return ctx.records;
  },
}))();
