import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const output = fileURLToPath(new URL("../wasm/", import.meta.url));
const result = spawnSync("wasm-pack", [
  "build", "crates/moss-bindings", "--release", "--target", "web",
  "--out-dir", output, "--no-pack", "--", "--features", "wasm",
], { cwd: root, stdio: "inherit" });
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
