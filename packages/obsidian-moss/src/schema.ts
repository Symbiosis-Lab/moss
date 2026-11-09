import type { ContentSchema, FieldDefinition } from "@symbiosis-lab/moss-ui";
import type { MossSite } from "./core";

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

export function siteSchema(site: MossSite): MossSchema {
  const schema: ContentSchema = JSON.parse(site.schemaJson());
  const fields = new Map<string, FieldSpec>();
  for (const [name, field] of Object.entries(schema.frontmatter.fields)) {
    fields.set(name, {
      name, type: field.type, description: field.description ?? "", group: field.group ?? "",
      enumValues: field.enum_values ?? undefined,
    });
  }
  return { fields };
}
