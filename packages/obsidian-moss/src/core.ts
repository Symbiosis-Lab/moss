import { initSync, MossSite, builtinSchemaJson } from "../wasm/moss_bindings";
import bytes from "../wasm/moss_bindings_bg.wasm";

let initialized = false;
export function initializeCore(): void {
  if (initialized) return;
  initSync({ module: bytes });
  initialized = true;
}

export interface EditorDiagnostic {
  severity: 1 | 2 | 3 | 4;
  message: string;
  path?: string | null;
  line?: number | null;
  column?: number | null;
}

export interface PagePreview {
  revision: string;
  html: string;
  title: string;
  diagnostics: EditorDiagnostic[];
}

export { MossSite, builtinSchemaJson };
