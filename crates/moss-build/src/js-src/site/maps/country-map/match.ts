/** Search matching: names, short names and cities, folded to plain lowercase words. */

/** One country of the index: slug, name, other names (short, formal, codes), cities. */
export type Row = [slug: string, name: string, names: string[], cities: string[]];
export type Hit = { slug: string; text: string };

/** The most rows a search lists. */
export const MAX_ROWS = 6;

/** Case, accents and punctuation do not matter: "Côte d'Ivoire" is "cote d ivoire". */
export const fold = (text: string): string =>
  text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).join(" ");

interface Candidate {
  /** Prefix of a name 0, of a city 1, then the same for the start of a later word (2, 3). */
  tier: number;
  exact: number;
  length: number;
  hit: Hit;
}

/** The best rows for a query: prefix matches before word-start matches, then exact, then short. */
export function rank(rows: Row[], query: string, max = MAX_ROWS): Hit[] {
  const q = fold(query);
  if (!q) return [];
  const best = new Map<string, Candidate>();
  const consider = (id: string, key: string, tier: number, hit: Hit): void => {
    const folded = fold(key);
    const prefix = folded.startsWith(q);
    if (!prefix && !` ${folded}`.includes(` ${q}`)) return;
    const candidate = { tier: tier + (prefix ? 0 : 2), exact: folded === q ? 0 : 1, length: folded.length, hit };
    const old = best.get(id);
    if (!old || compare(candidate, old) < 0) best.set(id, candidate);
  };
  for (const [slug, name, names, cities] of rows) {
    for (const key of [name, ...names]) consider(slug, key, 0, { slug, text: name });
    for (const city of cities) if (fold(city) !== fold(name)) consider(`${slug}/${city}`, city, 1, { slug, text: `${city}, ${name}` });
  }
  return [...best.values()].sort(compare).slice(0, max).map((c) => c.hit);
}

const compare = (a: Candidate, b: Candidate): number =>
  a.tier - b.tier || a.exact - b.exact || a.length - b.length || a.hit.text.localeCompare(b.hit.text);
