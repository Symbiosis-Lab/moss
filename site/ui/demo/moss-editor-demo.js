// `<moss-editor-demo>`: the "editor" surface (site/ui/demo/surfaces/editor.js) built on
// DemoFrameElement. This is the whole element — everything else lives in the base
// (site/ui/demo/demo-frame.js) or the adapter (surfaces/editor.js); adding a second surface later
// (a `<moss-preview-demo>`, say) means a new surfaces/*.js and a file this size, not a change here.
import { DemoFrameElement } from './demo-frame.js';
import * as editorSurface from './surfaces/editor.js';

class MossEditorDemo extends DemoFrameElement {
  get surface() {
    return editorSurface;
  }
}

customElements.define('moss-editor-demo', MossEditorDemo);
