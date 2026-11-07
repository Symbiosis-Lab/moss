/**
 * What the UI asks of the host it runs in. The UI never imports a host's own
 * code; each host fills this record before the UI first mounts. The required
 * members are what every host has. An optional member is a feature some hosts
 * lack: where it is absent the UI shows no control for it, rather than one
 * that does nothing.
 *
 * Read a member where it is used, never at module scope: a host fills the
 * record after this module has evaluated.
 */
import type { EngineOut, EngineResult, EngineTransport, ProgressUpdate, Takeover, WikilinkCompletion } from '@symbiosis-lab/moss-ui';
import type { ToastSurface } from './shared/ui/toast/app-toast-render';

/** What the empty folder's starter picker needs from the folder it opens in. */
export interface StarterPickerOpen {
  folderPath: string;
  folderName: string;
  /** The folder-mode host the picker replaces the cards in. */
  host: HTMLElement;
  /** Opens the home file the chosen starter wrote, by absolute path. */
  loadHome: (absolutePath: string) => Promise<unknown>;
  /** Puts the folder's cards back. */
  back: () => void;
}

/** The article the email composer opens on. */
export interface ComposerOpen {
  projectRoot: string;
  articleId: string;
  deployedUrl: string;
}

/** A mounted composer. */
export interface Composer {
  /** Flushes the draft and tears the composer down. */
  destroy(): void;
  /** Persists an unsaved edit; resolves once the draft write has landed. */
  flush(): Promise<void>;
}

/** Which publishing service a folder talks to, for the titlebar's environment chip. */
export interface HostingEnvironment {
  is_production: boolean;
  environment: string;
}

/** How a site import ended. */
export interface ImportOutcome {
  success: boolean;
  error?: string | null;
  total_pages: number;
}

/** What a copy into the folder landed, and what it left behind and why. */
export interface CopyInReport {
  copied: { final_name: string }[];
  skipped: { original_name: string; reason: string }[];
}

export type LogLevel = 'info' | 'warn' | 'error' | 'debug' | 'trace';

/** The one decision the nested-site guard awaits. */
export type GuardDecision =
  | { kind: 'open_here' }
  | { kind: 'open_nested'; abs_path: string }
  | { kind: 'cancel' };

export type SubmitThresholdDecision = (decision: GuardDecision, token: number) => Promise<EngineResult<null>>;
export type AcknowledgeThresholdPrompt = (token: number) => Promise<EngineResult<null>>;

/** A `{ name, extensions }` filter, as an OS file dialog takes it. */
export interface FileFilter { name: string; extensions: string[] }

/** An OS drag over the UI's view, in CSS pixels of that view. */
export type FileDragEvent =
  | { type: 'enter' }
  | { type: 'over'; x: number; y: number }
  | { type: 'drop'; paths: string[]; x: number; y: number }
  | { type: 'leave' };

/** The folder the host opened the window on. */
export interface OpenedFolder {
  folder_path: string;
  auto_serve: boolean;
  target_file: string | null;
  /** The folder's display name, for the waiting screen's title. */
  folder_name?: string;
  /** The folder's parent, home-abbreviated: the waiting screen's location line. */
  parent_display?: string;
  /** The cloud provider the folder lives in, by display name. */
  cloud_provider_label?: string | null;
}

/** An open that failed after its window was shown. */
export interface OpenFailure {
  folder_path: string;
  folder_name: string;
  detail: string;
}

/** Where a term's page is: the source that claims it, or the URL the build generates. */
export interface TermResolution {
  claimed_by: string | null;
  url: string;
}

/** Stops a subscription. */
export type Unlisten = () => void;


/** A comment on a published page, as its owner hides or shows it. */
export type CommentAction = (folder: string, source: string, id: string, pageKey: string, siteName: string) => Promise<EngineResult<null>>;

/**
 * How the UI's own parts talk to each other (the shell, the preview's
 * instruments, the editor). Engine events do not travel here; they arrive
 * through the engine's subscription.
 */
export interface Bus {
  emit(name: string, payload?: unknown): void;
  /** Returns the unsubscribe. */
  on<T>(name: string, handler: (payload: T) => void): () => void;
}

