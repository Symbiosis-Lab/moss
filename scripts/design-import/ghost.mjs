#!/usr/bin/env node
// A Ghost reference site for the design import judge: Ghost 6 from npm on
// 127.0.0.1, no Docker, with Casper or Source active, its accent colour and
// title font changed through the Admin API, and two posts whose text holds a
// link. A local proof, not a CI gate.
//
//   node scripts/design-import/ghost.mjs casper     prints the site's address, runs until Ctrl-C
//   node scripts/design-import/ghost.mjs source
//
// Then import it and judge the folder:
//   moss import <address> <folder> -r
//   node scripts/design-import/judge.mjs --reference <address> --folder <folder> --minimal
//
// Ghost is installed once into a cache under the system temp folder
// (GHOST_JUDGE_CACHE overrides) and reused; each run's content is deleted
// when it stops. Ghost 6 needs Node 22 or 24: the running Node when it fits,
// else GHOST_NODE, else the newest such Node under nvm.

import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";

const GHOST_VERSION = process.env.GHOST_VERSION || "6.75.0";
const OWNER = { name: "Judge", email: "judge@example.com", password: "Heron-ridge-5523-birch", blogTitle: "Field Notes Weekly" };

/** What each theme starts from: an accent and a title font that are not its defaults. */
const THEMES = {
  casper: { accent: "#b0302a", custom: { title_font: "Elegant serif" } },
  source: { accent: "#1f6f5c", custom: { title_font: "Elegant serif" } },
};

const POSTS = [
  { title: "First light on the ridge", html: '<p>The path climbs through birch and then opens onto the ridge. <a href="https://example.com/ridge">The trail map</a> shows the way.</p><h2>Notes</h2><p>The morning arrives all at once.</p>' },
  { title: "Counting herons on the river", html: "<p>Eleven herons in an hour, and one kingfisher that would not sit still.</p>" },
];

const fail = (message) => {
  process.stderr.write(`ghost: ${message}\n`);
  process.exit(1);
};

function ghostNode() {
  const fits = (version) => /^v?(22|24)\./.test(version);
  if (process.env.GHOST_NODE) return process.env.GHOST_NODE;
  if (fits(process.version)) return process.execPath;
  const nvm = join(process.env.NVM_DIR || join(homedir(), ".nvm"), "versions", "node");
  const major = (v) => Number(v.replace(/^v/, "").split(".")[0]);
  const found = existsSync(nvm) ? readdirSync(nvm).filter(fits).sort((a, b) => major(a) - major(b)).pop() : null;
  if (found) return join(nvm, found, "bin", "node");
  fail(`Ghost ${GHOST_VERSION} needs Node 22 or 24; this is ${process.version}. Set GHOST_NODE to one.`);
}

/** Ghost resolves its bundled parts relative to itself, so it is unpacked and installed in its own folder. */
function installGhost(node) {
  const root = join(process.env.GHOST_JUDGE_CACHE || join(tmpdir(), "moss-ghost-judge"), GHOST_VERSION);
  const ghost = join(root, "ghost");
  if (existsSync(join(root, "installed"))) return ghost;
  process.stderr.write(`Installing Ghost ${GHOST_VERSION} (once) into ${root}\n`);
  rmSync(root, { recursive: true, force: true });
  mkdirSync(ghost, { recursive: true });
  const env = { ...process.env, PATH: `${dirname(node)}:${process.env.PATH}` };
  const run = (cmd, args, cwd) => {
    if (spawnSync(cmd, args, { cwd, env, stdio: ["ignore", "ignore", "inherit"] }).status !== 0) fail(`${cmd} ${args.join(" ")} failed`);
  };
  const npm = join(dirname(node), "npm");
  run(npm, ["pack", `ghost@${GHOST_VERSION}`, "--silent"], root);
  run("tar", ["-xzf", `ghost-${GHOST_VERSION}.tgz`, "-C", ghost, "--strip-components=1"], root);
  run(npm, ["install", "--omit=dev", "--legacy-peer-deps", "--no-audit", "--no-fund", "--loglevel=error"], ghost);
  writeFileSync(join(root, "installed"), "");
  return ghost;
}

function freePort() {
  return new Promise((resolve) => {
    const server = createServer().listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/** The Admin API as Ghost's own admin calls it: same Origin, a session cookie. */
function admin(base) {
  let cookie = "";
  return async (method, path, body) => {
    const res = await fetch(new URL(`ghost/api/admin/${path}`, base), {
      method,
      headers: { Origin: base.origin, "Content-Type": "application/json", ...(cookie && { Cookie: cookie }) },
      body: body && JSON.stringify(body),
    });
    const session = res.headers.getSetCookie().find((c) => c.startsWith("ghost-admin-api-session="));
    if (session) cookie = session.split(";")[0];
    const text = await res.text();
    if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text.slice(0, 300)}`);
    return res.headers.get("content-type")?.includes("json") ? JSON.parse(text) : null;
  };
}

async function main() {
  const theme = process.argv[2] ?? "casper";
  const settings = THEMES[theme] ?? fail(`unknown theme ${theme}; casper or source`);
  const node = ghostNode();
  const ghost = installGhost(node);
  const port = await freePort();
  const base = new URL(`http://127.0.0.1:${port}/`);
  const content = mkdtempSync(join(tmpdir(), "ghost-judge-content-"));
  cpSync(join(ghost, "content"), content, { recursive: true });
  const child = spawn(node, [join(ghost, "index.js")], {
    cwd: ghost,
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      NODE_ENV: "development",
      url: base.href,
      server__host: "127.0.0.1",
      server__port: String(port),
      paths__contentPath: content,
      database__client: "better-sqlite3",
      database__connection__filename: join(content, "data", "ghost-judge.db"),
      security__staffDeviceVerification: "false",
      mail__transport: "stub",
    },
  });
  let stopped = false;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    try { process.kill(-child.pid, "SIGTERM"); } catch { /* already gone */ }
    rmSync(content, { recursive: true, force: true });
  };
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => { stop(); process.exit(0); });
  process.on("exit", stop);

  const deadline = Date.now() + 240_000;
  while (!(await fetch(new URL("ghost/api/admin/site/", base)).then((r) => r.ok, () => false))) {
    if (Date.now() > deadline || child.exitCode !== null) fail("Ghost did not start");
    await new Promise((r) => setTimeout(r, 500));
  }
  const call = admin(base);
  await call("POST", "authentication/setup/", { setup: [OWNER] });
  await call("POST", "session/", { username: OWNER.email, password: OWNER.password });
  const { posts } = await call("GET", "posts/?limit=all&fields=id");
  for (const { id } of posts) await call("DELETE", `posts/${id}/`);
  await call("PUT", `themes/${theme}/activate/`);
  await call("PUT", "settings/", { settings: [{ key: "accent_color", value: settings.accent }] });
  const { custom_theme_settings: current } = await call("GET", "custom_theme_settings/");
  const custom = current.map(({ id, key, value }) => ({ id, key, value: settings.custom[key] ?? value }));
  await call("PUT", "custom_theme_settings/", { custom_theme_settings: custom });
  for (const post of POSTS) await call("POST", "posts/?source=html", { posts: [{ ...post, status: "published" }] });
  await call("DELETE", "session/");
  process.stdout.write(`${base.href}\n`);
}

await main();
