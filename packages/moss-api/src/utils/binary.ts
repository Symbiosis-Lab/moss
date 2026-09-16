/**
 * Binary execution for moss plugins
 *
 * Allows plugins to execute external binaries (git, npm, etc.)
 * in a controlled environment.
 *
 * Working directory is auto-detected from the runtime context
 * (always the project root).
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
  /** Path to the binary (can be just the name if in PATH) */
  binaryPath: string;
  /** Arguments to pass to the binary */
  args: string[];
  /** Timeout in milliseconds (default: 60000) */
  timeoutMs?: number;
  /** Additional environment variables */
  env?: Record<string, string>;
  /** Data to pass to stdin (useful for commands like `git credential fill`) */
  stdin?: string;
  /** Working directory relative to project root (default: project root itself) */
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

/**
 * Execute an external binary
 *
 * Working directory is auto-detected from the runtime context
 * (always the project root).
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
 * // Run npm install with timeout
 * const result = await executeBinary({
 *   binaryPath: "npm",
 *   args: ["install"],
 *   timeoutMs: 120000,
 *   env: { NODE_ENV: "production" },
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

  const resolvedWorkingDir = workingDir
    ? `${ctx.project_path}/${workingDir}`
    : ctx.project_path;

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
