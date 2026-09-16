/**
 * Platform detection utilities for moss plugins
 *
 * Detects the current operating system and architecture to enable
 * platform-specific binary downloads and operations.
 */

import { executeBinary } from "./binary.js";

// ============================================================================
// Types
// ============================================================================

/**
 * Supported operating systems
 * @category Platform
 */
export type OSType = "darwin" | "linux" | "windows";

/**
 * Supported architectures
 * @category Platform
 */
export type ArchType = "arm64" | "x64";

/**
 * Platform key combining OS and architecture
 * @category Platform
 */
export type PlatformKey =
  | "darwin-arm64"
  | "darwin-x64"
  | "linux-x64"
  | "windows-x64";

/**
 * Complete platform information
 * @category Platform
 */
export interface PlatformInfo {
  /** Operating system */
  os: OSType;
  /** CPU architecture */
  arch: ArchType;
  /** Combined platform key for binary selection */
  platformKey: PlatformKey;
}

// ============================================================================
// Cached Detection
// ============================================================================

let cachedPlatform: PlatformInfo | null = null;

// ============================================================================
// Functions
// ============================================================================

/**
 * Detect the current platform (OS and architecture)
 *
 * Uses system commands to detect the platform:
 * - On macOS/Linux: `uname -s` for OS, `uname -m` for architecture
 * - On Windows: Falls back to environment variables and defaults
 *
 * Results are cached after the first call.
 *
 * @returns Platform information including OS, architecture, and combined key
 * @throws Error if platform detection fails or platform is unsupported
 *
 * @example
 * ```typescript
 * const platform = await getPlatformInfo();
 * console.log(platform.platformKey); // "darwin-arm64"
 * ```
 * @category Platform
 */
export async function getPlatformInfo(): Promise<PlatformInfo> {
  // Return cached result if available
  if (cachedPlatform) {
    return cachedPlatform;
  }

  const os = await detectOS();
  const arch = await detectArch(os);

  // Validate supported platform combinations
  const platformKey = `${os}-${arch}` as PlatformKey;
  const supportedPlatforms: PlatformKey[] = [
    "darwin-arm64",
    "darwin-x64",
    "linux-x64",
    "windows-x64",
  ];

  if (!supportedPlatforms.includes(platformKey)) {
    throw new Error(
      `Unsupported platform: ${platformKey}. ` +
        `Supported platforms: ${supportedPlatforms.join(", ")}`
    );
  }

  cachedPlatform = { os, arch, platformKey };
  return cachedPlatform;
}

/**
 * Clear the cached platform info
 *
 * Useful for testing or when platform detection needs to be re-run.
 *
 * @internal
 * @category Platform
 */
export function clearPlatformCache(): void {
  cachedPlatform = null;
}

// ============================================================================
// Internal Detection Functions
// ============================================================================

/**
 * Detect the operating system
 */
async function detectOS(): Promise<OSType> {
  try {
    // Try uname first (works on macOS and Linux)
    const result = await executeBinary({
      binaryPath: "uname",
      args: ["-s"],
      timeoutMs: 5000,
    });

    if (result.success) {
      const osName = result.stdout.trim().toLowerCase();
      if (osName === "darwin") {
        return "darwin";
      }
      if (osName === "linux") {
        return "linux";
      }
    }
  } catch {
    // uname not available, likely Windows
  }

  // Check for Windows via cmd
  try {
    const result = await executeBinary({
      binaryPath: "cmd",
      args: ["/c", "ver"],
      timeoutMs: 5000,
    });

    if (result.success && result.stdout.toLowerCase().includes("windows")) {
      return "windows";
    }
  } catch {
    // Not Windows either
  }

  throw new Error(
    "Unable to detect operating system. " +
      "Supported systems: macOS (Darwin), Linux, Windows"
  );
}

/**
 * Detect the CPU architecture
 */
async function detectArch(os: OSType): Promise<ArchType> {
  if (os === "windows") {
    // On Windows, check PROCESSOR_ARCHITECTURE environment variable
    try {
      const result = await executeBinary({
        binaryPath: "cmd",
        args: ["/c", "echo", "%PROCESSOR_ARCHITECTURE%"],
        timeoutMs: 5000,
      });

      if (result.success) {
        const arch = result.stdout.trim().toLowerCase();
        if (arch === "arm64") {
          return "arm64";
        }
        // AMD64, x86_64, etc. -> x64
        return "x64";
      }
    } catch {
      // Default to x64 on Windows
      return "x64";
    }
  }

  // Unix-like systems (macOS, Linux)
  try {
    const result = await executeBinary({
      binaryPath: "uname",
      args: ["-m"],
      timeoutMs: 5000,
    });

    if (result.success) {
      const machine = result.stdout.trim().toLowerCase();
      if (machine === "arm64" || machine === "aarch64") {
        return "arm64";
      }
      if (machine === "x86_64" || machine === "amd64") {
        return "x64";
      }
      // Fallback for other architectures
      if (machine.includes("arm")) {
        return "arm64";
      }
      return "x64";
    }
  } catch {
    // Fallback
  }

  // Default to x64 if detection fails
  return "x64";
}
