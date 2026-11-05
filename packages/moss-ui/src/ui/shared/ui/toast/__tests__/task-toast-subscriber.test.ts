/**
 * Unit tests for task-toast-subscriber.ts .
 *
 * - shouldToast rule: failed + !recoverable
 * - dedupe by TaskId across multiple updates
 * - formatToastMessage shape
 * - calls showToast with the right args
 */

import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

const { showToastMock } = vi.hoisted(() => ({
  showToastMock: vi.fn(),
}));
vi.mock("../toast-manager", async () => {
  const actual = await vi.importActual<typeof import("../toast-manager")>(
    "../toast-manager",
  );
  return {
    ...actual,
    showToast: showToastMock,
  };
});

import type { Advisory, PanelTaskWire, TaskState } from '@symbiosis-lab/moss-ui';
import {
  applyTaskUpdate,
  formatToastMessage,
  shouldToast,
  shippedDegradedAdvisory,
  __resetForTests,
  __seenCountForTests,
} from "../task-toast-subscriber";

// Post-collapse, a failed `TaskState` carries its root-cause advisory.
// Recoverability is the advisory's severity: `Blocking` = unrecoverable (the
// case the toast surfaces), `NeedsAction` = recoverable (no toast).
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

// A Succeeded state carrying a single non-blocking advisory at `severity`.
function succeededWithAdvisory(
  what: string,
  severity: Advisory["severity"],
): TaskState {
  return {
    state: "succeeded",
    receipt: "Syndicated 2 posts",
    advisories: [
      {
        scope: "Remote",
        severity,
        item: null,
        what,
        action: "None",
      },
    ],
  };
}

function wire(overrides: Partial<PanelTaskWire>): PanelTaskWire {
  return {
    id: "1",
    window_id: "main",
    scope: "action_panel",
    kind: "save",
    tone: "inline",
    title: null,
    fade_after_ms: null,
    started_at_ms: "0",
    state: failedState("disk full", false),
    ...overrides,
  } as PanelTaskWire;
}

// ── shouldToast ───────────────────────────────────────────────────────

describe("shouldToast", () => {
  it("returns true for failed + recoverable=false + non-inline tone", () => {
    expect(
      shouldToast(
        wire({
          tone: "ambient",
          state: failedState("boom", false),
        }),
      ),
    ).toBe(true);
  });

  it("returns false for inline tone (the badge is the surface)", () => {
    expect(
      shouldToast(
        wire({
          tone: "inline",
          state: failedState("boom", false),
        }),
      ),
    ).toBe(false);
  });

  it("returns false for failed + recoverable=true", () => {
    expect(
      shouldToast(
        wire({
          tone: "ambient",
          state: failedState("offline", true),
        }),
      ),
    ).toBe(false);
  });

  it("returns false for non-failed states", () => {
    expect(
      shouldToast(
        wire({
          tone: "ambient",
          state: { state: "running", fraction: 0.5, message: null },
        }),
      ),
    ).toBe(false);
    expect(
      shouldToast(
        wire({ tone: "ambient", state: { state: "succeeded", receipt: "Saved" } }),
      ),
    ).toBe(false);
    expect(
      shouldToast(wire({ tone: "ambient", state: { state: "cancelled" } })),
    ).toBe(false);
  });

  it("returns false for a Preview-scope Job (the glass panel owns its failures)", () => {
    // Preview-scope Jobs (Domain, Deploy, Build) render in the glass
    // panel — a failure shows there as a row. The toast subscriber must NOT
    // also fire, or the user gets a double-render (panel row + toast). The
    // toast remains the cross-surface signal for action-panel / workspace work.
    expect(
      shouldToast(
        wire({
          scope: "preview",
          tone: "ambient",
          state: failedState("publish failed", false),
        }),
      ),
    ).toBe(false);
  });
});

// ── shippedDegradedAdvisory (Gap-2, R12 quiet surfacing) ─────────────────

