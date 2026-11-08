import { describe, test, expect, vi, beforeEach } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";
import { emitEngineEvent, engineSubscribers, resetTestHost } from "../../__tests__/helpers/test-host";

// The runtime tests below fire a real "moss-event" payload through the test
// host's engine subscription (the source-scan tests need no module import).
const fireMossEvent = (payload: unknown) => emitEngineEvent("moss-event", payload);

// A listener that calls `onMossEvent("PublishVerdict", ...)` directly bypasses
// `subscribePublishVerdict`'s folder/target filter. Structurally forbid it:
// only moss-events.ts itself (where `subscribePublishVerdict` is implemented)
// may call `onMossEvent("PublishVerdict", ...)`.
// Ablation: add a second raw `onMossEvent("PublishVerdict", ...)` call
// anywhere else under src/ui — this goes red.
//
// The scan also covers two other ways to reach the same event:
//   (a) the low-level primitive — `listen("moss-event", ...)` with a
//       hand-rolled `payload.kind === "PublishVerdict"` check, skipping
//       `onMossEvent` entirely.
//   (c) a renamed import (`import { onMossEvent as sub } from ...`), which
//       defeats a plain search for the name `onMossEvent`.
// A third dodge — `onMossEvent(kind, ...)` where `kind` is a variable — is
// NOT resolvable by a source scan, so the second describe block below
// forbids passing anything but a string literal to `onMossEvent`, package-wide.

const APP = resolve(__dirname, "..", "..");
const ALLOWED_FILE = resolve(APP, "utils/moss-events.ts");
const CALL_PATTERN = /onMossEvent\(\s*["']PublishVerdict["']/;
// (a) the raw channel name, as a direct `listen(...)` call.
const RAW_CHANNEL_PATTERN = /\blisten\(\s*["']moss-event["']/;
// (a) a hand-rolled discriminant check on the kind field.
const HAND_KIND_CHECK_PATTERN = /\.kind\s*===\s*["']PublishVerdict["']/;
// (c) `import { onMossEvent as <alias> }` — captures the alias so the call
// pattern below can be rebuilt against it.
const RENAMED_IMPORT_PATTERN = /\bonMossEvent\s+as\s+(\w+)\b/;

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "__tests__") continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, out);
    } else if (entry.isFile() && entry.name.endsWith(".ts") && !entry.name.endsWith(".test.ts")) {
      out.push(full);
    }
  }
  return out;
}

/** True if `content` reaches a `PublishVerdict` event by any means this test
 * knows to check — the literal call, or dodges (a) and (c) above. */
function subscribesToPublishVerdictDirectly(content: string): boolean {
  if (CALL_PATTERN.test(content)) return true;
  if (RAW_CHANNEL_PATTERN.test(content) && HAND_KIND_CHECK_PATTERN.test(content)) return true;
  const renamed = RENAMED_IMPORT_PATTERN.exec(content);
  if (renamed) {
    const alias = renamed[1];
    const aliasCallPattern = new RegExp(`\\b${alias}\\(\\s*["']PublishVerdict["']`);
    if (aliasCallPattern.test(content)) return true;
  }
  return false;
}

describe("PublishVerdict has exactly one subscription entry point", () => {
  test('only utils/moss-events.ts calls onMossEvent("PublishVerdict", ...)', () => {
    const files = walk(APP);
    expect(files).toContain(ALLOWED_FILE);
    const offenders = files
      .filter((file) => file !== ALLOWED_FILE)
      .filter((file) => subscribesToPublishVerdictDirectly(readFileSync(file, "utf8")));

    expect(
      offenders,
      `these files subscribe to PublishVerdict directly instead of through ` +
        `subscribePublishVerdict (utils/moss-events.ts): ${offenders.join(", ")}`,
    ).toEqual([]);
  });
});

