// The two authored shapes of a name-list field (`author`, `editor`, `jury`,
// `tags`) — one bare name, or a list of them — and the one place the editor
// reconciles them. Every surface that reads such a field (the chip's display,
// the chip-list editor, the cmd+click gesture) goes through `toNameList`, so
// none of them asks which shape it got.

/** Mirrors `moss_core::frontmatter_union::normalize_name_list`. */
export function toNameList(value: unknown): string[] {
  if (typeof value === 'string') return value.trim() === '' ? [] : [value];
  if (Array.isArray(value)) return value.map((v) => String(v));
  return [];
}

/**
 * The write half, and the reason `toNameList` cannot stand alone: a file that
 * says `author: 顧玉玲` must still say that after an edit that changed nothing
 * about it, so a one-name list is written back as a bare string. This is
 * `serialize_name_list`'s contract (frontmatter_typed.rs) on the editor's side
 * of the round trip.
 */
export function fromNameList(names: string[]): string | string[] {
  return names.length === 1 ? names[0] : [...names];
}
