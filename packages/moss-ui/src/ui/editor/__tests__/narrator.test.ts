/**
 * Tests for the narration pure helpers.
 *
 * The DOM-mutating narrator renderer was deleted;
 * its behaviour is now exercised via progress-panel-narration.test.ts.
 * The pure formatting functions (pickHeadline, formatNarration) moved to
 * narration-format.ts and are tested here.
 */

import { describe, expect, it } from "vitest";
import {
  formatNarration,
  pickHeadline,
  type NarratedTask,
} from "../narration-format";

// ── pickHeadline ──────────────────────────────────────────────────────

describe("pickHeadline", () => {
  it("returns null for empty list", () => {
    expect(pickHeadline([])).toBeNull();
  });

  it("picks the most recently started non-terminal task", () => {
    const headline = pickHeadline([
      {
        id: "1",
        title: "older",
        startedAtMs: 100,
        state: { state: "running", fraction: null, message: null },
      },
      {
        id: "2",
        title: "newer",
        startedAtMs: 200,
        state: { state: "running", fraction: null, message: null },
      },
    ]);
    expect(headline?.title).toBe("newer");
  });

  it("prefers non-terminal even when terminal is newer", () => {
    const headline = pickHeadline([
      {
        id: "1",
        title: "old-running",
        startedAtMs: 100,
        state: { state: "running", fraction: null, message: null },
      },
      {
        id: "2",
        title: "new-success",
        startedAtMs: 200,
        state: { state: "succeeded", receipt: null },
      },
    ]);
    expect(headline?.title).toBe("old-running");
  });

  it("falls back to most recent terminal when all are terminal", () => {
    const headline = pickHeadline([
      {
        id: "1",
        title: "first",
        startedAtMs: 100,
        state: { state: "succeeded", receipt: null },
      },
      {
        id: "2",
        title: "last",
        startedAtMs: 200,
        state: { state: "succeeded", receipt: null },
      },
    ]);
    expect(headline?.title).toBe("last");
  });
});

// ── formatNarration ───────────────────────────────────────────────────

describe("formatNarration", () => {
  it("uses title for running tasks", () => {
    expect(
      formatNarration({
        id: "1",
        title: "Connecting to Matters",
        startedAtMs: 0,
        state: { state: "running", fraction: null, message: null },
      }),
    ).toBe("Connecting to Matters");
  });

  it("uses message when title is empty", () => {
    expect(
      formatNarration({
        id: "1",
        title: null,
        startedAtMs: 0,
        state: { state: "running", fraction: null, message: "Importing posts" },
      }),
    ).toBe("Importing posts");
  });

  it("uses receipt for succeeded", () => {
    expect(
      formatNarration({
        id: "1",
        title: "Import",
        startedAtMs: 0,
        state: { state: "succeeded", receipt: "All set — 42 posts imported" },
      }),
    ).toBe("All set — 42 posts imported");
  });

  it("derives 'X complete' from title when succeeded with no receipt", () => {
    expect(
      formatNarration({
        id: "1",
        title: "Import",
        startedAtMs: 0,
        state: { state: "succeeded", receipt: null },
      }),
    ).toBe("Import complete");
  });

  it("renders failure with prefix", () => {
    expect(
      formatNarration({
        id: "1",
        title: null,
        startedAtMs: 0,
        state: {
          state: "failed",
          advisory: {
            scope: "Config",
            severity: "NeedsAction",
            item: null,
            what: "network down",
            action: "None",
          },
        },
      }),
    ).toBe("Failed: network down");
  });
});
