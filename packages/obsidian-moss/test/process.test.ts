import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";
import type { ChildProcess } from "node:child_process";
import { MossProcess, type SpawnFn } from "../src/desktop/process";

class FakeChild extends EventEmitter {
  stderr = new EventEmitter();
  stdout = new EventEmitter();
  killed: string | null = null;
  kill(signal: string): boolean {
    this.killed = signal;
    this.emit("exit", null);
    return true;
  }
}

function makeProcess(): { proc: MossProcess; child: FakeChild; spawn: ReturnType<typeof vi.fn> } {
  const child = new FakeChild();
  const spawn = vi.fn(() => child as unknown as ChildProcess);
  return { proc: new MossProcess(spawn as unknown as SpawnFn), child, spawn };
}

describe("MossProcess", () => {
  it("spawns with the given binary, args and cwd", () => {
    const { proc, spawn } = makeProcess();
    proc.start("/bin/moss", ["build", "/v", "--serve"], "/v", {});
    expect(spawn).toHaveBeenCalledWith("/bin/moss", ["build", "/v", "--serve"], expect.objectContaining({ cwd: "/v" }));
    expect(proc.running).toBe(true);
  });

  it("asks moss to stop when Obsidian is gone, except on Windows", () => {
    const platform = Object.getOwnPropertyDescriptor(process, "platform")!;
    try {
      for (const [name, asked] of [["darwin", "1"], ["linux", "1"], ["win32", undefined]] as const) {
        Object.defineProperty(process, "platform", { value: name });
        const { proc, spawn } = makeProcess();
        proc.start("/bin/moss", [], "/v", {});
        const env = spawn.mock.calls[0][2].env;
        expect([name, env.MOSS_STOP_ON_STDIN_CLOSE, env.PATH]).toEqual([name, asked, process.env.PATH]);
      }
    } finally {
      Object.defineProperty(process, "platform", platform);
    }
  });

  it("announces the server once, from chunked stderr", () => {
    const { proc, child } = makeProcess();
    const onServerReady = vi.fn();
    proc.start("/bin/moss", [], "/v", { onServerReady });
    child.stderr.emit("data", "🌐 Preview server ready! Access at http://local");
    expect(onServerReady).not.toHaveBeenCalled();
    child.stderr.emit("data", "host:8082\n");
    child.stderr.emit("data", "more output mentioning Access at http://localhost:8082\n");
    expect(onServerReady).toHaveBeenCalledTimes(1);
    expect(onServerReady).toHaveBeenCalledWith({ url: "http://localhost:8082" });
  });

  it("follows a second Access at line, and a re-scan of the same buffer repeats nothing", () => {
    const { proc, child } = makeProcess();
    const onServerReady = vi.fn();
    proc.start("/bin/moss", [], "/v", { onServerReady });
    child.stderr.emit("data", "🌐 Preview server ready! Access at http://localhost:8081\n");
    child.stderr.emit("data", "Press Ctrl+C to stop the server\n");
    expect(onServerReady).toHaveBeenCalledTimes(1);
    expect(onServerReady).toHaveBeenNthCalledWith(1, { url: "http://localhost:8081" });
    child.stderr.emit("data", "🌐 Preview server ready! Access at http://localhost:8090\n");
    expect(onServerReady).toHaveBeenCalledTimes(2);
    expect(onServerReady).toHaveBeenNthCalledWith(2, { url: "http://localhost:8090" });
  });

  it("reports Build failed", () => {
    const { proc, child } = makeProcess();
    const onError = vi.fn();
    proc.start("/bin/moss", [], "/v", { onError });
    child.stderr.emit("data", "Build failed: something broke\n");
    expect(onError).toHaveBeenCalledWith("something broke");
  });

  it("reports spawn failure (binary vanished) as an error + exit", () => {
    const { proc, child } = makeProcess();
    const onError = vi.fn();
    const onExit = vi.fn();
    proc.start("/bin/moss", [], "/v", { onError, onExit });
    child.emit("error", new Error("ENOENT"));
    expect(onError).toHaveBeenCalledWith("Failed to start moss: ENOENT");
    expect(onExit).toHaveBeenCalledWith(null);
    expect(proc.running).toBe(false);
  });

  it("stop() kills the child and reports exit", () => {
    const { proc, child } = makeProcess();
    const onExit = vi.fn();
    proc.start("/bin/moss", [], "/v", { onExit });
    proc.stop();
    expect(child.killed).toBe("SIGTERM");
    expect(onExit).toHaveBeenCalledWith(null);
    expect(proc.running).toBe(false);
  });

  it("a deliberate stop() masks the SIGTERM exit code (143 is not an error)", () => {
    const { proc, child } = makeProcess();
    const onExit = vi.fn();
    proc.start("/bin/moss", [], "/v", { onExit });
    // Simulate a child that turns SIGTERM into a numeric exit status, the
    // way a shell script (and the real CLI) does.
    child.kill = function (this: FakeChild) {
      this.emit("exit", 143);
      return true;
    } as typeof child.kill;
    proc.stop();
    expect(onExit).toHaveBeenCalledWith(null);
  });

  it("a stopped child's late exit and output leave the next run alone", () => {
    const first = new FakeChild();
    first.kill = function (this: FakeChild, signal: string) {
      this.killed = signal;
      return true;
    } as typeof first.kill;
    const second = new FakeChild();
    const spawn = vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(second);
    const proc = new MossProcess(spawn as unknown as SpawnFn);
    const firstExit = vi.fn();
    const onServerReady = vi.fn();
    proc.start("/bin/moss", [], "/a", { onExit: firstExit });
    proc.stop();
    proc.start("/bin/moss", [], "/b", { onServerReady });
    first.stderr.emit("data", "🌐 Preview server ready! Access at http://localhost:8081\n");
    first.emit("exit", 143);
    expect(firstExit).toHaveBeenCalledWith(null);
    expect(onServerReady).not.toHaveBeenCalled();
    expect(proc.running).toBe(true);
    proc.stop();
    expect(second.killed).toBe("SIGTERM");
  });

  it("refuses a second start while running", () => {
    const { proc } = makeProcess();
    proc.start("/bin/moss", [], "/v", {});
    expect(() => proc.start("/bin/moss", [], "/v", {})).toThrow(/already running/);
  });

  it("bounds the stderr buffer on long watch runs", () => {
    const { proc, child } = makeProcess();
    proc.start("/bin/moss", [], "/v", {});
    for (let i = 0; i < 100; i++) child.stderr.emit("data", "x".repeat(1000) + "\n");
    expect(proc.output.length).toBeLessThanOrEqual(65536);
  });
});
