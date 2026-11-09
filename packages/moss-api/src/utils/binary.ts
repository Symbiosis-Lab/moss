/**
 * Binary execution for moss plugins
 *
 * Allows plugins to execute external binaries (git, npm, etc.)
 * in a controlled environment.
 *
 * The working directory is the project root unless `workingDir` names
 * another inside the project, or a deploy hook's `site_dir`.
 */

import { getTauriCore } from "./tauri.js";
import { getInternalContext } from "./context.js";
import { onEvent } from "./events.js";

// ============================================================================
// Types
// ============================================================================

/**
 * Options for executing a binary
 * @category Binary execution
 */
export interface ExecuteOptions {
  /**
   * The binary to run, named in the manifest's `requires` as
   * `execute_binary:<name>`. Pass the bare name to have it looked up on the
   * PATH. An absolute path is accepted only where moss itself would find that
   * binary: a PATH directory, a standard install directory such as
   * `/opt/homebrew/bin`, or the path `resolve_git_path` returned.
   */
  binaryPath: string;
  /** Arguments to pass to the binary */
  args: string[];
  /** Timeout in milliseconds (default: 60000) */
  timeoutMs?: number;
  /**
   * Environment variables to add. Only `GIT_TERMINAL_PROMPT`,
   * `COPYFILE_DISABLE` and `GIT_INDEX_FILE` are accepted; any other name
   * refuses the call. `GIT_INDEX_FILE` must be a deploy hook's
   * `context.index_file`.
   */
  env?: Record<string, string>;
  /** Data to pass to stdin (useful for commands like `git credential fill`) */
  stdin?: string;
  /**
   * Working directory: relative to the project root, or absolute, such as a
   * deploy hook's `context.site_dir` (default: the project root). moss refuses
   * one outside the project, or inside its `.moss/` folder other than
   * `site_dir`.
   */
  workingDir?: string;
  /**
   * Callback for real-time stderr output. When provided, stderr lines are
   * streamed from the Rust backend via Tauri events as they are produced.
   * Useful for long-running processes like `git push` where you want to
   * show progress to the user.
   */
  onStderr?: (line: string) => void;
}

/**
 * Result from binary execution
 * @category Binary execution
 */
export interface ExecuteResult {
  /** Whether the command succeeded (exit code 0) */
  success: boolean;
  /** Exit code from the process */
  exitCode: number;
  /** Standard output from the process */
  stdout: string;
  /** Standard error output from the process */
  stderr: string;
}

// ============================================================================
// Internal Types (Tauri response shape)
// ============================================================================

interface TauriBinaryResult {
  success: boolean;
  exit_code: number;
  stdout: string;
  stderr: string;
}

/** Payload shape emitted by Rust for real-time stderr streaming */
interface BinaryOutputEvent {
  streamId: string;
  line: string;
}

// ============================================================================
// Functions
// ============================================================================

/** A POSIX root, a Windows drive (`C:\` or `C:/`) or a UNC path. */
function isAbsolute(path: string): boolean {
  return /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(path);
}

/**
 * Execute an external binary
 *
 * The working directory is the project root unless `workingDir` names
 * another inside the project, or a deploy hook's `site_dir`.
 *
 * @param options - Execution options including binary path and args
 * @returns Execution result with stdout, stderr, and exit code
 * @throws Error if binary cannot be executed or called outside a hook
 *
 * @example
 * ```typescript
 * // Run git status
 * const result = await executeBinary({
 *   binaryPath: "git",
 *   args: ["status"],
 * });
 *
 * if (result.success) {
 *   console.log(result.stdout);
 * } else {
 *   console.error(result.stderr);
 * }
 * ```
 *
 * @example
 * ```typescript
 * // Push without ever waiting on a credential prompt
 * const result = await executeBinary({
 *   binaryPath: "git",
 *   args: ["push"],
 *   timeoutMs: 120000,
 *   env: { GIT_TERMINAL_PROMPT: "0" },
 * });
 * ```
 *
 * @category Binary execution
 */
export async function executeBinary(
  options: ExecuteOptions
): Promise<ExecuteResult> {
  const ctx = getInternalContext();
  const { binaryPath, args, timeoutMs = 60000, env, stdin, workingDir, onStderr } = options;

  const resolvedWorkingDir = !workingDir
    ? ctx.project_path
    : isAbsolute(workingDir)
      ? workingDir
      : `${ctx.project_path}/${workingDir}`;

  // When onStderr is provided, set up real-time stderr streaming
  const streamId = onStderr ? crypto.randomUUID() : undefined;
  let unlisten: (() => void) | undefined;

  if (onStderr && streamId) {
    unlisten = await onEvent<BinaryOutputEvent>("binary-output", (payload) => {
      if (payload.streamId === streamId) {
        onStderr(payload.line);
      }
    });
  }

  try {
    const result = await getTauriCore().invoke<TauriBinaryResult>(
      "execute_binary",
      {
        binaryPath,
        args,
        workingDir: resolvedWorkingDir,
        timeoutMs,
        env,
        stdinData: stdin,
        streamId,
      }
    );

    return {
      success: result.success,
      exitCode: result.exit_code,
      stdout: result.stdout,
      stderr: result.stderr,
    };
  } finally {
    if (unlisten) {
      unlisten();
    }
  }
}
