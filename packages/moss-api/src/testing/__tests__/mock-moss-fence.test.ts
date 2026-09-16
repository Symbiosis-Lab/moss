/**
 * Fixture-driven parity test for the `.moss/` sandbox fence mirror.
 *
 * The example cases — a plugin's own file, a sibling's, first-party
 * `review.json`, the legacy directory, no plugin identity — live once in
 * `open/fixtures/social-data-fence-cases.json`, read here AND by the Rust
 * `PluginPath::shared_social_data` test
 * (`open/crates/moss-build/src/vault/fs_tests.rs`). A hand-duplicated case
 * list in each language is exactly what let the mock enforce nothing for two
 * months while the real Rust guard rejected the Matters plugin's write —
 * this table is the fix for that class of drift, not just this one bug.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { describe, it, expect } from "vitest";
import { enforceMossFence } from "../mock-moss-fence.js";

interface FenceCase {
  pluginId: string | null;
  path: string;
  allowed: boolean;
  note: string;
}

const here = path.dirname(fileURLToPath(import.meta.url));
// __tests__ -> testing -> src -> moss-api -> packages -> open
const fixturePath = path.join(here, "..", "..", "..", "..", "..", "fixtures", "social-data-fence-cases.json");
const cases: FenceCase[] = JSON.parse(readFileSync(fixturePath, "utf-8"));

describe("enforceMossFence — shared fixture parity with PluginPath::shared_social_data", () => {
  it("loaded a non-empty fixture", () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  for (const { pluginId, path: relativePath, allowed, note } of cases) {
    it(`pluginId=${JSON.stringify(pluginId)} path=${relativePath} -> allowed=${allowed} (${note})`, () => {
      if (allowed) {
        expect(() => enforceMossFence(pluginId, relativePath)).not.toThrow();
      } else {
        expect(() => enforceMossFence(pluginId, relativePath)).toThrow(/Access to \.moss\/ is not allowed/);
      }
    });
  }
});
