import { describe, it, expect, afterEach } from "vitest";
import { getSecret, rejectSecret, setSecret } from "../keystore.js";
import { setupMockTauri, type MockTauriContext } from "../../testing/index.js";

/**
 * The mock is what plugin authors write their tests against, so it has to mean
 * the same thing the host does — the cookie mock that "cleared" when the host
 * did not is why this file exists. Two properties carry that: no plugin id
 * crosses the wire (the host takes it from the dispatch seam, so a mock that
 * accepted one would let a wrong test pass), and rejecting erases.
 */
describe("secrets", () => {
  let ctx: MockTauriContext;

  afterEach(() => ctx?.cleanup());

  it("reads back what the user's credential modal stored", async () => {
    ctx = setupMockTauri({ pluginName: "ipfs" });
    ctx.secretStorage.seed("ipfs", "pinata_jwt", "tok-1");
    expect(await getSecret("pinata_jwt")).toBe("tok-1");
  });

  it("answers null before the user has been asked", async () => {
    ctx = setupMockTauri({ pluginName: "ipfs" });
    expect(await getSecret("pinata_jwt")).toBeNull();
  });

  it("rejecting re-asks and resolves with the replacement", async () => {
    ctx = setupMockTauri({ pluginName: "ipfs" });
    ctx.secretStorage.seed("ipfs", "pinata_jwt", "revoked");
    ctx.secretStorage.answerNextPrompt("tok-2");

    const fresh = await rejectSecret("pinata_jwt", { detail: "Pinata said no." });

    expect(fresh).toBe("tok-2");
    expect(ctx.secretStorage.wasRejected("ipfs", "pinata_jwt")).toBe(true);
    expect(await getSecret("pinata_jwt")).toBe("tok-2");
  });

  it("a cancelled re-prompt answers null and leaves nothing stored", async () => {
    ctx = setupMockTauri({ pluginName: "ipfs" });
    ctx.secretStorage.seed("ipfs", "pinata_jwt", "revoked");

    expect(await rejectSecret("pinata_jwt")).toBeNull();
    expect(await getSecret("pinata_jwt")).toBeNull();
  });

  it("stores what a supervised login returned, scoped to the caller", async () => {
    ctx = setupMockTauri({ pluginName: "matters" });
    await setSecret("access_token", "tok-from-oauth");
    expect(await getSecret("access_token")).toBe("tok-from-oauth");
    expect(ctx.secretStorage.get("ipfs", "access_token")).toBeNull();
  });

  it("cannot reach another plugin's secret of the same name", async () => {
    ctx = setupMockTauri({ pluginName: "github" });
    ctx.secretStorage.seed("ipfs", "token", "ipfs-token");
    expect(await getSecret("token")).toBeNull();
  });
});
