/**
 * The view-zone overlay backdrop must cover the FULL preview region, flush to the
 * divider seam, when an action panel is open — no undarkened strip of preview
 * hugging the divider. Asserted against the CSS source: jsdom doesn't
 * compute used `left`/`inset` from class rules, and the geometry is only correct
 * once composited with the native panel/preview webviews (device-verify covers
 * the pixels). See layout-zones.md I9.
 */
import { describe, it, expect } from 'vitest';
import * as fs from 'node:fs';
import * as path from 'node:path';

const CSS = fs.readFileSync(path.resolve(__dirname, '../style.css'), 'utf8');
// The BaseModal chrome (backdrop + the action-panel dialog override) lives in
// the shared modal stylesheet so both the shell and editor webviews get it.
const MODAL_CSS = fs.readFileSync(
  (import.meta.dirname + "/../styles/shared/modal.css"),
  'utf8',
);

describe('view-zone backdrop covers the full preview, flush to the divider seam', () => {
  it('the base backdrop covers its whole containing block (inset: 0)', () => {
    // The shell `body` is inset to the visible preview band (style.css
    // `contain: layout`), so an `inset: 0` backdrop contained by it paints the
    // full band flush to the seam — no per-side inset needed.
    const rule = MODAL_CSS.match(/\.moss-modal-backdrop\s*\{([^}]*)\}/);
    expect(rule, '.moss-modal-backdrop base rule must exist').not.toBeNull();
    expect(rule![1]).toMatch(/inset:\s*0/);
  });

  it('does NOT inset the backdrop by the divider width when an action panel is open', () => {
    // Regression lock. A `body.moss-action-panel-open .moss-modal-backdrop`
    // override that adds `--moss-divider-width` to `left` pushed the
    // scrim one divider-width past the seam and left that strip of preview
    // undarkened. The backdrop must take no such inset — the base `inset: 0`
    // already lands flush at the seam in both the tiled (editor) and inset-left
    // (browser-panel) placement models.
    const override = MODAL_CSS.match(
      /body\.moss-action-panel-open\s+\.moss-modal-backdrop\s*\{([^}]*)\}/,
    );
    if (override) {
      expect(
        override[1],
        'the action-panel backdrop override must not inset left by --moss-divider-width',
      ).not.toMatch(/--moss-divider-width/);
    }
  });

  it('re-centers the dialog box into the preview region by half the panel inset, with no divider bias', () => {
    // The dialog box (not the scrim) is re-centered into the visible preview
    // region: `left: calc(50% + --action-panel-width / 2)`. This is load-bearing
    // for the inset-left browser-panel case (recenters right of the plugin
    // webview); in the tiled case `--action-panel-width` is 0 and it collapses to
    // the base `left: 50%`. It must NOT carry a `--moss-divider-width` term — that
    // was the sibling of the backdrop's stale inset.
    const rule = MODAL_CSS.match(
      /body\.moss-action-panel-open\s+\.moss-modal\s*\{([^}]*)\}/,
    );
    expect(rule, 'body.moss-action-panel-open .moss-modal rule must exist').not.toBeNull();
    expect(rule![1]).toMatch(
      /left:\s*calc\(50%\s*\+\s*var\(--action-panel-width\)\s*\/\s*2\)/,
    );
    expect(
      rule![1],
      'the dialog-box override must not carry a --moss-divider-width bias',
    ).not.toMatch(/--moss-divider-width/);
  });

});
