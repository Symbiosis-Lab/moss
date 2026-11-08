/**
 * The open documents as moss's own session keeps them, in the desktop and a
 * browser: the editor's pending save is flushed through the editor, the
 * engine moves and renames, and the editor follows its file through the two
 * rename events it already listens for (`editor:file-renaming`,
 * `editor:file-renamed`). When the document record's session lands, it
 * replaces this behind the same port.
 */
import type { Documents } from "../../host";
import { engine } from '../../engine';

export interface SessionDocumentsDeps {
  /** The editor's own pending save. */
  flush(): Promise<unknown>;
  /** Where the editor hears the rename events: the tree's root while it is mounted. */
  events(): EventTarget | null;
}

export function sessionDocuments(deps: SessionDocumentsDeps): Documents {
  return {
    flush: () => deps.flush(),
    rename: (from, to) => engine.renameEntryWithRefs(from, to),
    move: (paths, toDir) => engine.moveEntries(paths, toDir),
    retarget: (oldPath, newPath, phase) => {
      deps.events()?.dispatchEvent(new CustomEvent(phase === 'starting' ? 'editor:file-renaming' : 'editor:file-renamed', {
        bubbles: true,
        detail: { oldPath, newPath },
      }));
    },
  };
}
