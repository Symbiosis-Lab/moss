/** Completed-build publish-preflight transport cache. */
export interface GenerationProjection { build_generation: number }
export type PublishPreflightReader<T extends GenerationProjection> = (folder: string) => Promise<T | null>;
export type PublishPreflightListener<T extends GenerationProjection> = (folder: string, projection: T | null) => void;

/**
 * A cache is not a policy decision. It only keeps the last accepted BuildRecords
 * answer for each folder and makes late reads harmless. The request epoch is
 * intentionally independent of build generation: an older request must never
 * erase a newer request's clean (including null) answer merely because it
 * happened to complete last.
 */
/**
 * A reader over a command that answers a `Result`. An error throws, so
 * `refresh` keeps the folder's last answer rather than taking it as "no build".
 */
export function readResult<T extends GenerationProjection>(
  command: (folder: string) => Promise<{ status: 'ok'; data: T | null } | { status: 'error'; error: string }>,
): PublishPreflightReader<T> {
  return async (folder) => {
    const read = await command(folder);
    if (read.status === 'error') throw new Error(read.error);
    return read.data;
  };
}

export class PublishPreflightStore<T extends GenerationProjection> {
  private readonly projections = new Map<string, T>();
  private readonly listeners = new Set<PublishPreflightListener<T>>();
  private readonly requestEpoch = new Map<string, number>();

  constructor(private readonly read: PublishPreflightReader<T>) {}

  snapshot(folder: string): T | null {
    return this.projections.get(folder) ?? null;
  }

  subscribe(listener: PublishPreflightListener<T>): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  replace(folder: string, projection: T | null): T | null {
    const current = this.snapshot(folder);
    if (projection && current && current.build_generation > projection.build_generation) return current;
    if (projection) this.projections.set(folder, projection);
    else this.projections.delete(folder);
    this.emit(folder, projection);
    return projection;
  }

  /** Read atomically from BuildRecords; a superseded read returns the accepted snapshot. */
  async refresh(folder: string): Promise<T | null> {
    const epoch = (this.requestEpoch.get(folder) ?? 0) + 1;
    this.requestEpoch.set(folder, epoch);
    try {
      const projection = await this.read(folder);
      if (this.requestEpoch.get(folder) !== epoch) return this.snapshot(folder);
      return this.replace(folder, projection);
    } catch (cause) {
      if (this.requestEpoch.get(folder) !== epoch) return this.snapshot(folder);
      throw cause;
    }
  }

  private emit(folder: string, projection: T | null): void {
    for (const listener of this.listeners) listener(folder, projection);
  }
}

export function primePublishPreflight<T extends GenerationProjection>(store: PublishPreflightStore<T>, folder: string): void {
  void store.refresh(folder).catch(() => {});
}
