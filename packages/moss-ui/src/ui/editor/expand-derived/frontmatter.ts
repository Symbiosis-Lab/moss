// The front matter facts Expand reads and writes: `title`, `description`, `date` and `inferred`.
//
// The editor's document is the body only; the front matter lives in the chip
// bar's store and is serialised on save by the backend. `expandDerived` wants a
// whole page, because the title of a stub comes from its first derived embed
// and is written into its front matter. So the click hands it a page whose head
// carries just these keys (YAML flow syntax, which JSON string literals
// already satisfy: no YAML writer is needed), and reads the same keys back.
// Every other field stays in the store untouched.

export type FrontmatterValues = Record<string, unknown>;

/** What Expand may change on the page. `undefined` for a key means "leave it"; `inferred: null` means "remove it". */
export interface FrontmatterPatch {
  title?: string;
  description?: string;
  /** ISO `YYYY-MM-DD`, as the backend writes it. */
  date?: string;
  inferred?: string[] | null;
}

/** The fields a derived embed can supply to its page. */
const FACTS = ['title', 'description', 'date'] as const;

const stringsOf = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

/** A head for `values`, or '' when the page lists nothing as inferred (the backend then has no title to write). */
export function headFor(values: FrontmatterValues): string {
  const inferred = stringsOf(values.inferred);
  if (inferred.length === 0) return '';
  const lines = ['---', `inferred: [${inferred.map((s) => JSON.stringify(s)).join(', ')}]`];
  for (const key of FACTS) {
    const own = values[key];
    if (typeof own === 'string' && own.trim()) lines.push(`${key}: ${JSON.stringify(own)}`);
  }
  lines.push('---', '');
  return lines.join('\n');
}

/** `page` split into the head `headFor` produced (possibly rewritten) and the body. */
export function splitPage(sentHead: string, page: string): { head: string; body: string } {
  if (sentHead === '') return { head: '', body: page };
  const lines = page.split('\n');
  const close = lines.indexOf('---', 1);
  if (lines[0] !== '---' || close < 0) return { head: '', body: page };
  return { head: lines.slice(0, close + 1).join('\n') + '\n', body: lines.slice(close + 1).join('\n') };
}

function scalar(raw: string): string {
  const v = raw.trim();
  if (v.startsWith('"')) { try { return JSON.parse(v) as string; } catch { return v.slice(1, -1); } }
  if (v.startsWith("'")) return v.slice(1, -1).replace(/''/g, "'");
  return v;
}

/** What changed between the head sent and the head returned, for the fact keys. */
export function patchBetween(sentHead: string, gotHead: string): FrontmatterPatch {
  if (sentHead === gotHead || sentHead === '') return {};
  const read = (head: string): { title?: string; description?: string; date?: string; inferred?: string[] } => {
    const out: { title?: string; description?: string; date?: string; inferred?: string[] } = {};
    for (const line of head.split('\n')) {
      const fact = FACTS.find((k) => line.startsWith(`${k}:`));
      if (fact) out[fact] = scalar(line.slice(fact.length + 1));
      else if (line.startsWith('inferred:')) {
        const list = line.slice('inferred:'.length).trim();
        out.inferred = list.startsWith('[') ? list.slice(1, -1).split(',').map(scalar).filter(Boolean) : [];
      }
    }
    return out;
  };
  const before = read(sentHead);
  const after = read(gotHead);
  const patch: FrontmatterPatch = {};
  for (const key of FACTS) {
    if (after[key] !== undefined && after[key] !== before[key]) patch[key] = after[key];
  }
  const was = (before.inferred ?? []).join('\u0000');
  const now = (after.inferred ?? []).join('\u0000');
  if (was !== now) patch.inferred = after.inferred && after.inferred.length > 0 ? after.inferred : null;
  return patch;
}
