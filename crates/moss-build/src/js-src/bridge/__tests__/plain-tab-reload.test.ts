import { describe, it, expect, vi } from "vitest";
import { installPlainTabReload } from "../plain-tab-reload";

/** One browser: the streams its tabs opened, its lock and its channel between tabs. */
function browser() {
  const streams: { url: string; onopen: (() => void) | null; onmessage: (() => void) | null }[] = [];
  const waiting: (() => void)[] = [];
  let held = false;
  let channels: { onmessage: (() => void) | null }[] = [];
  let holder: object | null = null;
  let refuses = false;

  class EventSource {
    onopen: (() => void) | null = null;
    onmessage: (() => void) | null = null;
    constructor(public url: string) {
      streams.push(this);
    }
  }
  class BroadcastChannel {
    onmessage: (() => void) | null = null;
    constructor() {
      channels.push(this);
    }
    postMessage() {
      for (const other of channels) if (other !== this) other.onmessage?.();
    }
  }
  const locks = {
    request(_name: string, heldFor: () => Promise<never>) {
      if (refuses) return Promise.reject(new Error("refused"));
      const channel = channels[channels.length - 1];
      const take = () => {
        holder = channel;
        void heldFor();
      };
      if (held) waiting.push(take);
      else {
        held = true;
        take();
      }
      return new Promise(() => {});
    },
  };

  return {
    streams,
    /** Open the page in a tab and return the spy that stands for its reload. */
    tab({ secure = true } = {}) {
      const win = { EventSource, BroadcastChannel, navigator: secure ? { locks } : {} } as unknown as Window & typeof globalThis;
      const reload = vi.fn();
      installPlainTabReload(win, reload);
      return reload;
    },
    /** The tab holding the lock goes away, and hears nothing more. */
    closeHolder() {
      channels = channels.filter((channel) => channel !== holder);
      waiting.shift()?.();
    },
    /** The browser refuses every lock from now on. */
    refuseLocks() {
      refuses = true;
    },
  };
}

describe("a page in a plain tab", () => {
  it("reloads on each rebuild", () => {
    const { streams, tab } = browser();
    const reload = tab();
    expect(streams.map((s) => s.url)).toEqual(["/__moss/reload"]);
    streams[0].onopen?.();
    expect(reload).not.toHaveBeenCalled();
    streams[0].onmessage?.();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads when its stream reopens, since a rebuild may have been missed", () => {
    const { streams, tab } = browser();
    const reload = tab();
    streams[0].onopen?.();
    streams[0].onopen?.();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("holds its own stream where the browser offers no lock", () => {
    const { streams, tab } = browser();
    const reloads = [tab({ secure: false }), tab({ secure: false })];
    expect(streams).toHaveLength(2);
    streams[1].onmessage?.();
    expect(reloads.map((r) => r.mock.calls.length)).toEqual([0, 1]);
  });
});

describe("several plain tabs of one browser", () => {
  it("share one stream, and a rebuild reloads each of them once", () => {
    const { streams, tab } = browser();
    const reloads = [tab(), tab(), tab()];
    expect(streams).toHaveLength(1);
    streams[0].onmessage?.();
    expect(reloads.map((r) => r.mock.calls.length)).toEqual([1, 1, 1]);
  });

  it("pass the stream on when the tab holding it goes away, without reloading", () => {
    const { streams, tab, closeHolder } = browser();
    const reloads = [tab(), tab(), tab()];
    closeHolder();
    expect(streams).toHaveLength(2);
    expect(reloads.map((r) => r.mock.calls.length)).toEqual([0, 0, 0]);
    streams[1].onmessage?.();
    expect(reloads.map((r) => r.mock.calls.length)).toEqual([0, 1, 1]);
  });

  it("each hold their own stream when the browser refuses the lock", async () => {
    const { streams, tab, refuseLocks } = browser();
    refuseLocks();
    const reloads = [tab(), tab()];
    await Promise.resolve();
    expect(streams).toHaveLength(2);
    streams[0].onmessage?.();
    expect(reloads.map((r) => r.mock.calls.length)).toEqual([1, 0]);
  });
});