describe("shippedDegradedAdvisory", () => {
  it("surfaces a Succeeded Job's ShippedDegraded advisory as an info toast", () => {
    // A (Syndicate, Background) success Job that couldn't fully syndicate
    // stores a non-blocking ShippedDegraded advisory on Succeeded.advisories.
    // Per R12 it surfaces QUIETLY — an info-severity ephemeral toast.
    const adv = shippedDegradedAdvisory(
      wire({
        scope: "workspace",
        tone: "ambient",
        state: succeededWithAdvisory(
          "1 post(s) could not be syndicated",
          "ShippedDegraded",
        ),
      }),
    );
    expect(adv).not.toBeNull();
    expect(adv?.severity).toBe("ShippedDegraded");
    expect(adv?.what).toBe("1 post(s) could not be syndicated");
  });

  it("returns null for a clean Succeeded Job (no advisories — success makes no sound)", () => {
    expect(
      shippedDegradedAdvisory(
        wire({ scope: "workspace", state: { state: "succeeded", receipt: "Done" } }),
      ),
    ).toBeNull();
  });

  it("returns null when the success advisory is not ShippedDegraded", () => {
    // A NeedsAction success advisory is recoverable, surfaced via the inline
    // badge / action-panel — not this quiet info toast.
    expect(
      shippedDegradedAdvisory(
        wire({
          scope: "workspace",
          state: succeededWithAdvisory("install ffmpeg", "NeedsAction"),
        }),
      ),
    ).toBeNull();
  });

  it("returns null for non-Succeeded states", () => {
    expect(
      shippedDegradedAdvisory(wire({ state: failedState("boom", false) })),
    ).toBeNull();
    expect(
      shippedDegradedAdvisory(
        wire({ state: { state: "running", fraction: 0.5, message: null } }),
      ),
    ).toBeNull();
  });

  it("ignores a Preview-scope Succeeded advisory (the glass panel owns those)", () => {
    // Preview-scope Jobs render their advisories in the progress panel; the
    // toast must not double-render them.
    expect(
      shippedDegradedAdvisory(
        wire({
          scope: "preview",
          state: succeededWithAdvisory("degraded", "ShippedDegraded"),
        }),
      ),
    ).toBeNull();
  });
});

// ── formatToastMessage ─────────────────────────────────────────────────

describe("formatToastMessage", () => {
  it("combines title + error when both present", () => {
    expect(
      formatToastMessage(
        wire({
          title: "Saving index.md",
          state: failedState("disk full", false),
        }),
      ),
    ).toBe("Saving index.md: disk full");
  });

  it("uses just the error when no title", () => {
    expect(
      formatToastMessage(
        wire({
          title: null,
          state: failedState("boom", false),
        }),
      ),
    ).toBe("boom");
  });

  it("returns empty string for non-failed", () => {
    expect(
      formatToastMessage(
        wire({
          state: { state: "running", fraction: 0.5, message: null },
        }),
      ),
    ).toBe("");
  });
});

// ── applyTaskUpdate (filtering + dedupe + side effects) ───────────────

