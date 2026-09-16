import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";
import type { ChildProcess } from "node:child_process";
import { MossProcess, type SpawnFn } from "../src/process";

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
    expect(spawn).toHaveBeenCalledWith("/bin/moss", ["build", "/v", "--serve"], { cwd: "/v" });
    expect(proc.running).toBe(true);
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
    expect(onServerReady).toHaveBeenCalledWith({ url: "http://localhost:8082", port: 8082 });
  });

  it("reports Build failed and problem summaries", () => {
    const { proc, child } = makeProcess();
    const onError = vi.fn();
    const onProblems = vi.fn();
    proc.start("/bin/moss", [], "/v", { onError, onProblems });
    child.stderr.emit("data", "moss: 3 problems reported above — the site was still generated.\n");
    child.stderr.emit("data", "Build failed: something broke\n");
    expect(onProblems).toHaveBeenCalledWith(3);
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
