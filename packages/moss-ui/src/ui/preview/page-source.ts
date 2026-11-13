import { engine } from '../engine';
import type { PageSource as PageSourceData } from '@symbiosis-lab/moss-ui';
import { eventBus, Events, type PreviewUrlChangedEvent } from '../context/event-bus';
import { isRootUrlPath } from '../utils/url-path';
import { pendingOf, type EditSourceResolution, type ResolvedNode } from '../editor/session/node-class';

/** Waits before each re-ask after a failed lookup; then the page stays unanswered until it changes. */
const RETRY_MS = [1000, 3000, 9000];

type Answer = PreviewUrlChangedEvent;
type Listener = (page: Answer, previous: Answer | null) => void;

/**
 * Which source backs the page the preview shows, for navigation, the bus and
 * the editor. A module singleton: one preview per window, so one target page.
 *
 * A navigation sets the target. Every ask about the target joins the lookup in
 * flight, and nothing is ever published for another URL, so a re-ask can
 * neither cancel a navigation's answer nor answer for a page that is gone.
 *
 * The bus hears each confirmed navigation's answer once, and a later answer
 * only when it changes what the page is: its listeners treat every event as
 * arriving on a page.
 */
class PageSource {
  private target: string | null = null;
  /** Numbers each lookup; a lookup settles only while it is the newest. */
  private seq = 0;
  private flight: Promise<Answer | null> | null = null;
  /** A confirmed navigation whose answer the bus has not heard yet. */
  private owed: { isReload: boolean } | null = null;
  private announced: Answer | null = null;
  /** Failed lookups in a row while the target has no answer. */
  private failures = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly listeners = new Set<Listener>();
  /** The settled answer for the target, or null while none has landed. */
  current: Answer | null = null;

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** The preview is leaving for `urlPath`, or showing nothing (null). */
  invalidate(urlPath: string | null = null): void {
    if (urlPath === this.target) return;
    this.target = urlPath;
    this.seq++;
    this.flight = null;
    this.owed = null;
    this.announced = null;
    this.current = null;
    this.failures = 0;
    this.stopRetry();
  }

  /** The preview confirmed it shows `urlPath`; the bus is owed this page's answer. */
  navigate(urlPath: string, isReload: boolean): Promise<Answer | null> {
    this.invalidate(urlPath);
    this.owed = { isReload };
    // A rebuild can change the answer, so a reload always asks afresh.
    if (!isReload && this.flight) return this.flight;
    if (!isReload && this.current && !this.failures) {
      this.publish({ ...this.current, isReload }, this.current);
      return Promise.resolve(this.current);
    }
    return this.lookup();
  }

  /** No folder to look in: settled, with no answer and nothing to retry. */
  unavailable(urlPath: string, isReload: boolean): void {
    this.invalidate(urlPath);
    this.owed = { isReload };
    this.publish(noAnswer(urlPath, isReload), this.current);
  }

  /** An answer for the page the editor opens on; adopts it when the preview has none yet. */
  ensure(urlPath: string): Promise<Answer | null> {
    if (this.target === null) this.invalidate(urlPath);
    if (urlPath !== this.target) return Promise.resolve(null);
    if (this.flight) return this.flight;
    if (this.current && !this.failures) return Promise.resolve(this.current);
    return this.lookup();
  }

  /** Ask about the target again; null for any other URL. */
  refresh(urlPath: string): Promise<Answer | null> {
    if (urlPath !== this.target) return Promise.resolve(null);
    return this.flight ?? this.lookup();
  }

  /** The source backing `urlPath`, when it is the target and one is known. */
  sourceFile(urlPath: string): string | null {
    return this.current?.urlPath === urlPath ? this.current.sourcePath : null;
  }

  node(page: Answer): ResolvedNode {
    return { sourcePath: page.sourcePath, isRoot: isRootUrlPath(page.urlPath), takeover: page.takeover, pending: page.pending };
  }

  private lookup(): Promise<Answer | null> {
    const seq = ++this.seq;
    const urlPath = this.target!;
    this.stopRetry();
    const flight = (async () => {
      let data: PageSourceData & EditSourceResolution | null = null;
      try {
        const result = await engine.resolvePageSource(urlPath);
        if (result.status === 'ok') data = result.data;
      } catch {
        // A failed lookup is no evidence about the page.
      }
      if (seq !== this.seq) return null;
      this.flight = null;
      return this.settle(urlPath, data);
    })();
    this.flight = flight;
    return flight;
  }

  private settle(urlPath: string, data: PageSourceData & EditSourceResolution | null): Answer {
    const previous = this.current;
    const isReload = this.owed?.isReload ?? true;
    let page: Answer;
    if (data) {
      this.failures = 0;
      page = {
        urlPath, isReload, pending: pendingOf(data),
        sourcePath: data.source_path, takeover: data.takeover ?? null,
        isDir: data.is_dir, isArticle: data.is_article, syndicated: data.syndicated ?? [],
      };
    } else if (previous && !this.failures) {
      // A failed re-ask keeps the answer it could not improve on.
      page = { ...previous, isReload };
    } else {
      page = noAnswer(urlPath, isReload);
      const delay = RETRY_MS[this.failures++];
      if (delay !== undefined) this.retryTimer = setTimeout(() => void this.refresh(urlPath), delay);
    }
    this.publish(page, previous);
    return page;
  }

  private publish(page: Answer, previous: Answer | null): void {
    this.current = page;
    for (const listener of this.listeners) listener(page, previous);
    // Before the navigation is confirmed the preview has not arrived on the page.
    if (this.owed || (this.announced && changed(this.announced, page))) {
      this.owed = null;
      this.announced = page;
      eventBus.emit(Events.PREVIEW_URL_CHANGED, page);
    }
  }

  private stopRetry(): void {
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }
}

function noAnswer(urlPath: string, isReload: boolean): Answer {
  return { urlPath, isReload, pending: 'lookup', sourcePath: null, takeover: null, isDir: false, isArticle: false, syndicated: [] };
}

function changed(a: Answer, b: Answer): boolean {
  return a.pending !== b.pending || a.sourcePath !== b.sourcePath || a.isDir !== b.isDir || a.isArticle !== b.isArticle
    || JSON.stringify(a.takeover) !== JSON.stringify(b.takeover) || a.syndicated.join('\n') !== b.syndicated.join('\n');
}

export const pageSource = new PageSource();