describe("applyTaskUpdate", () => {
  beforeEach(() => {
    showToastMock.mockClear();
    __resetForTests();
  });
  afterEach(() => {
    __resetForTests();
  });

  it("does not toast when shouldToast returns false", () => {
    // succeeded → no toast
    applyTaskUpdate(
      wire({
        tone: "ambient",
        state: { state: "succeeded", receipt: null },
      }),
    );
    // recoverable failure → no toast
    applyTaskUpdate(
      wire({
        tone: "ambient",
        state: failedState("x", true),
      }),
    );
    // running → no toast
    applyTaskUpdate(
      wire({
        tone: "ambient",
        state: { state: "running", fraction: null, message: null },
      }),
    );
    // inline Blocking failure → suppressed (the badge is the surface)
    applyTaskUpdate(
      wire({
        tone: "inline",
        state: failedState("inline fail", false),
      }),
    );
    expect(showToastMock).not.toHaveBeenCalled();
    expect(__seenCountForTests()).toBe(0);
  });

  it("toasts once per failed+!recoverable wire payload (non-inline tone)", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        tone: "ambient",
        title: "Saving",
        state: failedState("disk full", false),
      }),
    );
    expect(showToastMock).toHaveBeenCalledTimes(1);
    expect(showToastMock).toHaveBeenCalledWith(
      { what: "Saving: disk full", severity: "Blocking" },
      expect.objectContaining({ id: "task-failed-1" }),
    );
    expect(__seenCountForTests()).toBe(1);
  });

  it("dedupes by TaskId — re-emits do not double-toast", () => {
    const payload = wire({
      id: "42",
      tone: "ambient",
      state: failedState("boom", false),
    });
    applyTaskUpdate(payload);
    applyTaskUpdate(payload);
    applyTaskUpdate(payload);
    expect(showToastMock).toHaveBeenCalledTimes(1);
    expect(__seenCountForTests()).toBe(1);
  });

  it("toasts independently for different TaskIds", () => {
    applyTaskUpdate(
      wire({
        id: "1",
        tone: "ambient",
        state: failedState("a", false),
      }),
    );
    applyTaskUpdate(
      wire({
        id: "2",
        tone: "ambient",
        state: failedState("b", false),
      }),
    );
    expect(showToastMock).toHaveBeenCalledTimes(2);
    expect(__seenCountForTests()).toBe(2);
  });

  it("surfaces a Workspace Succeeded ShippedDegraded advisory as a quiet info toast", () => {
    applyTaskUpdate(
      wire({
        id: "7",
        scope: "workspace",
        tone: "ambient",
        title: "Syndicate",
        state: succeededWithAdvisory(
          "1 post(s) could not be syndicated",
          "ShippedDegraded",
        ),
      }),
    );
    expect(showToastMock).toHaveBeenCalledTimes(1);
    expect(showToastMock).toHaveBeenCalledWith(
      { what: "1 post(s) could not be syndicated", severity: "ShippedDegraded" },
      expect.objectContaining({ id: "task-degraded-7" }),
    );
    expect(__seenCountForTests()).toBe(1);
  });

  it("dedupes the Gap-2 info toast by TaskId — re-emits do not double-toast", () => {
    const payload = wire({
      id: "8",
      scope: "workspace",
      state: succeededWithAdvisory("degraded", "ShippedDegraded"),
    });
    applyTaskUpdate(payload);
    applyTaskUpdate(payload);
    expect(showToastMock).toHaveBeenCalledTimes(1);
    expect(__seenCountForTests()).toBe(1);
  });

  it("does not toast a clean Succeeded Job (success makes no sound)", () => {
    applyTaskUpdate(
      wire({
        id: "9",
        scope: "workspace",
        state: { state: "succeeded", receipt: "Syndicated 2 posts" },
      }),
    );
    expect(showToastMock).not.toHaveBeenCalled();
    expect(__seenCountForTests()).toBe(0);
  });

  it("suppresses inline-tone failures — badge is the surface, no double-signal", () => {
    // Invariant #4: tone==="inline" → no toast; the inline badge is the surface.
    applyTaskUpdate(
      wire({
        id: "1",
        tone: "inline",
        state: failedState("x", false),
      }),
    );
    expect(showToastMock).not.toHaveBeenCalled();
  });

  it("still toasts non-inline Blocking failures (narrated/ambient/awaiting)", () => {
    applyTaskUpdate(
      wire({
        id: "2",
        tone: "ambient",
        state: failedState("y", false),
      }),
    );
    applyTaskUpdate(
      wire({
        id: "3",
        tone: "narrated",
        state: failedState("z", false),
      }),
    );
    applyTaskUpdate(
      wire({
        id: "4",
        tone: "awaiting",
        state: failedState("w", false),
      }),
    );
    expect(showToastMock).toHaveBeenCalledTimes(3);
  });

  it("Blocking task toast has no finite duration (Invariant #2 — persists until dismissed)", () => {
    applyTaskUpdate(
      wire({
        id: "5",
        tone: "ambient",
        state: failedState("boom", false),
      }),
    );
    expect(showToastMock).toHaveBeenCalledTimes(1);
    const [, opts] = showToastMock.mock.calls[0];
    // Must carry { persistent: true } and NOT carry a finite `duration`.
    expect(opts).toMatchObject({ persistent: true });
    expect(opts).not.toHaveProperty("duration");
  });
});
