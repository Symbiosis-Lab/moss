// Gives moss's frontmatter fields their Obsidian property types, so the Properties view shows a
// checkbox, a date or a list where a page needs one. Obsidian has no API for the view itself;
// its per-vault type table is `.obsidian/types.json`.
import { Notice, type App } from "obsidian";
import type { FieldSpec, MossSchema } from "./schema";

export type PropertyType = "text" | "multitext" | "number" | "checkbox" | "date" | "tags";
export type TypeTable = Record<string, string>;

/** Obsidian reserves the type `tags` for the property named `tags`. */
export function propertyType(field: Pick<FieldSpec, "name" | "widget">): PropertyType {
  switch (field.widget) {
    case "checkbox": return "checkbox";
    case "date-picker": return "date";
    case "number-input": return "number";
    case "tag-input": return field.name === "tags" ? "tags" : "multitext";
    default: return "text";
  }
}

/** The non-text types moss implies; a type the person already set is kept. */
export function mergeTypes(existing: TypeTable, schema: MossSchema): { types: TypeTable; added: string[] } {
  const types = { ...existing };
  const added: string[] = [];
  for (const field of schema.fields.values()) {
    const type = propertyType(field);
    if (type === "text" || field.name in types) continue;
    types[field.name] = type;
    added.push(field.name);
  }
  return { types, added };
}

/**
 * Writes `types.json` and, when the app exposes its type manager, updates the open Properties view.
 * A `types.json` moss cannot read is left as it is: null, after a notice.
 */
export async function showMossFields(app: App, schema: MossSchema): Promise<string[] | null> {
  const path = `${app.vault.configDir}/types.json`;
  const adapter = app.vault.adapter;
  let read: unknown = {};
  if (await adapter.exists(path)) {
    try {
      read = JSON.parse(await adapter.read(path));
    } catch {
      read = null;
    }
  }
  if (typeof read !== "object" || read === null || Array.isArray(read)) {
    new Notice(`Cannot read ${path}; left it unchanged.`);
    return null;
  }
  const saved = read as { types?: TypeTable };
  const { types, added } = mergeTypes(saved.types ?? {}, schema);
  if (added.length) await adapter.write(path, JSON.stringify({ ...saved, types }, null, 2));
  const manager = (app as App & { metadataTypeManager?: { setType(name: string, type: string): unknown } }).metadataTypeManager;
  for (const name of added) manager?.setType(name, types[name]);
  return added;
}
