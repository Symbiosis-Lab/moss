/**
 * Tests for the awaiting pure helpers.
 *
 * The DOM-mutating awaiting renderer was deleted;
 * its behaviour (amber dot, directive, escape action) is now exercised via
 * progress-panel-narration.test.ts. The pure formatting functions
 * (formatAwaitingCopy, escapeLabel, escapeCommand, escapeKind, dispatchEscape)
 * that remain useful are kept here, importing from narration-format.ts.
 *
 * dispatchEscape (Tauri invoke routing) is kept in narration-format.ts and
 * tested here for completeness. The panel itself invokes escape commands via
 * the task's `action` field passed to the renderer.
 *
 * The Awaiting affordance is driven by the typed `Action`
 * (`None` = bare cancel, `InApp{RecheckDns}` = recheck, any other `InApp`
 * = resend), no longer the stringly `EscapeAction`.
 */

import {
  describe,
  expect,
  it,
} from "vitest";

import type { Action } from '@symbiosis-lab/moss-ui';
import {
  escapeCommand,
  escapeKind,
  escapeLabel,
  formatAwaitingCopy,
} from "../narration-format";

// Typed fixtures for the three escape shapes the renderer cares about.
const CANCEL: Action = "None";
const RESEND: Action = {
  InApp: { op: "SignIn", args: { reason: "resend" }, label: "Resend email" },
};
const RECHECK: Action = {
  InApp: { op: "RecheckDns", args: null, label: "Recheck DNS" },
};

// ── Pure helpers ──────────────────────────────────────────────────────

describe("formatAwaitingCopy", () => {
  it("prefixes the directive with 'Waiting for you to ' and adds a period", () => {
    expect(
      formatAwaitingCopy({
        state: "awaiting",
        directive: "click the link in your email",
        action: CANCEL,
      }),
    ).toBe("Waiting for you to click the link in your email.");
  });

  it("does not add an extra period if directive ends with punctuation", () => {
    expect(
      formatAwaitingCopy({
        state: "awaiting",
        directive: "confirm sign-in on your other device.",
        action: CANCEL,
      }),
    ).toBe("Waiting for you to confirm sign-in on your other device.");
  });

  it("returns 'Waiting for you.' for empty directive", () => {
    expect(
      formatAwaitingCopy({
        state: "awaiting",
        directive: "",
        action: CANCEL,
      }),
    ).toBe("Waiting for you.");
  });

  it("returns empty string for non-awaiting state (defensive)", () => {
    expect(
      formatAwaitingCopy({
        state: "running",
        fraction: null,
        message: null,
      }),
    ).toBe("");
  });
});

describe("escapeLabel", () => {
  it("returns 'Cancel' for the bare-cancel action (None)", () => {
    expect(escapeLabel(CANCEL)).toBe("Cancel");
  });
  it("returns the action's label for a resend InApp", () => {
    expect(escapeLabel(RESEND)).toBe("Resend email");
  });
  it("returns the action's label for a recheck InApp", () => {
    expect(escapeLabel(RECHECK)).toBe("Recheck DNS");
  });
});

describe("escapeCommand", () => {
  it("maps each action to its Tauri command name", () => {
    expect(escapeCommand(CANCEL)).toBe("cancel_task");
    expect(escapeCommand(RESEND)).toBe("task_resend");
    expect(escapeCommand(RECHECK)).toBe("task_recheck");
  });
});

describe("escapeKind", () => {
  it("classifies each action for the data-escape-kind attribute", () => {
    expect(escapeKind(CANCEL)).toBe("cancel");
    expect(escapeKind(RESEND)).toBe("resend");
    expect(escapeKind(RECHECK)).toBe("recheck");
  });
});

// ── Type narrowing sanity check ───────────────────────────────────────

describe("Action escape shapes", () => {
  it("compiles for the bare-cancel and InApp affordances", () => {
    const cancel: Action = "None";
    const resend: Action = {
      InApp: { op: "SignIn", args: { reason: "resend" }, label: "Resend" },
    };
    const recheck: Action = {
      InApp: { op: "RecheckDns", args: null, label: "Recheck" },
    };
    expect(escapeKind(cancel)).toBe("cancel");
    expect(escapeKind(resend)).toBe("resend");
    expect(escapeKind(recheck)).toBe("recheck");
  });
});
