// packages/moss-ui/src/ui/editor/__tests__/cm-editor-rescue.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { CmEditor } from '../cm-editor';

describe('CmEditor onBeforeDestroy', () => {
  beforeEach(() => {
    document.body.innerHTML = `
      <div id="moss-editor-root">
        <div class="editor-heading" id="editor-heading" data-empty="true"></div>
        <div id="moss-editor-cm"></div>
      </div>
    `;
  });

  it('runs the onBeforeDestroy callback before destroying the view', () => {
    const root = document.getElementById('moss-editor-root')!;
    const cmContainer = document.getElementById('moss-editor-cm')!;
    const heading = document.getElementById('editor-heading')!;

    // Simulate the editor lifecycle: heading gets re-parented into the
    // CM6 scroller during/after mount. Then destroy() should let us rescue
    // it before the scroller goes away.
    const editor = new CmEditor({
      content: 'hello',
      container: cmContainer,
      onChange: () => {},
      onBeforeDestroy: () => {
        const h = document.getElementById('editor-heading');
        if (h) root.appendChild(h);
      },
    });

    // Mimic the production prepend that puts heading inside scrollDOM.
    editor.getView().scrollDOM.prepend(heading);
    expect(heading.closest('.cm-scroller')).not.toBeNull();

    editor.destroy();

    // Heading must still exist in the DOM, now back under moss-editor-root.
    const survivor = document.getElementById('editor-heading');
    expect(survivor).not.toBeNull();
    expect(survivor!.parentElement).toBe(root);
  });
});
