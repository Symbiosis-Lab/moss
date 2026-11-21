import { StateField } from "@codemirror/state";
export const editorInfoField = StateField.define({ create: () => ({}), update: (value) => value });
export const editorLivePreviewField = StateField.define({ create: () => true, update: (value: boolean) => value });
