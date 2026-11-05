import { describe, expect, it, test, vi } from "vitest";
import { PublishPreflightStore, readResult } from "../preflight-store";

const built = { build_generation: 7 };

describe("readResult", () => {
  test("an error keeps the folder's last answer instead of clearing it", async () => {
    let answer: { status: "ok"; data: typeof built | null } | { status: "error"; error: string } = {
      status: "ok",
      data: built,
    };
    const store = new PublishPreflightStore(readResult(async () => answer));
    expect(await store.refresh("/site")).toEqual(built);

    answer = { status: "error", error: "Preview target belongs to a different folder" };
    await expect(store.refresh("/site")).rejects.toThrow("different folder");
    expect(store.snapshot("/site")).toEqual(built);
  });

  test("asks about the folder it is refreshing", async () => {
    const asked: string[] = [];
    const store = new PublishPreflightStore(
      readResult(async (folder) => {
        asked.push(folder);
        return { status: "ok" as const, data: null };
      }),
    );
    await store.refresh("/site");
    expect(asked).toEqual(["/site"]);
  });
});

const projection = (generation: number, missing = false) => ({
  build_generation: generation,
  missing_references: missing
    ? [{
      source_path: 'post.md',
      source_revision: 'a'.repeat(64),
      reference: 'lost.jpg',
      source_span: { start_byte: 8, end_byte: 16, line: 2 },
    }]
    : [],
});

describe('PublishPreflightStore', () => {
  it('replaces missed stale-clean and stale-broken event caches with the authoritative refresh', async () => {
    const read = vi.fn()
      .mockResolvedValueOnce(projection(3, false))
      .mockResolvedValueOnce(projection(4, true));
    const store = new PublishPreflightStore<ReturnType<typeof projection>>(read);

    expect((await store.refresh('/vault'))?.missing_references).toEqual([]);
    expect((await store.refresh('/vault'))?.missing_references).toHaveLength(1);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('ignores an older completion after a newer generation is cached', () => {
    const store = new PublishPreflightStore<ReturnType<typeof projection>>(async () => null);
    store.replace('/vault', projection(9, true));

    expect(store.replace('/vault', projection(8, false))).toEqual(projection(9, true));
    expect(store.snapshot('/vault')).toEqual(projection(9, true));
  });

  it('keeps folder answers separate', () => {
    const store = new PublishPreflightStore<ReturnType<typeof projection>>(async () => null);
    store.replace('/a', projection(1, true));
    store.replace('/b', projection(1, false));

    expect(store.snapshot('/a')?.missing_references).toHaveLength(1);
    expect(store.snapshot('/b')?.missing_references).toEqual([]);
  });

  it('keeps a newer null answer when an older broken read settles afterwards', async () => {
    let old!: (value: ReturnType<typeof projection>) => void;
    let newest!: (value: null) => void;
    const read = vi.fn()
      .mockImplementationOnce(() => new Promise(resolve => { old = resolve; }))
      .mockImplementationOnce(() => new Promise(resolve => { newest = resolve; }));
    const store = new PublishPreflightStore<ReturnType<typeof projection>>(read);
    const stale = store.refresh('/vault');
    const fresh = store.refresh('/vault');

    newest(null);
    await expect(fresh).resolves.toBeNull();
    old(projection(3, true));
    await expect(stale).resolves.toBeNull();
    expect(store.snapshot('/vault')).toBeNull();
  });

  it('keeps a newer clean answer when an older failed read rejects afterwards', async () => {
    let rejectOld!: (error: Error) => void;
    let newest!: (value: ReturnType<typeof projection>) => void;
    const read = vi.fn()
      .mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject; }))
      .mockImplementationOnce(() => new Promise(resolve => { newest = resolve; }));
    const store = new PublishPreflightStore<ReturnType<typeof projection>>(read);
    const stale = store.refresh('/vault');
    const fresh = store.refresh('/vault');

    newest(projection(7, false));
    await expect(fresh).resolves.toEqual(projection(7, false));
    rejectOld(new Error('old read failed'));
    await expect(stale).resolves.toEqual(projection(7, false));
  });
});
