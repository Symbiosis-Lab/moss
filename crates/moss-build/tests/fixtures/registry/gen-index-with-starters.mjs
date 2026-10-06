// Regenerates index-with-starters.json by running moss-registry's own index
// builder (toEntry, buildStarterEntries, assembleIndex) over its real starter
// manifests, so the fixture is what the builder emits and not what we think
// it emits.
//
//   node gen-index-with-starters.mjs <path-to-moss-registry-checkout> index-with-starters.json
//
// Plugin rows are described by the four published plugin manifests below
// (pinned copies of the fields the builder reads). Archive bytes are synthetic
// and deterministic, so the sha256/size fields are stable but are not the
// hashes of any real release.
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";
import { join } from "node:path";

const registry = process.argv[2];
const out = process.argv[3];
if (!registry || !out) {
  console.error("usage: node gen-index-with-starters.mjs <moss-registry checkout> <out.json>");
  process.exit(2);
}
const b = await import(pathToFileURL(join(registry, ".github/scripts/build-index.mjs")).href);

const sha = (s) => createHash("sha256").update(s).digest("hex");
const asset = (tag, name) => ({ name, browser_download_url: `https://example.invalid/${tag}/${name}`, url: `https://api.invalid/${tag}/${name}` });
const base = { author: "moss team", repository: "https://github.com/Symbiosis-Lab/moss-registry" };
const plugins = [
  { id: "github", version: "1.6.1", display_name: "GitHub", description: "Deploy to GitHub Pages via GitHub Actions", capabilities: [], min_moss_version: "0.11.7", requires: ["execute_binary:git", "execute_binary:sh"] },
  { id: "ipfs", version: "0.1.0", display_name: "IPFS", description: "Publish your site to IPFS", capabilities: ["deploy"], min_moss_version: "0.7.23", requires: ["execute_binary:ipfs"] },
  { id: "matters", version: "1.5.5", display_name: "Matters", description: "Syndicate your articles to Matters.town", capabilities: ["process"], min_moss_version: "0.11.7", preview: true },
  { id: "onionpress", version: "0.4.1", display_name: "onionpress", description: "Publish moss sites to a self-hosted receiver", capabilities: ["deploy"], requires: ["execute_binary:curl"], requires_stack: true },
];
const entries = plugins.map((m) => {
  const tag = `${m.id}-v${m.version}`;
  const zip = asset(tag, `${m.id}-${m.version}.zip`);
  return b.toEntry({ id: m.id, version: m.version, tag, zip, icon: asset(tag, `${m.id}-${m.version}-icon.svg`) }, { ...base, ...m, name: m.id }, { sha256: sha(zip.name), sizeBytes: 1000 + zip.name.length });
});

const starters = ["essays", "organisation", "vertical"];
const manifests = Object.fromEntries(starters.map((id) => [id, JSON.parse(readFileSync(join(registry, "starters", id, "manifest.json"), "utf8"))]));
const releases = starters.map((id) => {
  const v = manifests[id].version;
  const tag = `starter-${id}-v${v}`;
  return { tag_name: tag, draft: false, prerelease: false, assets: [`${id}-${v}.zip`, `${id}-${v}-preview.zip`, `${id}-${v}.json`].map((n) => asset(tag, n)) };
});
const bytes = (a) => Buffer.from(`synthetic bytes of ${a.name}`.repeat(a.name.endsWith("preview.zip") ? 40 : 10));
const sidecars = Object.fromEntries(starters.map((id) => {
  const v = manifests[id].version;
  return [`${id}-${v}.json`, { preview_moss_version: "0.15.4", source_sha256: sha(bytes({ name: `${id}-${v}.zip` })), preview_sha256: sha(bytes({ name: `${id}-${v}-preview.zip` })) }];
}));
const starterEntries = await b.buildStarterEntries(releases, {
  fetchAsset: async (a) => ({ path: a.name, bytes: a.name.endsWith(".json") ? Buffer.from(JSON.stringify(sidecars[a.name])) : bytes(a) }),
  readManifest: (p) => manifests[p.split("-")[0]],
  warn: (m) => { throw new Error(m); },
});
if (starterEntries.length !== starters.length) throw new Error("a starter was skipped");

// The builder prints progress on stdout, so the index goes to a file.
writeFileSync(out, JSON.stringify(b.assembleIndex([...entries, ...starterEntries], { serial: 39 }), null, 2) + "\n");
