import type { FieldSpan } from "@symbiosis-lab/moss-ui";
import { initSync, MossSite, fieldSpans, siteBundle, siteTextPaths as textPaths } from "../wasm/moss_bindings";
import bytes from "../wasm/moss_bindings_bg.wasm";

let initialized = false;
export function initializeCore(): void {
  if (initialized) return;
  initSync({ module: bytes });
  initialized = true;
}

export type { EditorDiagnostic, PagePreview } from "@symbiosis-lab/moss-ui";

/** Convert the core's UTF-8 key spans to CodeMirror's UTF-16 coordinates. */
export function editorFieldSpans(text: string) {
  const spans: FieldSpan[] = JSON.parse(fieldSpans(text));
  const bytes = new TextEncoder().encode(text);
  const decoder = new TextDecoder("utf-8", { ignoreBOM: true });
  const offset = (byte: number) => decoder.decode(bytes.subarray(0, byte)).length;
  return spans.map((span) => ({ ...span, from: offset(span.from), to: offset(span.to) }));
}

/** The vault paths whose texts the site bundle reads; no other text is needed. */
export function siteTextPaths(files: string[]): string[] {
  return JSON.parse(textPaths(JSON.stringify(files)));
}

export { MossSite, fieldSpans, siteBundle };
