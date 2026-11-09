import { browser, expect } from "@wdio/globals";
import { obsidianPage } from "wdio-obsidian-service";

describe("moss on mobile", () => {
  it("loads in the mobile UI", async () => {
    expect((await obsidianPage.getPlatform()).isMobile).toBe(true);
    const loaded = await browser.executeObsidian(({ app }) => {
      type WithPlugins = { plugins: { plugins: Record<string, unknown> } };
      return Boolean((app as unknown as WithPlugins).plugins.plugins.moss);
    });
    expect(loaded).toBe(true);
  });

  it("decorates invalid frontmatter", async () => {
    await obsidianPage.openFile("bad-frontmatter.md");
    await browser.executeObsidian(async ({ app }) => {
      const leaf = app.workspace.getMostRecentLeaf();
      if (!leaf) throw new Error("No markdown leaf");
      const state = leaf.getViewState();
      state.state = { ...state.state, mode: "source", source: true };
      await leaf.setViewState(state);
    });
    const hint = browser.$(".moss-diag-hint");
    await hint.waitForExist({ timeout: 10000 });
    expect(await hint.getText()).toBe("titel");
    expect(await browser.$("span.moss-diag-error").getText()).toBe("draft");
  });

  it("previews the current page text in the side pane", async () => {
    await obsidianPage.openFile("welcome.md");
    await browser.executeObsidian(({ app }) => {
      const editor = app.workspace.activeEditor?.editor;
      if (!editor) throw new Error("No active markdown editor");
      editor.setValue("# Mobile page preview\n\nText from the current editor.");
    });
    await browser.executeObsidianCommand("moss:preview-page");
    const iframe = browser.$("iframe.moss-page-preview-iframe");
    await iframe.waitForExist({ timeout: 15000 });
    await browser.waitUntil(async () => (await iframe.getAttribute("srcdoc") ?? "").includes("Text from the current editor."));
    expect(await iframe.getAttribute("sandbox")).toBe("");
    await browser.switchFrame(iframe);
    expect(await browser.$("body").getText()).toContain("Text from the current editor.");
    await browser.switchFrame(null);
  });
});
