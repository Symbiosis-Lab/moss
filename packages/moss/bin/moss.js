#!/usr/bin/env node
// Shim: run the vendored moss binary, downloading it first if it isn't
// there yet (covers `npm install --ignore-scripts` and a postinstall that
// failed offline).

'use strict';

const fs = require('node:fs');
const { spawnSync } = require('node:child_process');
const { fetchBinary, resolveBinary } = require('../lib/fetch.js');

async function main() {
  const binary = resolveBinary();

  if (!fs.existsSync(binary)) {
    // An explicit MOSS_BINARY_PATH that isn't there is a mistake to report,
    // not a cue to download a release binary over the top of the developer's
    // intent.
    if (process.env.MOSS_BINARY_PATH) {
      throw new Error(
        `moss-npm: MOSS_BINARY_PATH is set to ${binary}, but no file is there.`
      );
    }
    await fetchBinary();
  }

  // stderr is piped so a loader failure can be recognized, then replayed
  // verbatim. stdin/stdout stay inherited for interactive use.
  const result = spawnSync(binary, process.argv.slice(2), {
    stdio: ['inherit', 'inherit', 'pipe'],
  });

  const stderr = result.stderr ? result.stderr.toString() : '';
  if (stderr) process.stderr.write(stderr);

  if (result.error) throw result.error;

  if (stderr.includes('error while loading shared libraries')) {
    console.error(
      '\nmoss-npm: the moss binary currently needs the WebKitGTK runtime libraries:\n' +
        '  sudo apt install libwebkit2gtk-4.1-0\n' +
        'This is temporary — a moss CLI build without that linkage is planned.'
    );
  }

  process.exit(result.status === null ? 1 : result.status);
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
