import { describe, it, expect } from 'vitest';
import {
  routeHistoryKey,
  computeHistoryInputs,
  FORM_FIELD_SELECTOR,
  type HistoryRouteInput,
} from '../history-routing';

const base: HistoryRouteInput = { pendingEdit: false, nativeOwner: false };
const route = (over: Partial<HistoryRouteInput>) => routeHistoryKey({ ...base, ...over });

/**
 * This table is deliberately SMALL, and that is the change worth reading.
 *
 * It used to carry six surface-named inputs and five ordered cases, because the
 * router reasoned about the chip popover's and the heading's internals from the
 * outside. Three of the four defects review found lived in that gap — most
 * sharply, "the heading has focus" was standing in for "the heading holds text
 * the store has not seen", which is a different question with a different
 * answer in both directions.
 *
 * The surface-specific reasoning now lives with the surfaces
 * (`ChipBar.hasOpenEditOverlay`, `HeadingController.hasPendingEdit`) and the
 * mapping from environment to these two facts lives in editor-main, where it is
 * exercised through a booted editor in editor-keydown.test.ts. What is left
 * here is the decision itself, and a decision this small is one you can check
 * by reading.
 */
describe('who owns ⌘Z', () => {
  it('a pending edit outranks everything, including focus on nothing', () => {
    // A popover can hold focus nowhere — showEdit skips autofocus for the union
    // widget and there is no overlay-level Escape handler. Reaching past it
    // would revert an older change while the popover sat over the result with
    // an uncommitted snapshot; the same is true of a heading typed into and not
    // yet blurred.
    expect(route({ pendingEdit: true })).toBe('flush');
    expect(route({ pendingEdit: true, nativeOwner: true })).toBe('flush');
  });

  it('a field that owns its own history keeps it', () => {
    // execCommand fires a real input event, which re-runs the widget listener
    // and re-commits through the store — so native undo propagates rather than
    // stranding the DOM ahead of the file.
    expect(route({ nativeOwner: true })).toBe('native');
  });

  it('otherwise the document history is the one meant', () => {
    expect(route({})).toBe('document');
  });
});

/**
 * The half above is a decision you can check by reading. THIS half is not, and
 * it is where every defect actually lived — so the table that used to sit on
 * the decision now sits here instead.
 *
 * It is a table rather than four booted-editor tests because the booted
 * route is untestable in practice: `editor-keydown`'s
 * fixture has no heading, its CmEditor mock has no `hasFocus`, and its ChipBar
 * is null at boot, so four separate mutations of this mapping — including
 * deleting the CM6-body exclusion outright — left all 2447 editor tests green.
 */
describe('computeHistoryInputs — reading the two facts out of the DOM', () => {
  const el = (html: string) => {
    const d = document.createElement('div');
    d.innerHTML = html;
    return d.firstElementChild as HTMLElement;
  };
  const heading = () => el('<div contenteditable="plaintext-only" id="editor-heading"></div>');
  const base = {
    activeElement: null as Element | null,
    cmBodyHasFocus: false,
    pending: [] as boolean[],
    logOwned: [] as (Element | null)[],
  };
  const inputs = (over: Partial<typeof base>) => computeHistoryInputs({ ...base, ...over });

  it('the CM6 body is not a native owner, though its contentDOM is contenteditable', () => {
    // CM6's contentDOM matches FORM_FIELD_SELECTOR, so without the explicit
    // exclusion the body's ⌘Z goes to CodeMirror's own history and undoes the
    // BODY ALONE — the split this whole feature exists to remove.
    const body = el('<div contenteditable="true" class="cm-content"></div>');
    expect(inputs({ activeElement: body, cmBodyHasFocus: true }).nativeOwner).toBe(false);
    // Same element, focus elsewhere in the app: nothing special about it.
    expect(inputs({ activeElement: body, cmBodyHasFocus: false }).nativeOwner).toBe(true);
  });

  it('a log-owned field keeps the document history even while focused', () => {
    // The heading in Title mode. Its committed value IS a log entry, so a
    // native undo would rewind the DOM behind the log's back and then re-commit
    // the rewound text as a NEW entry on the next blur.
    const h = heading();
    expect(inputs({ activeElement: h, logOwned: [h] }).nativeOwner).toBe(false);
  });

  it('the same field NOT declared log-owned keeps its own history', () => {
    // The heading in Filename mode (a commit there is a rename, not expressible
    // as a log entry) and the pre-load state, where the mode is still a guess
    // and the safe guess is "not ours". Both arrive here as a `null` entry.
    const h = heading();
    expect(inputs({ activeElement: h, logOwned: [null] }).nativeOwner).toBe(true);
    expect(inputs({ activeElement: h, logOwned: [] }).nativeOwner).toBe(true);
  });

  it('a null entry does not match focus-on-nothing', () => {
    // `logOwned` carries nulls so the caller can map without branching; an
    // `includes(activeElement)` would make a null entry swallow the null
    // activeElement and quietly report an owner that does not exist.
    expect(inputs({ activeElement: null, logOwned: [null] }).nativeOwner).toBe(false);
  });

  it('a plain chip input is a native owner; the surrounding chrome is not', () => {
    expect(inputs({ activeElement: el('<input>') }).nativeOwner).toBe(true);
    expect(inputs({ activeElement: el('<button>x</button>') }).nativeOwner).toBe(false);
  });

  it('any pending surface makes the edit pending, and none of them makes it not', () => {
    expect(inputs({ pending: [false, false] }).pendingEdit).toBe(false);
    expect(inputs({ pending: [true, false] }).pendingEdit).toBe(true);
    expect(inputs({ pending: [false, true] }).pendingEdit).toBe(true);
    // Pending is read independently of focus — a chip popover can hold focus
    // nowhere, and a heading holds its pending text while focused.
    expect(inputs({ pending: [true], activeElement: el('<input>') })).toEqual({
      pendingEdit: true,
      nativeOwner: true,
    });
  });
});

describe('FORM_FIELD_SELECTOR', () => {
  it('matches the ordinary fields', () => {
    const el = (html: string) => {
      const d = document.createElement('div');
      d.innerHTML = html;
      return d.firstElementChild!;
    };
    expect(el('<input>').matches(FORM_FIELD_SELECTOR)).toBe(true);
    expect(el('<textarea></textarea>').matches(FORM_FIELD_SELECTOR)).toBe(true);
    expect(el('<div contenteditable="true"></div>').matches(FORM_FIELD_SELECTOR)).toBe(true);
    // Attribute-PRESENCE form: the editor heading is `plaintext-only` and does
    // not match `[contenteditable="true"]`. That bug exists elsewhere in the
    // editor (file-tree.ts's drag guard) and must not be copied here.
    expect(el('<div contenteditable="plaintext-only"></div>').matches(FORM_FIELD_SELECTOR)).toBe(
      true,
    );
    expect(el('<div contenteditable="false"></div>').matches(FORM_FIELD_SELECTOR)).toBe(false);
    expect(el('<div></div>').matches(FORM_FIELD_SELECTOR)).toBe(false);
  });
});
