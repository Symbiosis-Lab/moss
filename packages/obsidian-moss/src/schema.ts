// The moss frontmatter schema, sourced from `moss describe --json`.
//
// `describe --json` is the live contract (field names and types change
// between releases — moss's own guidance says to read it rather than rely on
// memory). The plugin ships a captured snapshot (describe-snapshot.json,
// moss 0.14.1) so knowledge features work with zero binaries installed, and
// refreshes it from the real CLI when one is found.
//
// Captured shape (describe_schema_version 6): top-level `frontmatter` is an
// array of { name, type, widget, description, group, skip_schema,
// enum_values?, default? } where type ∈ string | integer | boolean | array |
// object | one_of.

export type FieldType = "string" | "integer" | "boolean" | "array" | "object" | "one_of";

export interface FieldSpec {
  name: string;
  type: FieldType;
  description: string;
  group: string;
  /** Present for one_of fields with a closed value set (e.g. sort). */
  enumValues?: string[];
  default?: string;
}

export interface MossSchema {
  /** moss binary version the schema came from ("bundled" suffix when from snapshot). */
  source: string;
  fields: Map<string, FieldSpec>;
}

interface RawField {
  name?: unknown;
  type?: unknown;
  description?: unknown;
  group?: unknown;
  enum_values?: unknown;
  default?: unknown;
}

const FIELD_TYPES: ReadonlySet<string> = new Set([
  "string",
  "integer",
  "boolean",
  "array",
  "object",
  "one_of",
]);

/**
 * Parse `moss describe --json` output into a schema.
 * Throws on malformed input — callers fall back to the bundled snapshot.
 */
export function parseDescribeJson(text: string, sourceLabel?: string): MossSchema {
  const doc = JSON.parse(text) as { moss_binary_version?: unknown; frontmatter?: unknown };
  const raw = doc.frontmatter;
  if (!Array.isArray(raw)) {
    throw new Error("describe --json: missing frontmatter array");
  }
  const fields = new Map<string, FieldSpec>();
  for (const entry of raw as RawField[]) {
    if (typeof entry?.name !== "string" || entry.name === "") continue;
    const type = typeof entry.type === "string" && FIELD_TYPES.has(entry.type)
      ? (entry.type as FieldType)
      : "string";
    const spec: FieldSpec = {
      name: entry.name,
      type,
      description: typeof entry.description === "string" ? entry.description : "",
      group: typeof entry.group === "string" ? entry.group : "",
    };
    if (Array.isArray(entry.enum_values) && entry.enum_values.every((v) => typeof v === "string")) {
      spec.enumValues = entry.enum_values as string[];
    }
    if (typeof entry.default === "string") spec.default = entry.default;
    fields.set(spec.name, spec);
  }
  if (fields.size === 0) {
    throw new Error("describe --json: frontmatter array had no usable fields");
  }
  const version = typeof doc.moss_binary_version === "string" ? doc.moss_binary_version : "unknown";
  return { source: sourceLabel ?? `moss ${version}`, fields };
}

/**
 * Fields Obsidian itself owns. Not moss's — no unknown-field diagnostic, but
 * hover says moss ignores them rather than pretending they publish.
 */
export const OBSIDIAN_NATIVE_FIELDS: ReadonlySet<string> = new Set([
  "aliases",
  "alias",
  "cssclass",
  "cssclasses",
  "publish",
  "permalink",
]);
