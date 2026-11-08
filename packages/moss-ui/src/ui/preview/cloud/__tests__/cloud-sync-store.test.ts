/**
 * The store is the single owner of the cloud-wait fact, so these tests pin the
 * semantics both surfaces depend on, so they cannot disagree about the same fact.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { cloudSync, type CloudSyncChange, type CloudSyncPayload } from "../cloud-sync-store";
import { unavailableAdvisoryReason } from "../../screen/cloud-wait";

// No Tauri mock: the store owns no I/O, which is the point of it.

/** A supervisor tick, with the fields a caller cares about overridden. */
function tick(over: Partial<CloudSyncPayload> = {}): CloudSyncPayload {
  return {
    phase: "materializing",
    provider: "gdrive",
    provider_label: "Google Drive",
    total: 250,
    remaining: 100,
    downloaded: 150,
    unavailable_count: 0,
    unavailable: [],
    ...over,
  };
}

describe("cloud-sync store", () => {
  let seen: CloudSyncChange[];

  beforeEach(() => {
    cloudSync.reset();
    seen = [];
    cloudSync.subscribe((c) => seen.push(c));
  });

  describe("a file the provider will not hand over", () => {
    it("revisions every accepted supervisor snapshot and folder switch, but not build phases", () => {
      cloudSync.setFolder("/site/a");
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 4, unavailable: ["a.md"] }));
      const first = cloudSync.getUnavailableSnapshot();
      expect(first).toEqual({ folder: "/site/a", revision: expect.any(Number), count: 4 });
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 4, unavailable: ["a.md"] }));
      cloudSync.ingest(tick({ phase: "home_waiting" }));
      expect(cloudSync.getUnavailableSnapshot().revision).toBe(first.revision + 1);
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 4, unavailable: ["b.md"] }));
      expect(cloudSync.getUnavailableSnapshot().revision).toBeGreaterThan(first.revision);
      cloudSync.setFolder("/site/b");
      expect(cloudSync.getUnavailableSnapshot()).toMatchObject({ folder: "/site/b", count: 0 });
    });
    it("replaces a failure sample with an empty recovered supervisor snapshot", () => {
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 4, unavailable: ["a.md", "b.md", "c.md"] }));
      expect(cloudSync.getState().unavailable_count).toBe(4);
      cloudSync.ingest(tick({ phase: "materializing", unavailable_count: 0, unavailable: [] }));
      expect(cloudSync.getState().unavailable_count).toBe(0);
      expect(cloudSync.getState().unavailable).toEqual([]);
    });

    it("does not erase the supervisor failure snapshot on a build tick", () => {
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 4, unavailable: ["a.md", "b.md", "c.md"] }));
      cloudSync.ingest(tick({ phase: "home_waiting", unavailable_count: 0, unavailable: [] }));
      expect(cloudSync.getState().unavailable_count).toBe(4);
    });
    it("reports the full count when the wire samples only three names", () => {
      cloudSync.ingest(tick({ phase: "unavailable", unavailable_count: 7, unavailable: ["a.md", "b.md", "c.md"] }));
      const { unavailable_count, unavailable } = cloudSync.getState();
      const reason = unavailableAdvisoryReason(unavailable_count, unavailable);
      expect(reason).toContain("7");
      expect(reason).toContain("a.md");
      expect(reason).not.toContain("3 files");
    });
    it("does not turn a legacy bounded sample into an exact count", () => {
      const older = tick({ phase: "unavailable", unavailable: ["a.md", "b.md", "c.md"] });
      delete older.unavailable_count;
      cloudSync.ingest(older);
      const { unavailable_count, unavailable } = cloudSync.getState();
      expect(unavailable_count).toBeNull();
      expect(unavailableAdvisoryReason(unavailable_count, unavailable)).toContain("some files");
    });
    it("is carried by name, separately from the stall verdict", () => {
      cloudSync.ingest(
        tick({ phase: "unavailable", remaining: 1, downloaded: 249, unavailable_count: 1, unavailable: ["notes.md"] }),
      );

      const s = cloudSync.getState();
      expect(s.unavailable).toEqual(["notes.md"]);
      // A concrete read error and a long wait have different meanings.
      expect(s.stalled).toBe(false);
      expect(s.remaining).toBe(1);
    });

    it("the next supervisor snapshot replaces a recovered failure", () => {
      cloudSync.ingest(
        tick({ phase: "unavailable", remaining: 1, downloaded: 249, unavailable_count: 1, unavailable: ["notes.md"] }),
      );
      cloudSync.ingest(tick({ phase: "materializing", remaining: 4 }));

      expect(cloudSync.getState().unavailable).toEqual([]);
    });

    it("is dropped when the folder changes, like every other per-folder fact", () => {
      cloudSync.ingest(
        tick({ phase: "unavailable", remaining: 1, downloaded: 249, unavailable_count: 1, unavailable: ["notes.md"] }),
      );
      cloudSync.setFolder("/somewhere/else");

      expect(cloudSync.getState().unavailable).toEqual([]);
    });
  });

  describe("blocking vs remaining — what moss is downloading vs what the site is waiting for", () => {
    // Bookkeeping files such as `.moss/data/deployed-article-map.json` are rendered by no
    // page, so a pending one must not raise the full-window screen, which has no dismissal
    // control, over a site that is already served. `blocking` (unlike `remaining`) is scoped to
    // files the current render actually needs — `resolveUnshowableReason`
    // reads it, not `remaining`, to choose between rank 5 ("downloading")
    // and rank 7 ("building"); this store's only job is to carry the fact
    // straight through.
    it("a pending file nothing renders from is not counted as blocking", () => {
      cloudSync.ingest(tick({ remaining: 1, blocking: 0, downloaded: 249 }));

      const s = cloudSync.getState();
      expect(s.blocking).toBe(0);
      // …and the panel still says so honestly: moss really is fetching it.
      expect(s.remaining).toBe(1);
      expect(s.phase).toBe("materializing");
    });

    it("a pending file the render needs is counted as blocking", () => {
      cloudSync.ingest(tick({ remaining: 4, blocking: 1 }));
      expect(cloudSync.getState().blocking).toBe(1);
    });

    it("blocking drops to zero the moment the last blocking file lands, not the last file", () => {
      cloudSync.ingest(tick({ remaining: 4, blocking: 1 }));
      cloudSync.ingest(tick({ remaining: 3, blocking: 0 }));
      expect(cloudSync.getState().blocking).toBe(0);
    });

    it("a concrete failure does not erase a pending structural file", () => {
      cloudSync.ingest(tick({ remaining: 1, blocking: 1 }));
      expect(cloudSync.getState().blocking).toBe(1);

      cloudSync.ingest(
        tick({ phase: "unavailable", remaining: 1, blocking: 1, unavailable_count: 1, unavailable: ["style.css"] }),
      );
      expect(cloudSync.getState().blocking).toBe(1);
    });

    it("a tick that carries no blocking count means what it always meant", () => {
      // The build emits none — its own gate answers the same question — and
      // neither does any Rust that predates the field. Falling back to
      // `remaining` keeps both honest instead of silently reporting zero.
      cloudSync.ingest(tick({ remaining: 100, blocking: undefined }));
      expect(cloudSync.getState().blocking).toBe(100);
    });
  });

  describe("previewBlank is a pushed fact, not derived", () => {
    it("starts false and is set only by setPreviewBlank", () => {
      expect(cloudSync.getState().previewBlank).toBe(false);
      cloudSync.setPreviewBlank(true);
      expect(cloudSync.getState().previewBlank).toBe(true);
      cloudSync.setPreviewBlank(false);
      expect(cloudSync.getState().previewBlank).toBe(false);
    });

    it("lowerGate also clears it, so nothing arms the next tick over a message the caller just put up", () => {
      // The build-error path takes the window. Leaving `previewBlank` set
      // would arm `resolveUnshowableReason` again on the very next supervisor
      // tick, painting the waiting screen back over the error message.
      cloudSync.setPreviewBlank(true);
      cloudSync.lowerGate();
      expect(cloudSync.getState().previewBlank).toBe(false);
    });

    it("re-asserting previewBlank is not news", () => {
      cloudSync.setPreviewBlank(true);
      const before = seen.length;
      cloudSync.setPreviewBlank(true);
      expect(seen).toHaveLength(before);
    });
  });

  describe("the gate is derived, not pushed", () => {
    it("home_waiting raises it and home_ready lowers it", () => {
      cloudSync.ingest(tick({ phase: "home_waiting" }));
      expect(cloudSync.getState().gateUp).toBe(true);

      cloudSync.ingest(tick({ phase: "home_ready", remaining: 0 }));
      expect(cloudSync.getState().gateUp).toBe(false);
    });

    it("supervisor ticks do not move it either way", () => {
      cloudSync.ingest(tick({ phase: "home_waiting" }));
      cloudSync.ingest(tick({ phase: "materializing" }));
      cloudSync.ingest(tick({ phase: "stalled" }));
      expect(cloudSync.getState().gateUp).toBe(true);
    });

    it("lowerGate ends the episode for everyone, not just the caller", () => {
      // The build-error path: the preview takes the window to show a message,
      // and Rust never emits `home_ready`. If that stayed local to the manager
      // the store would still say gateUp and the progress panel — which is
      // silent while a reason holds the window — would never report the
      // download that is still running.
      cloudSync.ingest(tick({ phase: "home_waiting" }));
      cloudSync.lowerGate();
      expect(cloudSync.getState().gateUp).toBe(false);
    });

    it("lowering an already-down gate tells nobody", () => {
      cloudSync.lowerGate();
      expect(seen).toHaveLength(0);
    });
  });

  describe("the stall edge is computed once, for everyone", () => {
    it("only the first of a run of stalled ticks is news", () => {
      cloudSync.ingest(tick({ phase: "stalled" }));
      expect(seen.at(-1)!.stalledEdge).toBe(true);
      cloudSync.ingest(tick({ phase: "stalled" }));
      cloudSync.ingest(tick({ phase: "stalled" }));
      expect(seen.slice(1).every((c) => !c.stalledEdge)).toBe(true);
    });

    it("a resumed download and a second stall are two separate pieces of news", () => {
      cloudSync.ingest(tick({ phase: "stalled" }));
      cloudSync.ingest(tick({ phase: "materializing" }));
      expect(cloudSync.getState().stalled).toBe(false);
      cloudSync.ingest(tick({ phase: "stalled" }));
      expect(seen.at(-1)!.stalledEdge).toBe(true);
    });

    it("every subscriber is told the same thing about the same tick", () => {
      // The bug this design removes: two surfaces each deciding whether a tick
      // was news, and disagreeing.
      const other: CloudSyncChange[] = [];
      cloudSync.subscribe((c) => other.push(c));
      cloudSync.ingest(tick({ phase: "stalled" }));
      cloudSync.ingest(tick({ phase: "stalled" }));
      expect(other.map((c) => c.stalledEdge)).toEqual(seen.map((c) => c.stalledEdge));
    });
  });

  describe("provider is sticky", () => {
    it("a tick with no provider does not erase the known one", () => {
      cloudSync.ingest(tick());
      cloudSync.ingest(tick({ provider: undefined, provider_label: undefined }));
      expect(cloudSync.getState().provider).toEqual({ id: "gdrive", label: "Google Drive" });
    });
  });

  describe("counts", () => {
    it("the newest supervisor numbers win", () => {
      cloudSync.ingest(tick({ downloaded: 10, total: 250, remaining: 240 }));
      cloudSync.ingest(tick({ downloaded: 90, total: 250, remaining: 160 }));
      expect(cloudSync.getState()).toMatchObject({ downloaded: 90, total: 250, remaining: 160 });
    });

    it("the build's counts are a seed the supervisor overrides for good", () => {
      // The two producers do not share a denominator: the build counts what its
      // scan saw evicted, the supervisor counts what its sweep found, which is
      // strictly more. Letting the newest win outright makes "3 of 5" and
      // "3 of 12" alternate on screen every couple of seconds.
      cloudSync.ingest(tick({ phase: "home_waiting", downloaded: 3, total: 5, remaining: 2 }));
      expect(cloudSync.getState().total).toBe(5); // seed, before any tick

      cloudSync.ingest(tick({ downloaded: 3, total: 12, remaining: 9 }));
      expect(cloudSync.getState().total).toBe(12);

      // A later build re-asserting its own smaller number must not win.
      cloudSync.ingest(tick({ phase: "home_waiting", downloaded: 3, total: 5, remaining: 2 }));
      expect(cloudSync.getState().total).toBe(12);
    });

    it("a gate raise carrying no counts keeps the last known progress", () => {
      // The waiting screen it puts up has nowhere else to get a number, and
      // 0 of 0 reads as "nothing is happening" at the exact moment the user
      // most needs to see that something is.
      cloudSync.ingest(tick({ downloaded: 42, total: 250, remaining: 208 }));
      cloudSync.ingest(tick({ phase: "home_waiting", downloaded: 0, total: 0, remaining: 0 }));
      expect(cloudSync.getState()).toMatchObject({ downloaded: 42, total: 250, gateUp: true });
    });

    it("a navigation gate preserves the supervisor's provider", () => {
      cloudSync.ingest(tick());
      cloudSync.ingest(tick({
        phase: "home_waiting", provider: "unknown", provider_label: "cloud",
        downloaded: 0, total: 0, remaining: 0,
      }));
      expect(cloudSync.getState().provider).toEqual({ id: "gdrive", label: "Google Drive" });
    });
  });

  describe("folder scoping happens here, once", () => {
    it("another folder's news is dropped", () => {
      cloudSync.setFolder("/vault/a");
      cloudSync.ingest(tick({ folder: "/vault/b", phase: "home_waiting" }));
      expect(seen).toHaveLength(0);
      expect(cloudSync.getState().gateUp).toBe(false);
    });

    it("an event with no folder is accepted — older Rust builds omit it", () => {
      cloudSync.setFolder("/vault/a");
      cloudSync.ingest(tick({ folder: undefined }));
      expect(seen).toHaveLength(1);
    });

    it("switching folders forgets the previous folder's gate and counts", () => {
      cloudSync.setFolder("/vault/a");
      cloudSync.ingest(tick({ folder: "/vault/a", phase: "home_waiting" }));
      expect(cloudSync.getState().gateUp).toBe(true);

      cloudSync.setFolder("/vault/b");
      expect(cloudSync.getState().gateUp).toBe(false);
      expect(cloudSync.getState().downloaded).toBe(0);
    });
  });

  it("an unknown phase is dropped, not guessed at", () => {
    // A future Rust build teaching this event a new phase must not be able to
    // move the gate by accident.
    cloudSync.ingest(tick({ phase: "home_waiting" }));
    cloudSync.ingest(tick({ phase: "something-new" }));
    expect(cloudSync.getState().gateUp).toBe(true);
    expect(cloudSync.getState().phase).toBe("home_waiting");
  });

  it("one subscriber throwing does not stop the others being told", () => {
    const quiet = vi.spyOn(console, "warn").mockImplementation(() => {});
    cloudSync.subscribe(() => {
      throw new Error("boom");
    });
    const after: CloudSyncChange[] = [];
    cloudSync.subscribe((c) => after.push(c));
    cloudSync.ingest(tick());
    expect(after).toHaveLength(1);
    quiet.mockRestore();
  });

  describe("filesArrived — the edge the editor's pending pane waits on", () => {
    /**
     * The editor can be sitting on a "still downloading" pane for a
     * file in this very folder. It must re-resolve when bytes land and at no
     * other time — so the edge has to be a fact about THIS tick, not a level
     * the subscriber re-derives from a counter it kept its own copy of. That
     * re-derivation is exactly what this store exists to remove.
     */
    it("fires on the tick where the count went up, and not on a re-assertion", () => {
      cloudSync.ingest(tick({ downloaded: 150, remaining: 100 }));
      expect(seen.at(-1)?.filesArrived).toBe(true);

      cloudSync.ingest(tick({ downloaded: 150, remaining: 100 }));
      expect(seen.at(-1)?.filesArrived).toBe(false);

      cloudSync.ingest(tick({ downloaded: 151, remaining: 99 }));
      expect(seen.at(-1)?.filesArrived).toBe(true);
    });

    it("does not turn a build gate transition into a file arrival", () => {
      cloudSync.ingest(tick({ phase: "home_waiting", total: 0, downloaded: 0, remaining: 0 }));
      expect(seen.at(-1)?.filesArrived).toBe(false);

      cloudSync.ingest(tick({ phase: "home_ready", total: 0, downloaded: 0, remaining: 0 }));
      expect(seen.at(-1)?.filesArrived).toBe(false);
    });

    it("does not fire on a stall or a refusal — nothing arrived", () => {
      cloudSync.ingest(tick({ downloaded: 150 }));
      cloudSync.ingest(tick({ phase: "stalled", downloaded: 150 }));
      expect(seen.at(-1)?.filesArrived).toBe(false);
      cloudSync.ingest(tick({ phase: "unavailable", downloaded: 150, unavailable_count: 1, unavailable: ["away.md"] }));
      expect(seen.at(-1)?.filesArrived).toBe(false);
    });

    it("does not fire on a preview-blank push, which carries no news about the download", () => {
      cloudSync.ingest(tick({ downloaded: 150 }));
      cloudSync.setPreviewBlank(true);
      expect(seen.at(-1)?.filesArrived).toBe(false);
    });
  });
});
