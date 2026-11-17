// Preloaded into every Node process of a render gate run (NODE_OPTIONS=--require) when
// GATE_INPUTS_OUT is set: appends each path the process reads, lists or probes, and each
// command it starts, as `kind\tpath` lines. scripts/render-gate-select.mjs turns the
// lines into the gate's input set. The static server (tests/support/static-server.py)
// records what it serves the same way; moss-cli is covered by its binary.
'use strict';
const out = process.env.GATE_INPUTS_OUT;
if (out) {
  const fs = require('node:fs');
  const cp = require('node:child_process');
  const { fileURLToPath } = require('node:url');
  const append = fs.appendFileSync;
  const seen = new Set();
  const log = (kind, target) => {
    try {
      let p = target instanceof URL ? fileURLToPath(target) : target;
      if (typeof p !== 'string') return;
      const line = `${kind}\t${p}`;
      if (seen.has(line)) return;
      seen.add(line);
      append(out, line + '\n');
    } catch {
      // A path we cannot read as a string is a descriptor or buffer: not a file input.
    }
  };
  const wrap = (obj, name, kind) => {
    const orig = obj[name];
    if (typeof orig !== 'function') return;
    obj[name] = function (target, ...rest) {
      log(kind, target);
      return orig.call(this, target, ...rest);
    };
  };
  for (const n of ['readFileSync', 'readFile', 'createReadStream']) wrap(fs, n, 'read');
  for (const n of ['readFile']) wrap(fs.promises, n, 'read');
  for (const n of ['existsSync', 'statSync', 'lstatSync', 'stat', 'lstat', 'access', 'accessSync']) wrap(fs, n, 'exists');
  for (const n of ['stat', 'lstat', 'access']) wrap(fs.promises, n, 'exists');
  for (const n of ['readdirSync', 'readdir', 'opendirSync', 'opendir']) wrap(fs, n, 'dir');
  for (const n of ['readdir', 'opendir']) wrap(fs.promises, n, 'dir');
  for (const n of ['spawn', 'spawnSync', 'exec', 'execSync', 'execFile', 'execFileSync', 'fork']) {
    const orig = cp[n];
    cp[n] = function (cmd, ...rest) {
      const args = Array.isArray(rest[0]) ? ' ' + rest[0].join(' ') : '';
      log('exec', String(cmd) + args);
      return orig.call(this, cmd, ...rest);
    };
  }
}
