//
// The editor (action-panel) webview's SINGLE subscription point for the
// cross-webview filesystem events. Owns ONE onMossEvent('FileChanged'), ONE
// onMossEvent('AssetReady'), ONE onMossEvent('SourceAssetChanged'), and ONE
// onMossEvent('RawFileCreated'); fans each out to handlers registered by the
// editor's subsystems, in a defined priority order (registry sync must run
// before body reload). Replaces the previous N independent self-subscriptions.
//
// NOT a home for preview-follower's transient retry — that stays self-managed
// (generation-guarded self-unsubscribe).
import { onMossEvent as realOnMossEvent } from "../utils/moss-events";
import type { FileChangeEvent } from '../preview/preview-actions';
import type { AssetReady } from '@symbiosis-lab/moss-ui';

type OnMossEvent = typeof realOnMossEvent;
type FileChangedHandler = (e: FileChangeEvent) => void | Promise<void>;
type AssetReadyHandler = (e: AssetReady) => void;
type SourceAssetChangedHandler = (e: { path: string }) => void;
type RawFileCreatedHandler = (e: { paths: string[] }) => void;

interface Reg<H> { handler: H; priority: number; }

/** The subset of EditorFileEvents that subsystems depend on to register handlers. */
export interface FileEventsCoordinator {
  onFileChanged(handler: (e: FileChangeEvent) => void | Promise<void>, priority?: number): () => void;
  onAssetReady(handler: (e: AssetReady) => void): () => void;
  onSourceAssetChanged(handler: (e: { path: string }) => void): () => void;
  onRawFileCreated(handler: (e: { paths: string[] }) => void): () => void;
}

export class EditorFileEvents implements FileEventsCoordinator {
  private fileChanged: Reg<FileChangedHandler>[] = [];
  private assetReady: Array<{ handler: AssetReadyHandler }> = [];
  private sourceAssetChanged: Array<{ handler: SourceAssetChangedHandler }> = [];
  private rawFileCreated: Array<{ handler: RawFileCreatedHandler }> = [];
  private unlisten: Array<() => void> = [];

  constructor(private readonly subscribe: OnMossEvent = realOnMossEvent) {}

  async start(): Promise<void> {
    if (this.unlisten.length > 0) return; // already started
    this.unlisten.push(await this.subscribe('FileChanged', async (msg) => {
      const e = msg.payload;
      // Snapshot the registry so a handler that unregisters mid-dispatch
      // doesn't mutate the array we're iterating.
      for (const r of [...this.fileChanged]) {
        try { await r.handler(e); }
        catch (err) { console.error('[EditorFileEvents] FileChanged handler threw:', err); }
      }
    }));
    this.unlisten.push(await this.subscribe('AssetReady', (msg) => {
      const e = msg.payload;
      for (const r of [...this.assetReady]) {
        try { r.handler(e); } catch (err) { console.error('[EditorFileEvents] AssetReady handler threw:', err); }
      }
    }));
    this.unlisten.push(await this.subscribe('SourceAssetChanged', (msg) => {
      const e = msg.payload as { path: string };
      for (const r of [...this.sourceAssetChanged]) {
        try { r.handler(e); } catch (err) { console.error('[EditorFileEvents] SourceAssetChanged handler threw:', err); }
      }
    }));
    this.unlisten.push(await this.subscribe('RawFileCreated', (msg) => {
      const e = msg.payload as { paths: string[] };
      for (const r of [...this.rawFileCreated]) {
        try { r.handler(e); } catch (err) { console.error('[EditorFileEvents] RawFileCreated handler threw:', err); }
      }
    }));
    // [diag] green-flash: the editor is now subscribed to FileChanged +
    // RawFileCreated. There is NO event replay — any create emitted before this
    // moment (e.g. during the initial import build, before the editor webview
    // attached) is dropped, so its green flash never fires.
    console.info('[diag] green-flash: EditorFileEvents subscribed (FileChanged + RawFileCreated live now)');
  }

  /**
   * Lower `priority` runs earlier. Returns an unregister fn.
   * Handlers run in ascending priority order, each awaited before the next —
   * so a lower-priority handler's async work completes before a higher-priority
   * one begins.
   */
  onFileChanged(handler: FileChangedHandler, priority = 50): () => void {
    const reg = { handler, priority };
    this.fileChanged.push(reg);
    this.fileChanged.sort((a, b) => a.priority - b.priority);
    return () => { this.fileChanged = this.fileChanged.filter((r) => r !== reg); };
  }

  onAssetReady(handler: AssetReadyHandler): () => void {
    const reg = { handler };
    this.assetReady.push(reg);
    return () => { this.assetReady = this.assetReady.filter((r) => r !== reg); };
  }

  onSourceAssetChanged(handler: SourceAssetChangedHandler): () => void {
    const reg = { handler };
    this.sourceAssetChanged.push(reg);
    return () => { this.sourceAssetChanged = this.sourceAssetChanged.filter((r) => r !== reg); };
  }

  onRawFileCreated(handler: RawFileCreatedHandler): () => void {
    const reg = { handler };
    this.rawFileCreated.push(reg);
    return () => { this.rawFileCreated = this.rawFileCreated.filter((r) => r !== reg); };
  }

  async stop(): Promise<void> {
    this.unlisten.forEach((u) => u());
    this.unlisten = [];
    this.fileChanged = [];
    this.assetReady = [];
    this.sourceAssetChanged = [];
    this.rawFileCreated = [];
  }
}
