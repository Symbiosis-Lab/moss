// Frontmatter extraction + validation, pure functions over document text.
//
// The CM6 layer needs positions (which characters to underline), so this
// parses the frontmatter block itself — a line-based YAML subset that
// understands exactly what it needs: top-level `key: value` lines, block
// lists, and nested maps. It deliberately does NOT try to be a YAML parser;
// classification is used only to decide whether a value can possibly satisfy
// the schema type, and anything ambiguous classifies leniently so a
// diagnostic is only raised when the value is clearly wrong.

import type { FieldSpec, MossSchema } from "./schema";
import { OBSIDIAN_NATIVE_FIELDS } from "./schema";

/** What the raw YAML value looks like, syntactically. */
export type ValueKind =
  | "string"
  | "boolean"
  | "integer"
  | "number"
  | "array"
  | "object"
  | "empty";

export interface FrontmatterField {
  name: string;
  /** Offset of the key's first character in the document. */
  keyFrom: number;
  keyTo: number;
  /** Offset range of the inline value ("" values: keyTo..keyTo). */
  valueFrom: number;
  valueTo: number;
  /** Raw inline value text, trimmed ("" for block values). */
  rawValue: string;
  kind: ValueKind;
  /** True when the raw value was quoted — a quoted "true" is a string. */
  quoted: boolean;
}

export interface FrontmatterBlock {
  /** Offset just after the opening `---\n`. */
  contentFrom: number;
  /** Offset of the closing delimiter line's start. */
  contentTo: number;
  fields: FrontmatterField[];
}

const KEY_LINE_RE = /^([A-Za-z_$][\w./$-]*)(\s*):(\s|$)/;

/**
 * Locate the frontmatter block and its top-level fields.
 * Returns null when the document has no frontmatter.
 */
export function parseFrontmatterBlock(doc: string): FrontmatterBlock | null {
  if (!doc.startsWith("---\n") && doc !== "---") return null;
  const lines = doc.split("\n");
  let closeLine = -1;
  for (let i = 1; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t === "---" || t === "...") {
      closeLine = i;
      break;
    }
  }
  if (closeLine === -1) return null;

  const fields: FrontmatterField[] = [];
  let offset = lines[0].length + 1; // past "---\n"
  const contentFrom = offset;
  for (let i = 1; i < closeLine; i++) {
    const line = lines[i];
    const m = KEY_LINE_RE.exec(line);
    if (m && !line.startsWith(" ") && !line.startsWith("\t")) {
      const name = m[1];
      const keyFrom = offset;
      const keyTo = offset + name.length;
      const colonIdx = name.length + m[2].length; // position of ":"
      let rawValue = line.slice(colonIdx + 1);
      const leadingWs = rawValue.length - rawValue.trimStart().length;
      const valueFrom = offset + colonIdx + 1 + leadingWs;
      rawValue = rawValue.trim();
      // Strip a trailing comment only when clearly separated (heuristic).
      const valueTo = valueFrom + rawValue.length;
      const { kind, quoted } = classifyValue(rawValue, lines, i);
      fields.push({ name, keyFrom, keyTo, valueFrom, valueTo, rawValue, kind, quoted });
    }
    offset += line.length + 1;
  }
  return { contentFrom, contentTo: offset, fields };
}

const INT_RE = /^[+-]?\d+$/;
const FLOAT_RE = /^[+-]?(\d+\.\d*|\.\d+|\d+)([eE][+-]?\d+)?$/;

function classifyValue(
  raw: string,
  lines: string[],
  lineIdx: number,
): { kind: ValueKind; quoted: boolean } {
  if (raw === "") {
    // Block value: look at the next non-empty line inside the frontmatter.
    for (let j = lineIdx + 1; j < lines.length; j++) {
      const t = lines[j];
      if (t.trim() === "---" || t.trim() === "...") break;
      if (t.trim() === "") continue;
      if (!t.startsWith(" ") && !t.startsWith("\t") && !t.startsWith("-")) break;
      if (t.trimStart().startsWith("- ") || t.trimStart() === "-") {
        return { kind: "array", quoted: false };
      }
      if (t.startsWith(" ") || t.startsWith("\t")) {
        return { kind: "object", quoted: false };
      }
      break;
    }
    return { kind: "empty", quoted: false };
  }
  if ((raw.startsWith('"') && raw.endsWith('"') && raw.length >= 2) ||
      (raw.startsWith("'") && raw.endsWith("'") && raw.length >= 2)) {
    return { kind: "string", quoted: true };
  }
  if (raw.startsWith("[")) return { kind: "array", quoted: false };
  if (raw.startsWith("{")) return { kind: "object", quoted: false };
  if (raw === "true" || raw === "false") return { kind: "boolean", quoted: false };
  if (INT_RE.test(raw)) return { kind: "integer", quoted: false };
  if (FLOAT_RE.test(raw)) return { kind: "number", quoted: false };
  return { kind: "string", quoted: false };
}

