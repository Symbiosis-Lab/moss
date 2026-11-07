import { StateField } from "@codemirror/state";
export const editorInfoField = StateField.define({ create: () => ({}), update: (value) => value });
