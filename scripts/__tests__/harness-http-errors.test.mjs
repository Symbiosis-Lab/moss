import { it } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolveBaseURL } from '../landing-harness.mjs';

async function withServer(run) {
  const root = await mkdtemp(join(tmpdir(), 'moss-harness-'));
  const { baseURL, close } = await resolveBaseURL(undefined, { root });
  try {
    await run(root, baseURL);
  } finally {
    await close();
    await rm(root, { recursive: true, force: true });
  }
}

it('landing-harness keeps HTTP error details in the local console', async (t) => {
  const reported = t.mock.method(console, 'error', () => {});
  await withServer(async (_root, baseURL) => {
    const response = await fetch(`${baseURL}%`);
    assert.equal(response.status, 500);
    assert.equal(await response.text(), 'Internal server error');
    assert.equal(reported.mock.calls.length, 1);
    assert.ok(reported.mock.calls[0].arguments[0] instanceof URIError);
  });
});

// WebKit decodes an MP4 only from a server that answers byte ranges.
it('landing-harness answers a byte range with the bytes asked for', async () => {
  await withServer(async (root, baseURL) => {
    await writeFile(join(root, 'clip.mp4'), '0123456789');
    const part = await fetch(`${baseURL}clip.mp4`, { headers: { Range: 'bytes=2-5' } });
    assert.equal(part.status, 206);
    assert.equal(part.headers.get('content-range'), 'bytes 2-5/10');
    assert.equal(await part.text(), '2345');
    const tail = await fetch(`${baseURL}clip.mp4`, { headers: { Range: 'bytes=7-' } });
    assert.equal(await tail.text(), '789');
    const whole = await fetch(`${baseURL}clip.mp4`);
    assert.equal(whole.status, 200);
    assert.equal(whole.headers.get('accept-ranges'), 'bytes');
  });
});
