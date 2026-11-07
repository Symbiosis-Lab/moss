import { builtinSchemaJson, initializeCore } from "./core";

export interface FieldDefinition {
  type: string;
  description?: string | null;
  group?: string | null;
  enum_values?: string[] | null;
}

export interface SitePlugin {
  name: string;
  fields: Record<string, FieldDefinition>;
}

export interface FieldSpec {
  name: string;
  type: string;
  description: string;
  group: string;
  enumValues?: string[];
}

export interface MossSchema {
  fields: Map<string, FieldSpec>;
}

export function composedSchema(plugins: SitePlugin[] = [], listingStyles: string[] = []): MossSchema {
  initializeCore();
  const builtin = JSON.parse(builtinSchemaJson()) as {
    frontmatter: { fields: Record<string, FieldDefinition> };
  };
  const definitions = new Map(Object.entries(builtin.frontmatter.fields));
  // Match the core's additive composition: built-ins and earlier plugins win.
  for (const plugin of plugins) {
    for (const [name, field] of Object.entries(plugin.fields)) {
      if (!definitions.has(name)) definitions.set(name, field);
    }
  }
  const fields = new Map<string, FieldSpec>();
  for (const [name, field] of definitions) {
    fields.set(name, {
      name, type: field.type, description: field.description ?? "", group: field.group ?? "",
      enumValues: name === "children_style"
        ? [...(field.enum_values ?? []), ...listingStyles]
        : field.enum_values ?? undefined,
    });
  }
  return { fields };
}
