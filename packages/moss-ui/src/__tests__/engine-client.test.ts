import { describe, expect, it } from "vitest";
import { engineClient, type EngineCommand } from "../index.ts";

function recording(answer: (cmd: EngineCommand) => Promise<unknown>) {
  const sent: [EngineCommand, Record<string, unknown>][] = [];
  const client = engineClient((cmd, args) => {
    sent.push([cmd, args]);
    return answer(cmd);
  });
  return { client, sent };
}

describe("engineClient", () => {
  it("sends positional arguments under the engine's own names", async () => {
    const { client, sent } = recording(async () => ({ status: "saved" }));
    await client.saveEditorContent("a.md", { title: "A" }, "body", "h");
    await client.moveEntries(["a.md"], "posts");
    await client.listVaultTerms();
    expect(sent).toEqual([
      ["save_editor_content", { filePath: "a.md", frontmatter: { title: "A" }, body: "body", expectedHash: "h", keepDisplaced: undefined }],
      ["move_entries", { paths: ["a.md"], targetDir: "posts" }],
      ["list_vault_terms", {}],
    ]);
  });

  it("answers an engine error string as an error result and lets a transport fault through", async () => {
    const refused = recording(() => Promise.reject("No project open"));
    expect(await refused.client.pageFacts("a.md")).toEqual({ status: "error", error: "No project open" });
    const broken = recording(() => Promise.reject(new Error("socket closed")));
    await expect(broken.client.pageFacts("a.md")).rejects.toThrow("socket closed");
  });

  it("answers the command's value", async () => {
    const { client } = recording(async () => ({ position: "page", auto: {} }));
    const facts = await client.pageFacts("a.md");
    expect(facts).toEqual({ status: "ok", data: { position: "page", auto: {} } });
    if (facts.status === "ok") {
      // @ts-expect-error a position is one of the build's words
      facts.data.position = "nowhere";
    }
  });
});
