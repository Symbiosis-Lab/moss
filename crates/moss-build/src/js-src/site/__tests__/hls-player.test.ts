/**
 * Tests for the hls.js half of adaptive playback (`hls-player.ts`), against a
 * fake `Hls` that records what the player asks of it. Three rules:
 *
 * - a video that is not playing does not load segments;
 * - a player created after the link was measured on real media starts from
 *   that estimate (init segments and alternate audio are not real media);
 * - a short clip buffered below what the link supports is refetched once;
 *
 * and the first player, before anything is measured, starts from the
 * browser's own estimate where it has one.
 */
import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

const hoisted = vi.hoisted(() => {
  type Handler = (event: string, data: unknown) => void;
  class FakeHls {
    static instances: FakeHls[] = [];
    static Events = {
      ERROR: "error",
      FRAG_BUFFERED: "fragBuffered",
      BUFFER_EOS: "bufferEos",
      FRAG_LOADING: "fragLoading",
      FRAG_LOADED: "fragLoaded",
    };
    static ErrorTypes = { NETWORK_ERROR: "network", MEDIA_ERROR: "media" };
    static isSupported = () => true;

    ctorConfig: Record<string, unknown>;
    config: Record<string, unknown>;
    handlers = new Map<string, Handler[]>();
    startLoad = vi.fn();
    levels = Array.from({ length: 6 }, (_, i) => ({ bitrate: 100_000 * (i + 1), details: { endSN: 1 } }));
    estimateSets: number[] = [];
    private estimate = NaN;
    /** What hls.js itself would have measured from a fragment. */
    measure(n: number) {
      this.estimate = n;
    }
    nextAutoLevel = -1;
    currentLevelSets: number[] = [];
    loadLevelSets: number[] = [];

    constructor(cfg: Record<string, unknown> = {}) {
      this.ctorConfig = cfg;
      this.config = { maxBufferLength: 30, testBandwidth: true, ...cfg };
      FakeHls.instances.push(this);
    }
    on(event: string, fn: Handler) {
      this.handlers.set(event, [...(this.handlers.get(event) ?? []), fn]);
    }
    emit(event: string, data: unknown) {
      for (const fn of this.handlers.get(event) ?? []) fn(event, data);
    }
    attachMedia = vi.fn();
    loadSource = vi.fn();
    destroy = vi.fn();
    recoverMediaError = vi.fn();
    get bandwidthEstimate() {
      return this.estimate;
    }
    set bandwidthEstimate(n: number) {
      this.estimate = n;
      this.estimateSets.push(n);
    }
    set currentLevel(n: number) {
      this.currentLevelSets.push(n);
    }
    set loadLevel(n: number) {
      this.loadLevelSets.push(n);
    }
  }
  return { FakeHls };
});

vi.mock("hls.js", () => ({ default: hoisted.FakeHls }));

const { FakeHls } = hoisted;
type Player = typeof import("../hls-player");
let player: Player;

beforeEach(async () => {
  FakeHls.instances = [];
  vi.resetModules(); // the page-wide estimate is module state
  player = await import("../hls-player");
});

function video(attrs: { preload?: string; autoplay?: boolean } = {}, duration = 8) {
  const v = document.createElement("video");
  if (attrs.preload) v.preload = attrs.preload;
  if (attrs.autoplay) v.setAttribute("autoplay", "");
  let bufferedEnd = 0;
  Object.defineProperty(v, "duration", { get: () => duration });
  Object.defineProperty(v, "buffered", {
    get: () => ({ length: bufferedEnd ? 1 : 0, end: () => bufferedEnd }),
  });
  return Object.assign(v, { setBuffered: (n: number) => (bufferedEnd = n) });
}

const media = (level: number, sn = 1) => ({ frag: { type: "main", sn, level } });

/** A player that has buffered one real fragment, with the link measured at `bps`. */
function measured(bps: number) {
  const v = video({ preload: "metadata" });
  player.attach(v, "m.m3u8");
  const hls = FakeHls.instances.at(-1)!;
  hls.measure(bps);
  hls.emit("fragBuffered", media(0));
  return hls;
}

