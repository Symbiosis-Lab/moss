// Port assignment for every render-gate webServer.
//
// Each checkout derives its own port block from a hash of its repo root
// path, into the 20000-29999 range (see RANGE_START/RANGE_SIZE below for
// why that range) — so two worktrees running gates at the same time land
// on different ports without coordinating, and a gate's `.config.ts` and
// its `.spec.ts` (ui-accent-seam needs both) agree on a port from the same
// checkout path alone.
//
// Override with MOSS_GATE_PORT_BASE to pin an exact base.
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// FNV-1a: small, dependency-free, and spreads similar strings (two sibling
// worktree paths differing only in their trailing segment) across the full
// output range rather than clustering them.
function fnv1a(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

// 20000-29999: clear of privileged ports, of the common dev-server ports
// (3000, 5173, 8080, ...) a stray process on the machine might already
// hold, and of both Linux's and macOS's default ephemeral-client-port
// ranges (32768+ and 49152+ respectively).
const RANGE_START = 20000;
const RANGE_SIZE = 10000;

function computeBase(): number {
  const override = process.env.MOSS_GATE_PORT_BASE;
  if (override !== undefined) {
    const parsed = Number(override);
    if (!Number.isInteger(parsed)) {
      throw new Error(`MOSS_GATE_PORT_BASE must be an integer, got ${JSON.stringify(override)}`);
    }
    return parsed;
  }
  return RANGE_START + (fnv1a(repoRoot) % RANGE_SIZE);
}

const BASE = computeBase();

// One slot per port a gate needs, named once. ui-accent-seam needs two
// (its two scratch sites are served simultaneously, so they must never
// collide with each other, not just across worktrees). Append new keys at
// the end — reordering existing ones just churns which literal port a gate
// gets, for no benefit, since uniqueness (not any particular value) is the
// property that matters.
const GATE_PORT_KEYS = [
  'customization-cascade',
  'comments-cascade',
  'no-important-cascade',
  'dark-layer-order',
  'heading-weight',
  'reading-scale-order',
  'pre-paint-dark',
  'ui-accent-seam:default',
  'ui-accent-seam:override',
  'nav-toggle-cluster',
  'nav-mobile',
  'grid-mobile-collapse',
  'card-cover-ratio',
  'grid-card-image-inline-size',
  'hero-caption',
  'content-width-escape',
  'hero-tone',
  'heading-anchor',
  'footnote-target',
  'share-card',
  'notebook-loads',
  'nav-island',
  'edge-clamp',
  'vertical-nav-chrome',
] as const;

type GatePortKey = (typeof GATE_PORT_KEYS)[number];

const OFFSETS: Map<string, number> = new Map(GATE_PORT_KEYS.map((key, index) => [key, index]));

/** This checkout's port for the given gate slot. Stable across runs; differs across worktrees. */
export function gatePort(key: GatePortKey): number {
  const offset = OFFSETS.get(key);
  if (offset === undefined) {
    throw new Error(`gate-ports: unknown key ${JSON.stringify(key)} (add it to GATE_PORT_KEYS)`);
  }
  return BASE + offset;
}
