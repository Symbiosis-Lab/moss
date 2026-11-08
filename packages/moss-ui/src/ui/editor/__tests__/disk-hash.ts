import { Text, type ChangeSet } from '@codemirror/state';

/**
 * The content hash a mocked backend reports. The real one is SHA-256 of the
 * file's bytes; any function of the text keeps the property the editor relies
 * on: the same text has the same hash, a different text a different one.
 */
export const textHash = (frontmatter: unknown, body: string): string => JSON.stringify([frontmatter, body]);

type Commands = { parseFrontmatter: (...args: never[]) => unknown };

/**
 * `commands` whose `parseFrontmatter` names the text it returns, as the real
 * parse does, so tests that mock a parse without a `hash` still exercise the
 * editor's hash comparison. A hash the mock does give is kept.
 */
export function withContentHash<T extends Commands>(commands: T): T {
  return new Proxy(commands, {
    get(target, key, receiver) {
      if (key !== 'parseFrontmatter') return Reflect.get(target, key, receiver);
      return async (...args: never[]) => {
        const r = await (target.parseFrontmatter as (...a: never[]) => Promise<any>)(...args);
        if (r?.status !== 'ok' || !r.data || r.data.hash !== undefined) return r;
        return { ...r, data: { ...r.data, hash: textHash(r.data.frontmatter, r.data.body) } };
      };
    },
  });
}

/** A save that succeeds and names the text it wrote. */
export const savedWithHash = async (_path: string, frontmatter: unknown, body: string) => ({
  status: 'ok' as const,
  data: { status: 'saved' as const, hash: textHash(frontmatter, body) },
});

type CmLike = { getContent?: () => string; setContent?: (text: string) => void; applyExternal?: unknown };

/**
 * A mocked `CmEditor` constructor whose instances can take an external change
 * set: it is applied to the instance's own text, and the resulting text is
 * handed to the mock's `setContent` spy. `CmEditor` has no such method; the
 * spy is these tests' record of the text the editor ended up showing.
 */
export function acceptingExternalChanges<T extends (...args: never[]) => unknown>(MockCmEditor: T): T {
  return function (this: unknown, ...args: never[]) {
    const inst = MockCmEditor(...args) as CmLike;
    if (inst && !inst.applyExternal) {
      inst.applyExternal = (changes: ChangeSet) => {
        if (changes.empty) return;
        inst.setContent?.(changes.apply(Text.of((inst.getContent?.() ?? '').split('\n'))).toString());
      };
    }
    return inst;
  } as unknown as T;
}
