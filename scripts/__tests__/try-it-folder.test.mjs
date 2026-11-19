import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { zipFolder, withPicture } from '../../site/home/try-it/folder.js';

// Python's zipfile honours the UTF-8 name flag the way Windows Explorer does;
// on a Mac, ditto is what Finder's Archive Utility uses. Apple's unzip ignores
// the flag, so it is not the check.
test('the zip unpacks to the same folder, a Chinese name included', async () => {
  const picture = new Uint8Array([0xff, 0xd8, 0xff, 0x00, 0x10, 0x20]);
  const blob = await zipFolder('我的网站', [
    { path: 'index.md', data: '# 我的网站\n' },
    { path: 'heron.jpg', data: new Blob([picture]) },
  ]);
  const dir = mkdtempSync(join(tmpdir(), 'try-it-zip-'));
  const zip = join(dir, 'site.zip');
  writeFileSync(zip, new Uint8Array(await blob.arrayBuffer()));
  const extract = 'import sys, zipfile; z = zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; z.extractall(sys.argv[2])';
  execFileSync('python3', ['-I', '-c', extract, zip, dir]);
  assert.deepEqual(readdirSync(join(dir, '我的网站')).sort(), ['heron.jpg', 'index.md']);
  assert.equal(readFileSync(join(dir, '我的网站', 'index.md'), 'utf8'), '# 我的网站\n');
  assert.deepEqual(new Uint8Array(readFileSync(join(dir, '我的网站', 'heron.jpg'))), picture);
  if (process.platform === 'darwin') {
    const finder = join(dir, 'finder');
    execFileSync('ditto', ['-x', '-k', zip, finder]);
    assert.deepEqual(readdirSync(join(finder, '我的网站')).sort(), ['heron.jpg', 'index.md']);
  }
});

test('a picture is its own paragraph after the text', () => {
  assert.equal(withPicture('Words.\n\n', 'heron.jpg'), 'Words.\n\n![](heron.jpg)\n');
  assert.equal(withPicture('', 'heron.jpg'), '![](heron.jpg)\n');
});
