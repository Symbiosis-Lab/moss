// postinstall: download the moss binary for this platform.
//
// Two things it will not do. It never fails the npm install — an offline
// machine or a proxy hiccup should not break an unrelated `npm install`, and
// the bin shim (bin/moss.js) retries the download lazily on first run, so
// exiting 0 here loses nothing. And it does not download at all when nobody
// asked for a release binary: inside the moss repo itself, or when the
// developer has pointed MOSS_BINARY_PATH at their own build.

'use strict';

const { fetchBinary, skipDownloadReason } = require('./lib/fetch.js');

const skip = skipDownloadReason();
if (skip) {
  console.log(`moss-npm: skipping the binary download — ${skip}.`);
  process.exit(0);
}

fetchBinary().catch((err) => {
  console.warn(`\nmoss-npm: could not download the moss binary now:\n  ${err.message}`);
  console.warn('moss-npm: will retry automatically the first time you run `moss`.\n');
  process.exit(0);
});
