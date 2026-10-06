import { it } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

for (const name of ['site-check-harness', 'landing-harness']) {
  it(`${name} keeps HTTP error details in the local console`, async (t) => {
    const { resolveBaseURL } = await import(`../${name}.mjs`);
    const root = await mkdtemp(join(tmpdir(), 'moss-harness-error-'));
    const reported = t.mock.method(console, 'error', () => {});
    const { baseURL, close } = await resolveBaseURL(undefined, { root });
    try {
      const response = await fetch(`${baseURL}%`);
      assert.equal(response.status, 500);
      assert.equal(await response.text(), 'Internal server error');
      assert.equal(reported.mock.calls.length, 1);
      assert.ok(reported.mock.calls[0].arguments[0] instanceof URIError);
    } finally {
      await close();
      await rm(root, { recursive: true, force: true });
    }
  });
}
