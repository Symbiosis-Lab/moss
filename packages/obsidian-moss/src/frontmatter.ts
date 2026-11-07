export interface FrontmatterField {
  name: string;
  /** Offset of the key's first character in the document. */
  keyFrom: number;
  keyTo: number;
  /** Offset range of the inline value ("" values: keyTo..keyTo). */
  valueFrom: number;
  valueTo: number;
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
  if (!/^---\r?\n/.test(doc)) return null;
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
      const valueTo = valueFrom + rawValue.length;
      fields.push({ name, keyFrom, keyTo, valueFrom, valueTo });
    }
    offset += line.length + 1;
  }
  return { contentFrom, contentTo: offset, fields };
}
