// E2E against a REAL Obsidian (wdio-obsidian-service). Proves the three
// things unit tests cannot: the plugin actually loads, the frontmatter
// diagnostics reach real editor pixels, and the preview command spawns the
// CLI (shim), parses its output, and shows the served page in the pane.
import { browser, expect } from "@wdio/globals";
import { obsidianPage } from "wdio-obsidian-service";

describe("moss plugin", () => {
  it("loads in the vault", async () => {
    const loaded = await browser.executeObsidian(({ app }) => {
      type WithPlugins = { plugins: { plugins: Record<string, unknown> } };
      return Boolean((app as unknown as WithPlugins).plugins.plugins["moss"]);
    });
    expect(loaded).toBe(true);
  });

  it("shows frontmatter diagnostics in the editor", async () => {
    await obsidianPage.openFile("bad-frontmatter.md");
    // Force source mode so the raw frontmatter text (and its decorations)
    // is what renders.
    await browser.executeObsidian(({ app }) => {
      const leaf = app.workspace.getMostRecentLeaf();
      if (!leaf) return;
      const state = leaf.getViewState();
      state.state = { ...state.state, mode: "source", source: true };
      void leaf.setViewState(state);
    });

    const unknown = browser.$(".moss-fm-unknown");
    await unknown.waitForExist({ timeout: 10000 });
    expect(await unknown.getText()).toBe("titel");

    const wrongType = browser.$(".moss-fm-wrong-type");
    await wrongType.waitForExist({ timeout: 10000 });
    expect(await wrongType.getText()).toContain("yes");

    // The clean note gets no decorations. Scope to the ACTIVE leaf —
    // Obsidian keeps the previous tab's DOM alive in the background.
    await obsidianPage.openFile("welcome.md");
    await browser.waitUntil(
      async () =>
        !(await browser.$(".workspace-leaf.mod-active .moss-fm-unknown").isExisting()) &&
        (await browser.$(".workspace-leaf.mod-active .cm-content").getText()).includes("Welcome"),
      { timeout: 10000, timeoutMsg: "diagnostics lingered in the active editor" },
    );
  });

  it("runs Preview site: spawns the CLI, parses the URL, shows the page", async () => {
    await browser.executeObsidianCommand("moss:preview");

    const iframe = browser.$("iframe.moss-preview-iframe");
    await iframe.waitForExist({ timeout: 30000 });
    await browser.waitUntil(
      async () => /^http:\/\/localhost:\d+/.test((await iframe.getAttribute("src")) ?? ""),
      { timeout: 30000, timeoutMsg: "iframe src never pointed at the shim server" },
    );
    const src = await iframe.getAttribute("src");
    expect(src).toMatch(/^http:\/\/localhost:\d+/);

    // Prove real bytes rendered, not just an attribute: enter the frame and
    // read the served page.
    await browser.switchFrame(iframe);
    const heading = browser.$("h1");
    await heading.waitForExist({ timeout: 15000 });
    expect(await heading.getText()).toBe("Hello from moss shim");
    await browser.switchFrame(null);

    // Status bar reflects the running server (the plugin's own item).
    const status = browser.$(".status-bar-item.plugin-moss");
    await status.waitForExist({ timeout: 10000 });
    expect(await status.getText()).toContain("serving");

    await browser.executeObsidianCommand("moss:stop-preview");
    let lastStatus = "";
    try {
      await browser.waitUntil(async () => {
        lastStatus = await browser.$(".status-bar-item.plugin-moss").getText();
        return lastStatus.includes("moss: idle");
      }, { timeout: 15000 });
    } catch {
      throw new Error(`status bar never returned to idle (last: "${lastStatus}")`);
    }
  });
});