describe("a clip that is not playing does not load", () => {
  test("preload=none waits for the play event", () => {
    const v = video({ preload: "none" });
    player.attach(v, "m.m3u8");
    const hls = FakeHls.instances[0];
    expect(hls.ctorConfig.autoStartLoad).toBe(false);
    expect(hls.startLoad).not.toHaveBeenCalled();
    v.dispatchEvent(new Event("play"));
    v.dispatchEvent(new Event("play"));
    expect(hls.startLoad).toHaveBeenCalledTimes(1);
  });

  test("raising preload to auto also starts it", async () => {
    const v = video({ preload: "none" });
    player.attach(v, "m.m3u8");
    const hls = FakeHls.instances[0];
    v.preload = "auto";
    await vi.waitFor(() => expect(hls.startLoad).toHaveBeenCalledTimes(1));
  });

  test("autoplay and preload=metadata load as before", () => {
    player.attach(video({ preload: "none", autoplay: true }), "a.m3u8");
    player.attach(video({ preload: "metadata" }), "b.m3u8");
    for (const hls of FakeHls.instances) {
      expect(hls.ctorConfig.autoStartLoad).toBeUndefined();
      expect(hls.startLoad).not.toHaveBeenCalled();
    }
  });
});

describe("later players start from what earlier ones learned", () => {
  test("a real fragment's estimate is the next player's starting estimate", () => {
    const first = measured(8_000_000);
    expect(first.ctorConfig.abrEwmaDefaultEstimate).toBeUndefined();
    player.attach(video({ preload: "metadata" }), "b.m3u8");
    // Through the constructor: a later assignment is overwritten when the
    // manifest loads.
    expect(FakeHls.instances[1].ctorConfig.abrEwmaDefaultEstimate).toBe(8_000_000);
    expect(FakeHls.instances[1].estimateSets).toEqual([]);
  });

  test("init-segment and alternate-audio fragments are not recorded", () => {
    const v = video({ preload: "metadata" });
    player.attach(v, "a.m3u8");
    const first = FakeHls.instances[0];
    first.measure(900_000);
    first.emit("fragBuffered", { frag: { type: "main", sn: "initSegment", level: 0 } });
    first.emit("fragBuffered", { frag: { type: "audio", sn: 1, level: 0 } });
    player.attach(video({ preload: "metadata" }), "b.m3u8");
    expect(FakeHls.instances[1].ctorConfig.abrEwmaDefaultEstimate).toBeUndefined();
  });

  test("a deferred player attached before the measurement still uses it when it starts", () => {
    const v = video({ preload: "none" });
    player.attach(v, "b.m3u8");
    const deferred = FakeHls.instances[0];
    measured(6_000_000);
    v.dispatchEvent(new Event("play"));
    expect(deferred.estimateSets).toEqual([6_000_000]);
    expect(deferred.startLoad).toHaveBeenCalledTimes(1);
  });
});

describe("the browser's own estimate starts the first player", () => {
  /** `navigator.connection` is a prototype getter in Chromium; shadow it on the instance. */
  function connection(value: unknown) {
    Object.defineProperty(navigator, "connection", { value, configurable: true });
  }
  afterEach(() => {
    Reflect.deleteProperty(navigator, "connection");
  });
  const first = () => {
    player.attach(video({ preload: "metadata" }), "m.m3u8");
    return FakeHls.instances.at(-1)!.ctorConfig;
  };

  test("downlink in megabits becomes 0.8 of that in bits per second", () => {
    connection({ downlink: 10 });
    expect(first().abrEwmaDefaultEstimate).toBe(8_000_000);
  });

  test("a downlink under 1 Mbps, an unusable one, or saveData leaves hls.js's default", () => {
    for (const c of [
      { downlink: 0.4 },
      { downlink: 0 },
      { downlink: NaN },
      { downlink: undefined },
      { downlink: 10, saveData: true },
    ]) {
      connection(c);
      expect(first()).not.toHaveProperty("abrEwmaDefaultEstimate");
    }
  });

  test("without the API nothing changes", () => {
    connection(undefined);
    expect(first()).not.toHaveProperty("abrEwmaDefaultEstimate");
  });

  test("a measured page estimate wins over the browser's", () => {
    connection({ downlink: 10 });
    measured(3_000_000);
    expect(first().abrEwmaDefaultEstimate).toBe(3_000_000);
  });
});

