// wdio-obsidian-service config: drives a REAL Obsidian (downloaded and
// cached under ~/.cache/wdio-obsidian) against a scratch copy of e2e/vault
// with this plugin installed from the package root.
//
// Hermetic by construction: e2e/shim/moss is prepended to PATH, so the
// plugin's CLI detection finds a shim that prints byte-identical output to
// the real `moss build --serve` (captured from 0.11.1) and serves a
// one-page site. No real moss binary, no network beyond the Obsidian
// download.
//
// On a headless Linux box run it under xvfb: `xvfb-run -a pnpm run test:e2e`.
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const shimDir = path.join(here, "shim");

// The Obsidian process inherits this environment; the plugin's PATH scan
// must find the shim first.
process.env.PATH = `${shimDir}${path.delimiter}${process.env.PATH ?? ""}`;

export const config: WebdriverIO.Config = {
  runner: "local",
  framework: "mocha",
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,

  capabilities: [
    {
      browserName: "obsidian",
      browserVersion: "latest",
      "wdio:obsidianOptions": {
        installerVersion: "latest",
        plugins: [path.join(here, "..")],
        vault: path.join(here, "vault"),
      },
    },
  ],

  services: ["obsidian"],
  reporters: ["obsidian"],

  cacheDir: process.env.OBSIDIAN_CACHE ?? path.join(os.homedir(), ".cache", "wdio-obsidian"),

  logLevel: "warn",
  waitforTimeout: 15000,
  // First run downloads Obsidian; be generous.
  connectionRetryTimeout: 240000,

  mochaOpts: {
    ui: "bdd",
    timeout: 180000,
  },
};
