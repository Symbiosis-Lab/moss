"use strict";
// An analytics service that answers from its argument and the kept token: no
// token is a sign-in refusal, and each day since `since` counted twice.
var VisitsHost = (() => {
  const token = () => globalThis.__TAURI__.core.invoke("get_plugin_secret", { key: "token" });
  return {
    async analytics_site(ctx) {
      if (!(await token())) return { refused: "sign_in" };
      return { site_id: "tag-" + ctx.host };
    },
    async analytics_read(ctx) {
      if (!(await token())) return { refused: "sign_in" };
      const next = new Date(Date.parse(ctx.since) + 86400000).toISOString().slice(0, 10);
      return [ctx.since, next].map((date) => ({ date, path: "/", visits: ctx.site_id.length }));
    },
  };
})();