describe("an optimistic start from the browser's figure is given up if it stalls", () => {
  function connection(value: unknown) {
    Object.defineProperty(navigator, "connection", { value, configurable: true });
  }
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    Reflect.deleteProperty(navigator, "connection");
  });
  const loading = (duration = 6) => ({ frag: { type: "main", sn: 1, level: 5, duration } });
  function browserStarted() {
    connection({ downlink: 10 });
    player.attach(video({ preload: "metadata" }), "m.m3u8");
    return FakeHls.instances.at(-1)!;
  }

  test("no fragment within the deadline: one restart on the lowest level, then auto", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(2999);
    expect(hls.loadLevelSets).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(hls.loadLevelSets).toEqual([0]);
    expect(hls.estimateSets, "the wrong estimate is replaced by the lowest rung's").toEqual([100_000]);
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(10_000);
    expect(hls.loadLevelSets).toEqual([0]);
    hls.emit("fragLoaded", loading());
    expect(hls.loadLevelSets).toEqual([0, -1]);
  });

  test("the deadline is half a short fragment's duration", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading(4));
    vi.advanceTimersByTime(1999);
    expect(hls.loadLevelSets).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(hls.loadLevelSets).toEqual([0]);
  });

  test("the error raised by interrupting the load does not release the restart", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(3000);
    hls.emit("error", { fatal: false });
    expect(hls.loadLevelSets).toEqual([0]);
  });

  test("a fragment that arrives in time causes no restart", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    hls.emit("fragLoaded", loading());
    vi.advanceTimersByTime(10_000);
    expect(hls.loadLevelSets).toEqual([]);
  });

  test("an init segment does not start or stop the clock", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", { frag: { type: "main", sn: "initSegment", level: 5, duration: 0 } });
    vi.advanceTimersByTime(10_000);
    expect(hls.loadLevelSets).toEqual([]);
  });

  test("a measured page estimate or the default start sets no timer", () => {
    measured(3_000_000);
    connection({ downlink: 10 });
    player.attach(video({ preload: "metadata" }), "b.m3u8");
    Reflect.deleteProperty(navigator, "connection");
    player.attach(video({ preload: "metadata" }), "c.m3u8");
    for (const hls of FakeHls.instances.slice(1)) {
      hls.emit("fragLoading", loading());
      expect(vi.getTimerCount()).toBe(0);
    }
  });

  test("a deferred player re-seeded with a measured estimate is left alone", () => {
    connection({ downlink: 10 });
    const v = video({ preload: "none" });
    player.attach(v, "m.m3u8");
    const hls = FakeHls.instances.at(-1)!;
    measured(3_000_000); // another player records a measurement first
    v.dispatchEvent(new Event("play"));
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(10_000);
    expect(hls.loadLevelSets).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  test("destroying the player cancels the guard without writing to it", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(3000);
    for (let i = 0; i < 3; i++) hls.emit("error", { fatal: true, type: "network" });
    expect(hls.destroy).toHaveBeenCalled();
    expect(hls.loadLevelSets, "no release on the way out").toEqual([0]);
  });

  test("after the restart the refetch at the low level does not switch up", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    vi.advanceTimersByTime(3000);
    hls.nextAutoLevel = 0; // the reset estimate keeps selection low
    hls.emit("fragLoaded", loading());
    hls.emit("fragBuffered", { frag: { type: "main", sn: 1, level: 0 } });
    expect(hls.loadLevelSets).toEqual([0, -1]);
    expect(hls.currentLevelSets).toEqual([]);
  });

  test("an error, or the fatal-error fallback, clears the timer", () => {
    const hls = browserStarted();
    hls.emit("fragLoading", loading());
    hls.emit("error", { fatal: false });
    vi.advanceTimersByTime(10_000);
    expect(hls.loadLevelSets).toEqual([]);

    const second = browserStarted();
    second.emit("fragLoading", loading());
    for (let i = 0; i < 3; i++) second.emit("error", { fatal: true, type: "network" });
    expect(second.destroy).toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a short clip buffered low is refetched once", () => {
  test("upgrades to the level the estimate supports, then returns to auto", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.measure(20_000_000);
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0));
    hls.emit("fragBuffered", media(0));
    expect(hls.currentLevelSets).toEqual([3]);
    expect(hls.loadLevelSets).toEqual([]);

    // The refetched clip arrives whole at the new level: automatic again, and
    // the same estimate does not ask for the same step twice.
    hls.emit("fragBuffered", media(3));
    expect(hls.loadLevelSets).toEqual([-1]);
    hls.emit("fragBuffered", media(3));
    hls.emit("fragBuffered", media(2));
    expect(hls.currentLevelSets).toEqual([3]);
  });

  test("released on the last fragment even if the buffer end falls short of duration", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0));
    v.setBuffered(7.5);
    hls.emit("fragBuffered", media(3, 1));
    expect(hls.loadLevelSets).toEqual([-1]);
  });

  test("not released by a fragment that is not the last", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0));
    hls.emit("fragBuffered", media(3, 0));
    expect(hls.loadLevelSets).toEqual([]);
  });

  test("released at end of stream", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0));
    hls.emit("bufferEos", {});
    expect(hls.loadLevelSets).toEqual([-1]);
  });

  test("released on an error while pinned, and not when nothing is pinned", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.emit("error", { fatal: false });
    expect(hls.loadLevelSets).toEqual([]);
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0));
    hls.emit("error", { fatal: false });
    expect(hls.loadLevelSets).toEqual([-1]);
  });

  test("a low estimate causes no switch", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 0;
    hls.emit("fragBuffered", media(0));
    expect(hls.currentLevelSets).toEqual([]);
  });

  test("never switches down", () => {
    const v = video({ preload: "metadata" }, 8);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 1;
    hls.emit("fragBuffered", media(2));
    expect(hls.currentLevelSets).toEqual([]);
  });

  test("a long clip is left alone until its last fragment is buffered", () => {
    const v = video({ preload: "metadata" }, 120);
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    hls.nextAutoLevel = 3;
    hls.emit("fragBuffered", media(0, 0));
    expect(hls.currentLevelSets).toEqual([]);
    hls.emit("fragBuffered", media(0, 1)); // the last: now whole
    expect(hls.currentLevelSets).toEqual([3]);
  });
});