export type DiagnosticKind = "unknown-field" | "wrong-type" | "not-in-enum";

export interface Diagnostic {
  kind: DiagnosticKind;
  field: string;
  /** Range to decorate (key for unknown-field, value otherwise). */
  from: number;
  to: number;
  message: string;
  /** Closest known field name, for unknown-field. */
  suggestion?: string;
}

/** Validate a parsed frontmatter block against the schema. */
export function validateFrontmatter(block: FrontmatterBlock, schema: MossSchema): Diagnostic[] {
  const out: Diagnostic[] = [];
  for (const field of block.fields) {
    const spec = schema.fields.get(field.name);
    if (!spec) {
      if (OBSIDIAN_NATIVE_FIELDS.has(field.name)) continue;
      const suggestion = nearestFieldName(field.name, schema);
      out.push({
        kind: "unknown-field",
        field: field.name,
        from: field.keyFrom,
        to: field.keyTo,
        message: suggestion
          ? `moss does not know the field \`${field.name}\` — did you mean \`${suggestion}\`?`
          : `moss does not know the field \`${field.name}\`. It will be ignored.`,
        ...(suggestion ? { suggestion } : {}),
      });
      continue;
    }
    const typeDiag = checkType(field, spec);
    if (typeDiag) out.push(typeDiag);
  }
  return out;
}

function checkType(field: FrontmatterField, spec: FieldSpec): Diagnostic | null {
  if (field.kind === "empty") return null;
  const range = { from: field.valueFrom, to: Math.max(field.valueTo, field.valueFrom + 1) };
  const wrong = (message: string): Diagnostic => ({
    kind: "wrong-type",
    field: field.name,
    ...range,
    message,
  });

  switch (spec.type) {
    case "string":
      // Any scalar reads as a string; only structures are clearly wrong.
      if (field.kind === "array" || field.kind === "object") {
        return wrong(`\`${field.name}\` expects text, not a ${field.kind}.`);
      }
      return null;
    case "boolean":
      if (field.kind === "boolean") return null;
      if (field.quoted || field.kind === "string") {
        return wrong(
          `\`${field.name}\` expects true or false — \`${field.rawValue}\` is text. Write \`${field.name}: true\` without quotes.`,
        );
      }
      return wrong(`\`${field.name}\` expects true or false.`);
    case "integer":
      if (field.kind === "integer") return null;
      if (field.kind === "number") {
        return wrong(`\`${field.name}\` expects a whole number.`);
      }
      return wrong(`\`${field.name}\` expects a number, got ${describeKind(field)}.`);
    case "array":
      // A bare scalar is accepted as a one-element list; structures that are
      // clearly not lists are flagged.
      if (field.kind === "object" || field.kind === "boolean") {
        return wrong(`\`${field.name}\` expects a list (use \`[a, b]\` or one \`- item\` per line).`);
      }
      return null;
    case "object":
      if (field.kind === "object") return null;
      return wrong(`\`${field.name}\` expects a nested map (indented \`key: value\` lines below it).`);
    case "one_of":
      if (spec.enumValues && field.kind === "string" && !field.quoted) {
        if (!spec.enumValues.includes(field.rawValue)) {
          return {
            kind: "not-in-enum",
            field: field.name,
            ...range,
            message: `\`${field.name}\` expects one of: ${spec.enumValues.join(", ")}.`,
          };
        }
      }
      return null;
  }
}

function describeKind(field: FrontmatterField): string {
  if (field.quoted) return "quoted text";
  return field.kind === "string" ? `text (\`${field.rawValue}\`)` : `a ${field.kind}`;
}

/** Closest schema field by edit distance (≤ 2), or undefined. */
export function nearestFieldName(name: string, schema: MossSchema): string | undefined {
  let best: string | undefined;
  let bestDist = 3;
  for (const candidate of schema.fields.keys()) {
    const d = editDistance(name.toLowerCase(), candidate.toLowerCase(), bestDist);
    if (d < bestDist) {
      bestDist = d;
      best = candidate;
    }
  }
  return best;
}

/** Bounded Levenshtein — bails at `cap` to keep the hot path cheap. */
function editDistance(a: string, b: string, cap: number): number {
  if (Math.abs(a.length - b.length) >= cap + 1) return cap + 1;
  const prev = new Array<number>(b.length + 1);
  const curr = new Array<number>(b.length + 1);
  for (let j = 0; j <= b.length; j++) prev[j] = j;
  for (let i = 1; i <= a.length; i++) {
    curr[0] = i;
    let rowMin = curr[0];
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      curr[j] = Math.min(prev[j] + 1, curr[j - 1] + 1, prev[j - 1] + cost);
      rowMin = Math.min(rowMin, curr[j]);
    }
    if (rowMin > cap) return cap + 1;
    for (let j = 0; j <= b.length; j++) prev[j] = curr[j];
  }
  return prev[b.length];
}
