import {
  Decoration,
  EditorView,
  hoverTooltip,
  ViewPlugin,
  type DecorationSet,
  type Tooltip,
  type ViewUpdate,
} from "@codemirror/view";
import { StateEffect, Text, type Extension } from "@codemirror/state";
import { editorInfoField } from "obsidian";
import type { EditorDiagnostic, MossSite } from "./core";
import { editorFieldSpans } from "./core";
import type { MossSchema } from "./schema";

export type SchemaProvider = () => MossSchema;
export type SiteProvider = () => MossSite;
export const contextChanged = StateEffect.define<void>();

export function diagnosticRanges(text: string, diagnostics: EditorDiagnostic[]) {
  const doc = Text.of(text.split("\n"));
  const fields = editorFieldSpans(text);
  return diagnostics.map((diagnostic) => {
    const field = fields.find((field) => field.key === diagnostic.path);
    const line = doc.line(Math.max(1, Math.min(doc.lines, diagnostic.line ?? 1)));
    return {
      diagnostic,
      from: field?.from ?? line.from,
      to: field?.to ?? line.to,
      line: !field,
      cls: `moss-diag-${diagnostic.severity === 1 ? "error" : diagnostic.severity === 2 ? "warning" : "hint"}`,
    };
  });
}

export function diagnosticsToDecorations(text: string, diagnostics: EditorDiagnostic[]): DecorationSet {
  return Decoration.set(diagnosticRanges(text, diagnostics).map((range) => {
    const spec = { class: range.cls };
    return range.line
      ? Decoration.line(spec).range(range.from)
      : Decoration.mark(spec).range(range.from, range.to);
  }), true);
}

function check(view: EditorView, site: MossSite): EditorDiagnostic[] {
  const path = view.state.field(editorInfoField, false)?.file?.path;
  return path ? JSON.parse(site.check(path, view.state.doc.toString())) : [];
}

function lintPlugin(getSite: SiteProvider) {
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    constructor(view: EditorView) { this.decorations = this.build(view); }
    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged || update.transactions.some((tr) =>
        tr.effects.some((effect) => effect.is(contextChanged)))) this.decorations = this.build(update.view);
    }
    private build(view: EditorView): DecorationSet {
      return diagnosticsToDecorations(view.state.doc.toString(), check(view, getSite()));
    }
  }, { decorations: (plugin) => plugin.decorations });
}

/** Tooltip body for a field the schema knows. Exported for tests. */
export function fieldInfoText(schema: MossSchema, name: string): string | null {
  const spec = schema.fields.get(name);
  if (spec) {
    const head = `${spec.name}: ${spec.type}${spec.group ? ` — ${spec.group}` : ""}`;
    return spec.description ? `${head}\n${spec.description}` : head;
  }
  return null;
}

function buildHover(getSchema: SchemaProvider, getSite: SiteProvider) {
  return hoverTooltip((view, pos): Tooltip | null => {
    const doc = view.state.doc.toString();
    const fields = editorFieldSpans(doc);
    const diags = diagnosticRanges(doc, check(view, getSite()));
    // A diagnostic under the pointer wins over the plain field explanation.
    const hit = diags.find((d) => pos >= d.from && pos <= d.to);
    const field = fields.find(
      (f) => pos >= f.from && pos <= view.state.doc.line(f.line).to,
    );
    if (!hit && !field) return null;

    const lines: string[] = [];
    if (hit) lines.push(hit.diagnostic.message);
    if (field) {
      const info = fieldInfoText(getSchema(), field.key);
      if (info) lines.push(info);
    }
    if (lines.length === 0) return null;

    const from = hit ? hit.from : field!.from;
    const to = hit ? hit.to : field!.to;
    return {
      pos: from,
      end: to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "moss-fm-tooltip";
        for (const text of lines) {
          const p = document.createElement("div");
          p.textContent = text;
          dom.appendChild(p);
        }
        return { dom };
      },
    };
  });
}

export interface FieldCompletion {
  name: string;
  type: string;
  description: string;
  /** Text to insert (includes the `: ` so the caret lands at the value). */
  insert: string;
}

/**
 * Field-name completions for a doc, minus fields already present and
 * internal (`_`-prefixed) ones. Pure — Obsidian's `EditorSuggest` wrapper in
 * field-suggest.ts renders these; completion deliberately does NOT go through
 * CM6 `autocompletion()`, which would race Obsidian's own suggest system.
 */
export function fieldCompletions(schema: MossSchema, doc: string, prefix: string): FieldCompletion[] {
  const present = new Set(editorFieldSpans(doc).map((f) => f.key));
  const out: FieldCompletion[] = [];
  const p = prefix.toLowerCase();
  for (const spec of schema.fields.values()) {
    if (present.has(spec.name)) continue;
    if (spec.name.startsWith("_")) continue; // internal fields
    if (p !== "" && !spec.name.toLowerCase().startsWith(p)) continue;
    out.push({
      name: spec.name,
      type: spec.type,
      description: spec.description,
      insert: `${spec.name}: `,
    });
  }
  return out;
}

/** The complete frontmatter knowledge extension (lint + hover). */
export function frontmatterExtension(getSchema: SchemaProvider, getSite: SiteProvider): Extension {
  return [lintPlugin(getSite), buildHover(getSchema, getSite)];
}