describe("upgradeTarget", () => {
  const base = { floor: 0, best: 2, duration: 8, whole: false, bufferTarget: 30 };
  test("short clip qualifies before it is whole", () => {
    expect(player.upgradeTarget(base)).toBe(2);
  });
  test("long clip needs to be whole", () => {
    expect(player.upgradeTarget({ ...base, duration: 90 })).toBeNull();
    expect(player.upgradeTarget({ ...base, duration: 90, whole: true })).toBe(2);
  });
  test("equal or lower best, or unknown duration, gives nothing", () => {
    expect(player.upgradeTarget({ ...base, best: 0 })).toBeNull();
    expect(player.upgradeTarget({ ...base, floor: 3 })).toBeNull();
    expect(player.upgradeTarget({ ...base, duration: NaN })).toBeNull();
  });
});

describe("clean-up", () => {
  test("after the fatal-error fallback, play and preload no longer start loading", async () => {
    const v = video({ preload: "none" });
    v.dataset.mossMp4 = "clip.mp4";
    v.load = vi.fn();
    player.attach(v, "a.m3u8");
    const hls = FakeHls.instances[0];
    for (let i = 0; i < 3; i++) hls.emit("error", { fatal: true, type: "network" });
    expect(hls.destroy).toHaveBeenCalled();
    v.dispatchEvent(new Event("play"));
    v.preload = "auto";
    await new Promise((r) => setTimeout(r, 20));
    expect(hls.startLoad.mock.calls.length).toBe(2); // the two network-error recoveries only
  });
});