/** A bus for a host whose UI is one page, as in a browser. */
export function inPageBus(): Bus {
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  return {
    emit(name, payload) {
      for (const handler of [...(handlers.get(name) ?? [])]) handler(payload);
    },
    on<T>(name: string, handler: (payload: T) => void) {
      const set = handlers.get(name) ?? new Set();
      handlers.set(name, set);
      const h = handler as (payload: unknown) => void;
      set.add(h);
      return () => set.delete(h);
    },
  };
}

/**
 * The open documents, whoever owns their text: moss's own editing session in
 * the desktop and a browser, the host's editor where it has one. A move or a
 * rename of an open file goes through here, so the document follows its file
 * and no unsaved text is left on the old path.
 */
export interface Documents {
  /** Writes any unsaved text, so the engine reads each file as the person sees it; resolves once written. */
  flush(): Promise<unknown>;
  /** Renames one entry and rewrites the links to it. Absolute paths. */
  rename(from: string, to: string): Promise<EngineResult<EngineOut<'rename_entry_with_refs'>>>;
  /** Moves entries into a folder and rewrites the links to them. Absolute paths. */
  move(paths: string[], toDir: string): Promise<EngineResult<EngineOut<'move_entries'>>>;
  /**
   * The open document follows its file, root-relative paths: `starting`
   * before the write, so its own echo is not read as an outside change, and
   * `done` once the file is there.
   */
  retarget(from: string, to: string, phase: 'starting' | 'done'): void;
}

export interface UiHost {
  /** The engine that owns the open folder. */
  engine: {
    /** How its commands reach it. */
    call: EngineTransport;
    /** Subscribes to what it sends by name; resolves once subscribed, so nothing sent after is missed. */
    on<T>(name: string, handler: (payload: T) => void): Promise<Unlisten>;
  };
  /** One page's bus by default; a host whose UI spans windows replaces it. */
  bus: Bus;
  /**
   * Where the folder's source files are served, before a root-relative path:
   * the editor shows an image from its source, not from the build.
   */
  sourceAssetBase: string;
  /** Opens a URL where the person can see it; the UI asks only for http(s) (`utils/open-external.ts`). */
  openExternal(url: string): Promise<void>;
  /**
   * Lets the person pick files; answers their absolute paths, `[]` for a
   * cancel or where there is no dialog. A host that cannot hand back a path
   * outside the folder (a browser) copies the picks in first, each into
   * `dirFor(name)` (root-relative; the root when absent), and answers the
   * copies' paths, which are then inside the folder.
   */
  pickFiles(filters: FileFilter[], multiple: boolean, dirFor?: (fileName: string) => Promise<string>): Promise<string[]>;

  // ── What a host with files on disk and windows of its own adds (the desktop) ──

  /** The editor beside the preview. */
  editorPanel?: {
    open(sourcePath: string | null, isRoot: boolean, takeover: Takeover | null): Promise<EngineResult<null>>;
    close(): Promise<EngineResult<null>>;
  };
  /**
   * The host asks the editor to write every unsaved text, before it closes the
   * panel or quits. The handler resolves with whether all of it was written.
   */
  onSaveRequest?: (handler: () => Promise<boolean>) => Promise<Unlisten>;
  /**
   * Copies files and folders from outside the folder into `targetDir`
   * (absolute), each under a free name. A host method, not an engine command:
   * the sources are paths only the host may read.
   */
  copyIn?: (paths: string[], targetDir: string) => Promise<EngineResult<CopyInReport>>;
  /**
   * OS drags over the UI's view: files from the file manager, and moss's own
   * rows where the host intercepts every native drag (their drop carries no
   * paths). One drop per gesture.
   */
  onFileDrop?: (handler: (event: FileDragEvent) => void | Promise<void>) => Promise<Unlisten>;
  /** Asks for the keyboard for the UI's view, where the host gives each view its own. */
  focusView?: () => void;
  /** Whether the absolute path outside the folder is a directory; a drop asks before it copies in. */
  isDirectory?: (path: string) => Promise<boolean>;
  /**
   * The reference form for the file at absolute `absPath`, written from the
   * page at root-relative `fromPage`; `null` when the file is outside the
   * folder or of a kind moss cannot reference.
   */
  assetRefForPath?: (absPath: string, fromPage: string) => Promise<WikilinkCompletion | null>;
  /** Where the term `name`, in a field that feeds a kind of terms, has its page. */
  resolveTerm?: (field: string, name: string) => Promise<EngineResult<TermResolution>>;
  /** Builds the folder, serves it when `autoServe`, and keeps watching it; resolves with the build's message. */
  build?: (folder: string, autoServe: boolean, onProgress: (update: ProgressUpdate) => void) => Promise<string>;
  /** The native swipe phases for preview navigation; resolves false where there are none. */
  swipePhases?: <E>(onEvent: (event: E) => void) => Promise<boolean>;
  /** The folder the host opened this window on, if it opened one. */
  openedFolder?: () => OpenedFolder | null;
  /** Calls `handler` with an open's failure, now if it has already failed and when it does. */
  onOpenFailure?: (handler: (failure: OpenFailure) => void) => void;
  /** The window that chooses a folder, and reopening one that failed to open: the preview's waiting screen offers both. */
  launcher?: {
    show(): Promise<EngineResult<null>>;
    openRecent(folder: string): Promise<EngineResult<null>>;
  };
  /** Sends the app's logs to moss's developers, from the waiting screen of a folder that failed to open. */
  sendLogs?: () => Promise<EngineResult<string>>;
  /** Answers the nested-site question the host asked when the folder opened. */
  nestedSiteGuard?: { decide: SubmitThresholdDecision; acknowledge: AcknowledgeThresholdPrompt };

