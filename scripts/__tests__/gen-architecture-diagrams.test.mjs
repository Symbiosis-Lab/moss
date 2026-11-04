// The diagram check goes red on a crate, path or item the code lacks and on a
// block pele cannot parse, and the crate graph is a function of the metadata.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeResolver, problemsIn, renderCrateGraph, replaceGenerated } from '../gen-architecture-diagrams.mjs';

const resolves = makeResolver(['moss-core', 'moss-build']);
const doc = (body) => '```mermaid\nflowchart TB\n' + body + '\n```\n';

test('names that exist pass and prose is not checked', () => {
  const md = doc(
    [
      '  a["moss-build<br/>the build"]:::crate',
      '  b["crates/moss-build/src/build/ports"]:::path',
      '  c{{"HostPorts"}}:::symbol',
      '  d["the hosting service"]',
    ].join('\n'),
  );
  assert.deepEqual(problemsIn(md, resolves), []);
});

test('a missing crate, path or item goes red', () => {
  const md = doc(['  a["moss-wire"]:::crate', '  b["crates/moss-wire"]:::path', '  c["BuildInputs"]:::symbol'].join('\n'));
  assert.equal(problemsIn(md, resolves).length, 3);
});

test('an untracked path, or one leaving the repository, goes red', () => {
  const md = doc(['  a["node_modules/pele"]:::path', '  b["../moss"]:::path', '  c["crates/*"]:::path'].join('\n'));
  assert.equal(problemsIn(md, resolves).length, 3);
});

test('a block pele cannot parse goes red', () => {
  assert.match(problemsIn(doc('  a[ --> '), resolves)[0], /pele cannot parse/);
});

test('the crate graph draws workspace crates and their normal edges only', () => {
  const meta = {
    packages: [
      { name: 'moss-core', dependencies: [{ name: 'moss-build', kind: 'dev' }] },
      { name: 'moss-build', dependencies: [{ name: 'moss-core', kind: null }, { name: 'serde', kind: null }] },
    ],
  };
  const block = renderCrateGraph(meta);
  assert.match(block, /moss_build --> moss_core/);
  assert.doesNotMatch(block, /serde|moss_core --> moss_build/);
  const stale = 'x\n<!-- architecture:crate-graph:start -->\nold\n<!-- architecture:crate-graph:end -->\ny';
  assert.equal(replaceGenerated(stale, block), `x\n${block}\ny`);
  assert.deepEqual(problemsIn(block, resolves), []);
});
