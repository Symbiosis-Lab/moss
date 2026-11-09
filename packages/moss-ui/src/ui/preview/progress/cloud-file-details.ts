import { engine } from "../../engine";
import { cloudSync } from "../cloud/cloud-sync-store";
import type { NoticeVM } from "./progress-panel-stage";

type CloudFiles = NonNullable<NoticeVM["cloudFiles"]>;
type QueryState = CloudFiles & { folder: string; revision: number };

/** Lazy full-list query for the one cloud notice. The store still owns counts. */
export class CloudFileDetails {
  private current: QueryState | null = null;

  constructor(private render: () => void) {}

  reset(): void {
    this.current = null;
  }

  view(count: number | null): CloudFiles {
    const folder = cloudSync.getUnavailableSnapshot().folder;
    return {
      count,
      status: this.current?.folder === folder ? this.current.status : "idle",
      files: this.current?.folder === folder ? this.current.files : [],
    };
  }

  /** An open disclosure follows the next authoritative supervisor snapshot. */
  onSnapshot(open: boolean): void {
    const snapshot = cloudSync.getUnavailableSnapshot();
    if (!this.current || (snapshot.folder === this.current.folder &&
        snapshot.revision === this.current.revision && snapshot.count !== 0)) return;
    const previousFiles = snapshot.folder === this.current.folder ? this.current.files : [];
    this.current = null;
    if (open && snapshot.count !== 0) void this.request(previousFiles);
  }

  async request(previousFiles = this.current?.files ?? []): Promise<void> {
    const snapshot = cloudSync.getUnavailableSnapshot();
    if (!snapshot.folder || snapshot.count === 0) return;
    const request: QueryState = {
      folder: snapshot.folder, revision: snapshot.revision, count: snapshot.count,
      status: "loading", files: previousFiles,
    };
    this.current = request;
    this.render();
    let result: Awaited<ReturnType<typeof engine.getCloudUnavailableFiles>>;
    try {
      result = await engine.getCloudUnavailableFiles(request.folder);
    } catch {
      result = { status: "error", error: "query failed" };
    }
    const latest = cloudSync.getUnavailableSnapshot();
    if (this.current !== request || latest.folder !== request.folder ||
        latest.revision !== request.revision || latest.count === 0) return;
    this.current = result.status === "ok"
      ? { ...request, status: "loaded", files: result.data }
      : { ...request, status: "error", files: [] };
    this.render();
  }
}
