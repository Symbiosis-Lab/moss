/**
 * Unit tests for inline-status.ts.
 *
 * Pure functions (formatBadgeLabel, badgeStatusClass, isTerminal) are
 * exercised directly. DOM-mutating behavior (applyTaskUpdate) is
 * exercised through jsdom — the test asserts badge presence, class
 * names, and fade-after behavior using fake timers.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskState } from '@symbiosis-lab/moss-ui';
import type { PanelTaskWire } from '@symbiosis-lab/moss-ui';
import {
  applyTaskUpdate,
  badgeStatusClass,
  formatBadgeLabel,
  isTerminal,
  __badgeCountForTests,
  __resetForTests,
} from "../inline-status";

// Post-collapse, a failed `TaskState` carries its root-cause advisory. The
// `what` is the human message and the `severity` is the recoverability
// (`Blocking` = unrecoverable, `NeedsAction` = recoverable).
function failedState(what: string, recoverable: boolean): TaskState {
  return {
    state: "failed",
    advisory: {
      scope: "Config",
      severity: recoverable ? "NeedsAction" : "Blocking",
      item: null,
      what,
      action: "None",
    },
  };
}

// A bare-cancel awaiting state (`Action::None`).
function awaitingState(directive: string): TaskState {
  return { state: "awaiting", directive, action: "None" };
}

function wire(overrides: Partial<PanelTaskWire>): PanelTaskWire {
  return {
    id: "1",
    window_id: "main",
    scope: "action_panel",
    // Default to a neutral kind: silent-save suppresses `kind:"save"` for
    // non-failed states, so generic badge/fade machinery tests use "format".
    // Tests that exercise save semantics set kind:"save" explicitly.
    kind: "format",
    tone: "inline",
    title: null,
    fade_after_ms: null,
    started_at_ms: "0",
    state: { state: "running", fraction: null, message: null },
    ...overrides,
  } as PanelTaskWire;
}

// ── formatBadgeLabel ────────────────────────────────────────────────────

describe("formatBadgeLabel", () => {
  it("uses title for running tasks when present", () => {
    expect(
      formatBadgeLabel(
        wire({
          title: "Saving index.md",
          state: { state: "running", fraction: 0.5, message: "writing" },
        }),
      ),
    ).toBe("Saving index.md");
  });

  it("falls back to state message when no title", () => {
    expect(
      formatBadgeLabel(
        wire({
          title: null,
          state: { state: "running", fraction: 0.5, message: "encoding" },
        }),
      ),
    ).toBe("encoding");
  });

  it("falls back to 'Working…' when running with neither", () => {
    expect(
      formatBadgeLabel(
        wire({
          title: null,
          state: { state: "running", fraction: null, message: null },
        }),
      ),
    ).toBe("Working…");
  });

  it("renders receipt for succeeded", () => {
    expect(
      formatBadgeLabel(
        wire({ state: { state: "succeeded", receipt: "Saved" } }),
      ),
    ).toBe("Saved");
  });

  it("derives label from title when succeeded with no receipt", () => {
    expect(
      formatBadgeLabel(
        wire({ title: "Save", state: { state: "succeeded", receipt: null } }),
      ),
    ).toBe("Save complete");
  });

  it("renders error for failed", () => {
    expect(
      formatBadgeLabel(
        wire({
          state: failedState("disk full", false),
        }),
      ),
    ).toBe("Failed: disk full");
  });

  it("renders Cancelled for cancelled", () => {
    expect(formatBadgeLabel(wire({ state: { state: "cancelled" } }))).toBe(
      "Cancelled",
    );
  });

  it("renders awaiting directive verbatim", () => {
    expect(
      formatBadgeLabel(
        wire({
          state: {
            state: "awaiting",
            directive: "verify DNS",
            action: "None",
          },
        }),
      ),
    ).toBe("verify DNS");
  });
});

// ── badgeStatusClass + isTerminal ─────────────────────────────────────

describe("badgeStatusClass", () => {
  it("maps each state to its class", () => {
    expect(
      badgeStatusClass({ state: "running", fraction: null, message: null }),
    ).toBe("is-running");
    expect(
      badgeStatusClass({
        state: "awaiting",
        directive: "x",
        action: "None",
      }),
    ).toBe("is-awaiting");
    expect(badgeStatusClass({ state: "succeeded", receipt: null })).toBe(
      "is-succeeded",
    );
    expect(
      badgeStatusClass(failedState("x", false)),
    ).toBe("is-failed");
    expect(badgeStatusClass({ state: "cancelled" })).toBe("is-cancelled");
  });
});

describe("isTerminal", () => {
  it("returns true for succeeded/failed/cancelled, false otherwise", () => {
    expect(isTerminal({ state: "succeeded", receipt: null })).toBe(true);
    expect(isTerminal(failedState("x", false))).toBe(
      true,
    );
    expect(isTerminal({ state: "cancelled" })).toBe(true);
    expect(isTerminal({ state: "running", fraction: null, message: null })).toBe(
      false,
    );
    expect(
      isTerminal({
        state: "awaiting",
        directive: "x",
        action: "None",
      }),
    ).toBe(false);
  });
});

// ── applyTaskUpdate filtering ─────────────────────────────────────────

describe("applyTaskUpdate (filtering)", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });
  afterEach(() => {
    __resetForTests();
    document.body.innerHTML = "";
  });

  it("ignores non-inline tones", () => {
    applyTaskUpdate(wire({ tone: "ambient" }));
    expect(__badgeCountForTests()).toBe(0);
    applyTaskUpdate(wire({ tone: "narrated" }));
    expect(__badgeCountForTests()).toBe(0);
    applyTaskUpdate(wire({ tone: "awaiting" }));
    expect(__badgeCountForTests()).toBe(0);
  });

  it("renders a badge for inline tone", () => {
    applyTaskUpdate(
      wire({
        title: "Saving",
        state: { state: "running", fraction: 0.5, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    const badge = document.querySelector(".moss-inline-status");
    expect(badge).not.toBeNull();
    expect(badge?.classList.contains("is-running")).toBe(true);
    expect(badge?.textContent).toContain("Saving");
  });
});

// ── applyTaskUpdate DOM behavior + fade_after ─────────────────────────

describe("applyTaskUpdate (DOM + fade_after)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    document.body.innerHTML = '<div data-inline-task-anchor="format"></div>';
  });
  afterEach(() => {
    __resetForTests();
    document.body.innerHTML = "";
    vi.useRealTimers();
  });

  it("appends the badge to a kind-specific anchor when present", () => {
    applyTaskUpdate(
      wire({
        title: "Saving",
        state: { state: "running", fraction: 0.5, message: null },
      }),
    );
    const anchor = document.querySelector(
      '[data-inline-task-anchor="format"]',
    ) as HTMLElement;
    const badge = anchor.querySelector(".moss-inline-status");
    expect(badge).not.toBeNull();
  });

  it("creates a fallback container when no anchor matches the kind", () => {
    document.body.innerHTML = ""; // no anchor at all
    applyTaskUpdate(
      wire({
        kind: "format",
        title: "Formatting",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    const fallback = document.getElementById("moss-inline-status-fallback");
    expect(fallback).not.toBeNull();
    expect(fallback?.querySelector(".moss-inline-status")).not.toBeNull();
  });

  it("updates the same badge on subsequent task updates (no stacking)", () => {
    applyTaskUpdate(
      wire({
        id: "42",
        state: { state: "running", fraction: 0.2, message: null },
      }),
    );
    applyTaskUpdate(
      wire({
        id: "42",
        state: { state: "running", fraction: 0.7, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
  });

  it("replaces a sibling badge on same anchor+kind when a new task arrives", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        state: { state: "succeeded", receipt: "Saved" },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    // New save spawns — the prior "Saved" receipt is replaced (no
    // accumulating badges).
    applyTaskUpdate(
      wire({
        id: "2",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
  });

  it("persistent (fade_after_ms=null) terminal badge stays after success", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        fade_after_ms: null,
        state: { state: "succeeded", receipt: "Saved" },
      }),
    );
    // Advance past the typical fade window — badge should still be present.
    vi.advanceTimersByTime(60_000);
    expect(__badgeCountForTests()).toBe(1);
    const badge = document.querySelector(".moss-inline-status");
    expect(badge).not.toBeNull();
    expect(badge?.classList.contains("is-fading")).toBe(false);
  });

  it("fade_after_ms triggers CSS fade after the duration, then DOM-remove", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        fade_after_ms: 1000,
        state: { state: "succeeded", receipt: "Saved" },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    // Just before fade-after: badge present, not fading.
    vi.advanceTimersByTime(999);
    const badge = document.querySelector(".moss-inline-status");
    expect(badge?.classList.contains("is-fading")).toBe(false);

    // At fade-after: CSS fade class applied.
    vi.advanceTimersByTime(2);
    expect(
      document.querySelector(".moss-inline-status")?.classList.contains(
        "is-fading",
      ),
    ).toBe(true);

    // After fade animation (400ms): DOM removed.
    vi.advanceTimersByTime(400);
    expect(__badgeCountForTests()).toBe(0);
  });

  it("running state cancels a pending fade timer (retry scenario)", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        fade_after_ms: 1000,
        state: { state: "succeeded", receipt: "Saved" },
      }),
    );
    vi.advanceTimersByTime(500); // halfway through fade window
    // Same task transitions back to running (e.g., user retried).
    applyTaskUpdate(
      wire({
        id: "1",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    // Past the original fade window — badge should still be present,
    // not fading.
    vi.advanceTimersByTime(2000);
    expect(__badgeCountForTests()).toBe(1);
    expect(
      document.querySelector(".moss-inline-status")?.classList.contains(
        "is-fading",
      ),
    ).toBe(false);
  });

  it("tone escalation (inline -> awaiting) removes the orphaned badge", () => {
    // First emit: classic inline running save badge appears.
    applyTaskUpdate(
      wire({
        id: "7",
        tone: "inline",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    expect(document.querySelector(".moss-inline-status")).not.toBeNull();

    // Router escalates the same task to awaiting (e.g., save hit a
    // non-recoverable network confirmation). The wire payload carries
    // tone="awaiting"; ownership moves to the Awaiting renderer.
    applyTaskUpdate(
      wire({
        id: "7",
        tone: "awaiting",
        state: {
          state: "awaiting",
          directive: "retry from network panel",
          action: "None",
        },
      }),
    );
    // The Inline renderer must drop its badge so the Awaiting renderer
    // has a clean slate. Without this guard, the DOM node would linger
    // until process death.
    expect(__badgeCountForTests()).toBe(0);
    expect(document.querySelector(".moss-inline-status")).toBeNull();
  });
});

// ── silent-save: Save renders only on failure ─────────────────────────────

describe("applyTaskUpdate (silent-save)", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });
  afterEach(() => {
    __resetForTests();
    document.body.innerHTML = "";
  });

  it("renders no badge for a running save", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        kind: "save",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(0);
  });

  it("renders no badge for a succeeded save (no receipt)", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        kind: "save",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    applyTaskUpdate(
      wire({
        id: "1",
        kind: "save",
        state: { state: "succeeded", receipt: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(0);
  });

  it("renders a persistent badge for a failed save", () => {
    vi.useFakeTimers();
    applyTaskUpdate(
      wire({
        id: "1",
        kind: "save",
        fade_after_ms: null,
        state: failedState("disk full", true),
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    vi.advanceTimersByTime(60_000); // far past any fade
    expect(__badgeCountForTests()).toBe(1);
    vi.useRealTimers();
  });

  it("clears a prior failed save badge when the next save starts", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        kind: "save",
        fade_after_ms: null,
        state: failedState("disk full", true),
      }),
    );
    expect(__badgeCountForTests()).toBe(1);
    // A new save task begins (different id) → the stale failure clears.
    applyTaskUpdate(
      wire({
        id: "2",
        kind: "save",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    expect(__badgeCountForTests()).toBe(0);
  });
});