describe("onMossEvent()'s kind argument is always a string literal", () => {
  // Excludes a bare `.` so a comment like "the direct `void onMossEvent(...)`
  // subscription" (editor-main.ts) — ellipsis prose, not a call — doesn't
  // register as a violation; no real argument can start with a bare dot.
  const NON_LITERAL_KIND_PATTERN = /\bonMossEvent\(\s*[^"'\s).]/;

  test("every onMossEvent(...) call in the package passes a literal kind, never a variable", () => {
    const offenders = walk(APP).filter((file) => NON_LITERAL_KIND_PATTERN.test(readFileSync(file, "utf8")));

    expect(
      offenders,
      `these files call onMossEvent with a non-literal kind, which no scan (including ` +
        `the PublishVerdict check above) can verify: ${offenders.join(", ")}`,
    ).toEqual([]);
  });
});

// The real `subscribePublishVerdict` folder/target filter, exercised through
// the real `moss-event` channel (mocked `listen()` only). Previously proven
// only incidentally, through one consumer (the email composer) firing a
// stray-folder verdict at it; that consumer no longer holds a subscription
// of its own; mountEmailComposer owns that subscription. Test the filter here
// instead of through whichever caller happens to exist.
describe("onMossEvents — one subscription, one handler per kind", () => {
  beforeEach(() => {
    resetTestHost();
  });

  test("hands each event to its kind's handler and ignores kinds it has none for", async () => {
    const { onMossEvents } = await import("../moss-events");
    const built = vi.fn();
    const ready = vi.fn();
    await onMossEvents({ BuildComplete: built, AssetReady: ready });
    expect(engineSubscribers("moss-event")).toBe(1);
    fireMossEvent({ kind: "BuildComplete", payload: { total_time_ms: 5 } });
    fireMossEvent({ kind: "FolderHealthChanged", payload: { folder: "/a", unavailable: false, degraded: false } });
    expect(built).toHaveBeenCalledWith({ kind: "BuildComplete", payload: { total_time_ms: 5 } });
    expect(ready).not.toHaveBeenCalled();
  });

  test("takes no PublishVerdict handler: a verdict reaches a listener only through the folder filter", async () => {
    const { onMossEvents } = await import("../moss-events");
    // @ts-expect-error PublishVerdict is not a key of MossEventHandlers.
    const typeOnly = () => onMossEvents({ PublishVerdict: () => {} });
    expect(typeof typeOnly).toBe("function");
  });
});

describe("subscribePublishVerdict — folder/target filter", () => {
  beforeEach(() => {
    resetTestHost();
  });

  const verdict = (overrides: Record<string, unknown> = {}) => ({
    target: "moss",
    folder: "/site-a",
    state: "live",
    origin: "publish",
    generation: "gen-1",
    url: "https://example.mosspub.com",
    http_code: "200",
    bootstrap_pct: null,
    pages: [],
    ...overrides,
  });

  test("a verdict for a different folder never reaches the callback", async () => {
    const { subscribePublishVerdict } = await import("../moss-events");
    const cb = vi.fn();
    subscribePublishVerdict(() => "/site-a", null, cb);
    fireMossEvent({ kind: "PublishVerdict", payload: verdict({ folder: "/site-b" }) });
    expect(cb).not.toHaveBeenCalled();
  });

  test("a mismatched target never reaches the callback", async () => {
    const { subscribePublishVerdict } = await import("../moss-events");
    const cb = vi.fn();
    subscribePublishVerdict(() => "/site-a", "onionpress", cb);
    fireMossEvent({ kind: "PublishVerdict", payload: verdict({ target: "moss" }) });
    expect(cb).not.toHaveBeenCalled();
  });

  test("a matching folder and target reaches the callback", async () => {
    const { subscribePublishVerdict } = await import("../moss-events");
    const cb = vi.fn();
    subscribePublishVerdict(() => "/site-a", "moss", cb);
    const v = verdict({ folder: "/site-a", target: "moss" });
    fireMossEvent({ kind: "PublishVerdict", payload: v });
    expect(cb).toHaveBeenCalledWith(v);
  });
});
