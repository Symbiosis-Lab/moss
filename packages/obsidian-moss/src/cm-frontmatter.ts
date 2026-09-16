// CM6 glue for the frontmatter knowledge layer: lint decorations, hover
// explanations, and field-name completion. All analysis is pure
// (frontmatter.ts) — this file only maps results onto editor primitives, so
// vitest covers the logic and this stays thin.
//
// Works in Obsidian's source mode (and the raw-frontmatter portion of Live
// Preview when properties-as-text is enabled). Live Preview's Properties
// widget replaces the frontmatter text entirely; surfacing diagnostics inside
// that widget is out of CM6's reach and out of scope for v1.
import {
  Decoration,
  EditorView,
  hoverTooltip,
  ViewPlugin,
  type DecorationSet,
  type Tooltip,
  type ViewUpdate,
} from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import {
  parseFrontmatterBlock,
  validateFrontmatter,
  type Diagnostic,
} from "./frontmatter";
import type { MossSchema } from "./schema";
import { OBSIDIAN_NATIVE_FIELDS } from "./schema";

/** Late-bound schema so a CLI refresh reaches editors without re-mounting. */
export type SchemaProvider = () => MossSchema;

const DECO_CLASS: Record<Diagnostic["kind"], string> = {
  "unknown-field": "moss-fm-unknown",
  "wrong-type": "moss-fm-wrong-type",
  "not-in-enum": "moss-fm-wrong-type",
};

/** Pure: diagnostics → decoration set. Exported for tests. */
export function diagnosticsToDecorations(diags: Diagnostic[]): DecorationSet {
  const sorted = [...diags].sort((a, b) => a.from - b.from);
  return Decoration.set(
    sorted.map((d) =>
      Decoration.mark({
        class: DECO_CLASS[d.kind],
        attributes: { "data-moss-diagnostic": d.kind },
      }).range(d.from, d.to),
    ),
  );
}

/** Pure: compute diagnostics for a document. Exported for tests. */
export function computeDiagnostics(doc: string, schema: MossSchema): Diagnostic[] {
  const block = parseFrontmatterBlock(doc);
  if (!block) return [];
  return validateFrontmatter(block, schema);
}

function lintPlugin(getSchema: SchemaProvider) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = this.build(view);
      }

      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged) {
          this.decorations = this.build(u.view);
        }
      }

      private build(view: EditorView): DecorationSet {
        // Frontmatter is at the top of the file; skip all work when the doc
        // does not start with a fence.
        const doc = view.state.doc;
        if (doc.length < 4 || doc.sliceString(0, 4) !== "---\n") {
          return Decoration.none;
        }
        return diagnosticsToDecorations(computeDiagnostics(doc.toString(), getSchema()));
      }
    },
    { decorations: (v) => v.decorations },
  );
}

/** Tooltip body for a field the schema knows. Exported for tests. */
export function fieldInfoText(schema: MossSchema, name: string): string | null {
  const spec = schema.fields.get(name);
  if (spec) {
    const head = `${spec.name}: ${spec.type}${spec.group ? ` — ${spec.group}` : ""}`;
    return spec.description ? `${head}\n${spec.description}` : head;
  }
  if (OBSIDIAN_NATIVE_FIELDS.has(name)) {
    return `${name} is an Obsidian field. moss ignores it when publishing.`;
  }
  return null;
}

function buildHover(getSchema: SchemaProvider) {
  return hoverTooltip((view, pos): Tooltip | null => {
    const doc = view.state.doc.toString();
    const block = parseFrontmatterBlock(doc);
    if (!block) return null;
    const diags = validateFrontmatter(block, getSchema());
    // A diagnostic under the pointer wins over the plain field explanation.
    const hit = diags.find((d) => pos >= d.from && pos <= d.to);
    const field = block.fields.find(
      (f) => (pos >= f.keyFrom && pos <= f.keyTo) || (pos >= f.valueFrom && pos <= f.valueTo),
    );
    if (!hit && !field) return null;

    const lines: string[] = [];
    if (hit) lines.push(hit.message);
    if (field) {
      const info = fieldInfoText(getSchema(), field.name);
      if (info) lines.push(info);
    }
    if (lines.length === 0) return null;

    const from = hit ? hit.from : field!.keyFrom;
    const to = hit ? hit.to : field!.keyTo;
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
  const block = parseFrontmatterBlock(doc);
  const present = new Set(block?.fields.map((f) => f.name) ?? []);
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
export function frontmatterExtension(getSchema: SchemaProvider): Extension {
  return [lintPlugin(getSchema), buildHover(getSchema)];
}
