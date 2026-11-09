// formatVaultLocation is the wait screen's whole location-line contract: the
// home-abbreviated parent path arrives already computed from Rust
// (`describe_vault_location` in the host location resolver, via `redact_home_dir`)
// — this module only fits it into one quiet line and, where a cloud
// provider is detected, reads it as a friendly place rather than the
// technical container path underneath.
import { describe, it, expect } from "vitest";
import { formatVaultLocation, middleTruncate } from "../vault-location";

describe("middleTruncate", () => {
  it("returns short text unchanged", () => {
    expect(middleTruncate("~/Documents/Sites", 40)).toBe("~/Documents/Sites");
  });

  it("cuts the middle, keeping both ends", () => {
    const long = "~/Library/CloudStorage/GoogleDrive-someone@example.com/My Drive/Sites/blog";
    const result = middleTruncate(long, 30);
    expect(result.length).toBe(30);
    expect(result).toContain("…");
    expect(result.startsWith("~/Library")).toBe(true);
    expect(result.endsWith("blog")).toBe(true);
  });

  it("never returns text longer than the budget", () => {
    for (const max of [1, 5, 10, 40]) {
      expect(middleTruncate("a".repeat(100), max).length).toBeLessThanOrEqual(max);
    }
  });
});

describe("formatVaultLocation", () => {
  it("displays the home-abbreviated parent path when no cloud provider is detected", () => {
    const location = formatVaultLocation({
      folderName: "blog",
      parentDisplay: "~/Documents/Sites",
    });
    expect(location.display).toBe("~/Documents/Sites");
    expect(location.full).toBe("~/Documents/Sites");
  });

  it("middle-truncates the display but keeps the full string for the tooltip", () => {
    const parentDisplay = "~/Library/CloudStorage/GoogleDrive-someone@example.com/My Drive/Sites";
    const location = formatVaultLocation({ folderName: "blog", parentDisplay, maxLength: 24 });
    expect(location.display.length).toBe(24);
    expect(location.display).not.toBe(parentDisplay);
    expect(location.full).toBe(parentDisplay);
  });

  it("reads a detected cloud provider as a friendly breadcrumb, not the raw path", () => {
    // The design's own example: an Obsidian vault nested deep inside iCloud's
    // Mobile Documents container reads as "iCloud Drive › Obsidian Vault",
    // not the bundle-id path underneath.
    const location = formatVaultLocation({
      folderName: "Obsidian Vault",
      parentDisplay: "~/Library/Mobile Documents/iCloud~md~obsidian/Documents",
      cloudProviderLabel: "iCloud Drive",
    });
    expect(location.display).toBe("iCloud Drive › Obsidian Vault");
    expect(location.full).toBe("iCloud Drive › Obsidian Vault");
  });

  it("reads Google Drive the same way", () => {
    const location = formatVaultLocation({
      folderName: "blog",
      parentDisplay: "~/Library/CloudStorage/GoogleDrive-user@gmail.com/My Drive/Sites",
      cloudProviderLabel: "Google Drive",
    });
    expect(location.display).toBe("Google Drive › blog");
  });

  it("null cloud label falls back to the raw parent path", () => {
    // `cloud_provider_label` is `Option<String>` on the Rust side and arrives
    // as `null`, not merely absent — must be treated the same as absent.
    const location = formatVaultLocation({
      folderName: "blog",
      parentDisplay: "~/Documents/Sites",
      cloudProviderLabel: null,
    });
    expect(location.display).toBe("~/Documents/Sites");
  });
});
