import { describe, it, expect } from "vitest";
import fs from "node:fs"; import path from "node:path";
const css = fs.readFileSync(path.resolve(__dirname, "../styles/shared/toast.css"), "utf8");
describe("toast shelf position", () => {
  it("container is bottom-anchored", () => {
    const b = css.match(/#moss-toast-container\s*\{[^}]*\}/)?.[0] ?? "";
    expect(b).toMatch(/bottom:\s*20px/); expect(b).not.toMatch(/top:\s*20px/);
  });
  it("slide-in rises from the bottom", () => {
    const k = css.match(/@keyframes moss-toast-slide-in\s*\{[\s\S]*?\}\s*\}/)?.[0] ?? "";
    expect(k).not.toMatch(/translateY\(\s*-/);
  });
});