  /** Show a file or folder in the system's file manager. */
  reveal?: (path: string) => Promise<EngineResult<null>>;
  /** Lets the person pick a folder outside the page; `null` for a cancel. */
  pickFolder?: (defaultPath?: string) => Promise<string | null>;
  /** The native window the UI is drawn in; absent in a browser tab. */
  window?: {
    isFullscreen(): Promise<boolean>;
    isMaximized(): Promise<boolean>;
    minimize(): void;
    toggleMaximize(): void;
    close(): void;
  };
  /** Where the UI's log lines go; the console where absent. */
  log?: (level: LogLevel, message: string) => Promise<void>;
  /** The host's own documents, where its editor owns the text; moss's session otherwise. */
  documents?: Documents;
  /** "Try an example": choose a starter for an empty folder. */
  starterPicker?: {
    open(open: StarterPickerOpen): Promise<void>;
    /** Whether the host opened this folder to choose a starter in; asking spends the answer. */
    takeRequest(folder: string): Promise<boolean>;
  };
  /** The email composer, mounted into the editor's container on `?compose=`. */
  composer?: (container: HTMLElement, open: ComposerOpen) => Promise<Composer>;
  hostingEnvironment?: (folder: string) => Promise<HostingEnvironment>;
  /** Writes an empty folder's first page; answers its absolute path. */
  startWriting?: (folder: string) => Promise<EngineResult<string>>;
  /** Imports the site at `url` into `folder`. */
  importSite?: (url: string, folder: string) => Promise<EngineResult<ImportOutcome>>;
  /** Opens the plugins view filtered to `filter`. */
  openPluginsView?: (filter: string, folder: string) => Promise<EngineResult<null>>;
  hideComment?: CommentAction;
  unhideComment?: CommentAction;
  /** The host's queued app-level toasts, drawn by the editor while it is what the person sees. */
  toasts?: Pick<ToastSurface, 'take' | 'onAction'>;
  /** Tells the host which page the preview wants first, so its build serves that one early. */
  previewRequirement?: (folder: string, urlPath: string) => Promise<EngineResult<null>>;
  /** Hands the OS absolute paths for a drop outside moss (Finder, Explorer). */
  dragOut?: (paths: string[]) => Promise<void>;
  /** Downloads a tool the build needs (ffmpeg, git). */
  downloadTool?: (tool: string) => Promise<EngineResult<null>>;
  /**
   * Runs a plugin's process hook as the person's own act (a Sync), which may
   * write the folder. Calls `say(true, name)` while it waits for a hook a build
   * started in that plugin and `say(false, name)` once it runs, `name` being
   * what the plugin's manifest calls it.
   */
  runPlugin?: (plugin: string, say: (waiting: boolean, name: string) => void) => Promise<EngineResult<null>>;
}

/** Filled by `fillHost` before the UI mounts; read through it, never before. */
export const host = { bus: inPageBus() } as UiHost;

/** A host's whole record, checked against the seam when it is written. */
export function fillHost(impl: UiHost): void {
  Object.assign(host, impl);
}
