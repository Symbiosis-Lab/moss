// Child-process lifecycle for the moss CLI. Owns spawn/kill and stream
// accumulation; all parsing is delegated to cli.ts. The spawn function is
// injected so unit tests drive it with a fake, and the e2e drives it with a
// shim script on PATH.
import type { ChildProcess } from "node:child_process";
import {
  parseBuildFailure,
  parseCliError,
  parseProblemCount,
  parseServerUrl,
  type ServerAddress,
} from "./cli";

/** The subset of child_process.spawn the runner needs. */
export type SpawnFn = (
  command: string,
  args: string[],
  options: { cwd?: string },
) => ChildProcess;

export interface RunCallbacks {
  /** The preview server announced its URL on stderr. */
  onServerReady?(addr: ServerAddress): void;
  /** A fatal `Build failed:` / `Error:` line appeared. */
  onError?(message: string): void;
  /** Non-fatal problem summary (`moss: N problems reported above …`). */
  onProblems?(count: number): void;
  /** Process exited. `code` is null when killed by signal. */
  onExit?(code: number | null): void;
  /** Raw stderr chunk, for logging. */
  onOutput?(chunk: string): void;
}

/**
 * One running moss CLI invocation. Build-only runs exit on their own;
 * `--serve` runs live until `stop()`.
 */
export class MossProcess {
  private child: ChildProcess | null = null;
  private stopRequested = false;
  private stderrBuf = "";
  private announcedServer = false;
  private announcedProblems = false;
  private announcedError = false;

  constructor(private spawnFn: SpawnFn) {}

  get running(): boolean {
    return this.child !== null;
  }

  /** Accumulated stderr so far (bounded — see append). */
  get output(): string {
    return this.stderrBuf;
  }

  start(binary: string, args: string[], cwd: string, callbacks: RunCallbacks): void {
    if (this.child) throw new Error("MossProcess already running");
    this.stopRequested = false;
    this.stderrBuf = "";
    this.announcedServer = false;
    this.announcedProblems = false;
    this.announcedError = false;

    const child = this.spawnFn(binary, args, { cwd });
    this.child = child;

    const onChunk = (data: unknown) => {
      const text = String(data);
      callbacks.onOutput?.(text);
      this.append(text);
      this.scan(callbacks);
    };
    child.stderr?.on("data", onChunk);
    // moss prints status on stderr, but scan stdout too so a future move of
    // any line to stdout does not silently blind the plugin.
    child.stdout?.on("data", onChunk);

    child.on("error", (err: Error) => {
      this.child = null;
      callbacks.onError?.(`Failed to start moss: ${err.message}`);
      callbacks.onExit?.(null);
    });
    child.on("exit", (code: number | null) => {
      this.child = null;
      // A SIGTERM'd process reports 143 (or a signal, code null); after a
      // deliberate stop() neither is an error, so report a clean shutdown.
      callbacks.onExit?.(this.stopRequested ? null : code);
    });
  }

  stop(): void {
    if (!this.child) return;
    const child = this.child;
    this.child = null;
    this.stopRequested = true;
    child.kill("SIGTERM");
  }

  private append(text: string): void {
    this.stderrBuf += text;
    // A --watch server can run for days; keep the tail only. 64k is far more
    // than any parse target needs and the URL line is re-checked before trim.
    if (this.stderrBuf.length > 65536) {
      this.stderrBuf = this.stderrBuf.slice(-32768);
    }
  }

  private scan(callbacks: RunCallbacks): void {
    if (!this.announcedServer) {
      const addr = parseServerUrl(this.stderrBuf);
      if (addr) {
        this.announcedServer = true;
        callbacks.onServerReady?.(addr);
      }
    }
    if (!this.announcedError) {
      const failure = parseBuildFailure(this.stderrBuf) ?? parseCliError(this.stderrBuf);
      if (failure) {
        this.announcedError = true;
        callbacks.onError?.(failure);
      }
    }
    if (!this.announcedProblems) {
      const n = parseProblemCount(this.stderrBuf);
      if (n > 0) {
        this.announcedProblems = true;
        callbacks.onProblems?.(n);
      }
    }
  }
}
