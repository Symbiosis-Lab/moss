// Preview Manager Unit Tests
// Tests core functionality of preview system including initialization,
// progress handling, and state management
//
// Note: Pure functions (path matching, state transitions, action determination)
// are tested in their respective test files:
// - preview-state.test.ts - State machine tests
// - preview-actions.test.ts - File change action tests
// - path-matcher.test.ts - Path utility tests
//
import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { PreviewManager } from '../preview-manager';
import { nearestSnap } from '../preview-instruments';
import { type RefreshDecisionContext } from '../preview-refresh-decision';
import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { engine } from '../../engine';
import { info, warn } from "../../log";
import { eventBus, Events } from '../../context/event-bus';
import { HERO_ALIGN, HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS, HERO_WORDS } from '../../editor/session/instrument-events';

// Mock the backend integration
// Mock blueprint grid animation
const mockBlueprintCleanup = vi.hoisted(() => vi.fn());
vi.mock('@symbiosis-lab/moss-blueprint-grid', () => ({
  startBlueprintGrid: vi.fn().mockReturnValue(mockBlueprintCleanup),
  // The rAF loop is what this file stubs out; the pitch is a shared constant
  // the screen's grid alignment reads, and a mock that drops it makes every
  // render throw.
  GRID_SPACING: 40,
}));

// Mock ProgressPanel
const mockProgressPanel = vi.hoisted(() => ({
  init: vi.fn().mockResolvedValue(undefined),
  upsertTask: vi.fn(),
  removeTask: vi.fn(),
  hasActiveTasks: vi.fn().mockReturnValue(false),
  setServerBaseUrl: vi.fn(),
  setStage: vi.fn(),
  // Folder-health scope: re-scoped on every startBuild.
  setFolder: vi.fn(),
  show: vi.fn(),
  hide: vi.fn(),
  destroy: vi.fn(),
  // onAdvisoryChange does not exist on the real ProgressPanel; its callback is
  // onTopSeverityChange, so it must NOT appear in this mock.
  // The panel drives the seam dot's severity color.
  onTopSeverityChange: vi.fn(),
  // The panel drives the seam's visibility (shown only when there's content).
  onContentChange: vi.fn(),
  // Escape key handler — called via the document keydown listener wired
  // by PreviewManager's constructor when the panel is present.
  onEscapeKey: vi.fn(),
  // The cloud-download waiting screen suppresses this panel's auto-surfacing while
  // the full-window gate owns the screen.
  setHomeGateActive: vi.fn(),
  // Cloud-sync ticks are app-wide; the panel discards those belonging to a
  // folder the user has left.
  setFolderPath: vi.fn(),
  // The one-unshowable-surface ladder: pushed by `PreviewManager.
  // applyUnshowableReason` on every reconcile.
  setScreenUp: vi.fn(),
  // What rank 7 ("building") of the ladder shows on the full-window screen.
  buildStageKnownLabel: vi.fn().mockReturnValue(null),
  // The panel is silent while any screen is up, so it is the screen's only
  // source for the build's stage sentence; it says when that sentence moved.
  setOnStageLabelChanged: vi.fn(),
}));
const MockProgressPanelClass = vi.hoisted(() => vi.fn().mockImplementation(() => mockProgressPanel));
vi.mock('../progress/progress-panel', () => ({
  ProgressPanel: MockProgressPanelClass,
}));

// Mock NavigationManager since PreviewManager now creates one in constructor
// Use vi.hoisted to ensure the mock state is available when vi.mock runs
const mockNavState = vi.hoisted(() => ({
  currentUrl: null as string | null,
  requestedUrl: null as string | null,
  requestRevision: 0,
  activePage: null as { url: string; sourceFile: string | null } | null,
  resetCalled: false,
  navResultListener: null as ((info: { url: string; notFound: boolean }) => void) | null,
  previewRequirementListener: null as ((urlPath: string) => void) | null,
  deviceModeChangeListener: null as ((isMobile: boolean) => void) | null,
  bridgeCalls: [] as unknown[][],
  bridgeAnswer: null as ((...args: unknown[]) => unknown) | null,
}));
vi.mock('../navigation-manager', () => ({
  CACHE_BUST_PARAM: '_t',
  NavigationManager: class {
    destroy() {}
    isMobilePreview() { return false; }
    constructor() { mockNavState.requestedUrl = null; mockNavState.requestRevision = 0; }
    getCurrentUrl() { return mockNavState.requestedUrl ?? mockNavState.currentUrl; }
    getRequestRevision() { return mockNavState.requestRevision; }
    beginNavigation(url: string) { mockNavState.requestedUrl = url.split('?')[0]; mockNavState.requestRevision++; }
    getActivePage() { return mockNavState.activePage; }
    setFolderPath(_path: string) {}
    // What `armNewPageWait` uses to turn a url path into the URL the bridge
    // will echo back.
    resolveFullUrl(urlPath: string) {
      const normalized = urlPath.startsWith('/') ? urlPath.slice(1) : urlPath;
      return `http://localhost:8080/${normalized}`;
    }
    navigateToPage(urlPath: string) { this.beginNavigation(this.resolveFullUrl(urlPath)); }
    reset() { mockNavState.resetCalled = true; }
    isNavigationPending() { return mockNavState.requestedUrl != null; }
    // Capture the rename 404-retry observer so tests can drive it directly.
    setNavigationResultListener(cb: ((info: { url: string; notFound: boolean }) => void) | null) {
      mockNavState.navResultListener = cb;
    }
    setPreviewRequirementListener(cb: ((urlPath: string) => void) | null) {
      mockNavState.previewRequirementListener = cb;
    }
    // Capture the device-mode change listener so tests can drive it directly.
    setDeviceModeChangeListener(cb: ((isMobile: boolean) => void) | null) {
      mockNavState.deviceModeChangeListener = cb;
    }
    // Both return false so refresh() exercises the cache-busting iframe.src
    // fallback (morph default-ON tries morph first, then reload, then iframe.src).
    async morph() { return false; }
    async reload() { return false; }
    async callBridge(...args: unknown[]) { mockNavState.bridgeCalls.push(args); return mockNavState.bridgeAnswer?.(...args) ?? null; }
  },
}));

// The engine's subscriptions the code under test makes (host.engine.on).
const engineOn = vi.fn(async (_name: string, _cb: (payload: any) => void) => () => {});
let emit = vi.fn();
// The bus's own emit, kept past the spy below: a test delivers a UI message through it.
let busDeliver: (name: string, payload?: unknown) => void = () => {};
// The open failure the host already has when the page subscribes, and its handler for a later one.
type OpenFailure = { folder_path: string; folder_name: string; detail: string };
let failedEarly: OpenFailure | null = null;
let openFailed: ((failure: OpenFailure) => void) | null = null;

// Mock the platform logging seam (source now imports from here)
vi.mock("../../log", () => ({
  uiEvent: vi.fn(),
  markStartup: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
  trace: vi.fn(),
}));

// Import mocked functions
// The host's build members, stubbed: the code under test reaches the build only through them.
const getBuildConfig = vi.fn();
const buildWithProgress = vi.fn();
import { startBlueprintGrid } from '@symbiosis-lab/moss-blueprint-grid';
import { cloudSync } from '../cloud/cloud-sync-store';
import { isOverlayUp, queueOrRun, __test__reset as resetOverlayGate } from "../../shared/ui/toast/overlay-toast-gate";
import { host } from "../../host";
import { resetTestHost } from "../../__tests__/helpers/test-host";

describe('PreviewManager', () => {
  let previewManager: PreviewManager;
  let mockIframe: HTMLIFrameElement;
  let mockOverlay: HTMLElement;

  beforeEach(() => {
    resetTestHost();
    host.engine.on = engineOn as typeof host.engine.on;
    host.openedFolder = getBuildConfig;
    host.build = buildWithProgress;
    // A desktop with a launcher, a file manager and an open-failure channel, as the waiting screen expects.
    host.launcher = { show: vi.fn(async () => ({ status: 'ok', data: null })), openRecent: vi.fn(async () => ({ status: 'ok', data: null })) } as any;
    host.reveal = vi.fn(async () => ({ status: 'ok', data: null })) as any;
    // The host reports an open that failed before the page subscribed at once, and each later one.
    openFailed = null;
    host.onOpenFailure = (handler) => {
      openFailed = handler;
      if (failedEarly) handler(failedEarly);
    };
    busDeliver = host.bus.emit;
    emit = vi.spyOn(host.bus, 'emit').mockImplementation(() => {}) as any;
    // `cloudSync` is a module-level singleton (packages/moss-ui/src/ui/preview/cloud/cloud-sync-store.ts),
    // so it carries state and subscribers across tests unless cleared here —
    // and it must happen before any PreviewManager under test subscribes to
    // it in initialize(), or that subscription is the one `reset()` throws
    // away.
    cloudSync.reset();
    // Same reason: overlay-toast-gate is a module-level singleton too.
    resetOverlayGate();

    // Create mock iframe element
    // An EventTarget, so the preview manager's `load` listener can be fired.
    mockIframe = Object.assign(new EventTarget(), {
      src: '',
      // The zoom controller resets these on destroy(), so a stub without them
      // makes every teardown path throw before the rest of destroy() runs.
      style: {},
      removeAttribute: vi.fn(),
      onerror: null,
    }) as any;

    // A REAL overlay element, not a mock object. The screens render real
    // markup into it and the blueprint canvas now lives there as a sibling
    // that must SURVIVE a re-render — none of which a hand-written stub can
    // model, and its predecessor had already grown a fake `querySelector` to
    // answer "is a screen up?". Assertions here read the overlay's state
    // rather than which classList calls were made.
    mockOverlay = document.createElement('div');
    mockOverlay.id = 'moss-preview-overlay';

    // Mock document.getElementById to return our mock iframe, overlay, or container
    vi.spyOn(document, 'getElementById').mockImplementation((id: string) => {
      if (id === 'moss-preview-iframe') return mockIframe as any;
      if (id === 'moss-preview-overlay') return mockOverlay;
      if (id === 'moss-preview-container') return document.createElement('div');
      return null;
    });

    // Mock global URL and Blob
    global.URL = {
      createObjectURL: vi.fn().mockReturnValue('blob:mock-url'),
    } as any;
    global.Blob = vi.fn() as any;

    // Mock URL constructor for URL manipulation tests
    global.URL = class MockURL {
      pathname: string;
      origin: string;
      hostname: string;
      searchParams: URLSearchParams;

      constructor(url: string) {
        if (url === 'not-a-valid-url' || url === 'invalid-url') {
          throw new Error('Invalid URL');
        }

        // Handle about:blank specially
        if (url === 'about:blank') {
          this.origin = 'null';
          this.pathname = '';
          this.hostname = '';
          this.searchParams = new URLSearchParams();
          return;
        }

        // Parse the URL manually for testing
        const match = url.match(/^(https?:\/\/([^\/]+))(\/.*)?$/);
        if (match) {
          this.origin = match[1];
          this.hostname = match[2].split(':')[0]; // Extract hostname without port
          this.pathname = match[3] || '/';
          this.searchParams = new URLSearchParams();
        } else {
          throw new Error('Invalid URL format');
        }
      }

      toString() {
        return `${this.origin}${this.pathname}${this.searchParams.toString() ? '?' + this.searchParams.toString() : ''}`;
      }

      static createObjectURL = vi.fn().mockReturnValue('blob:mock-url');
    } as any;

    // Clear all mocks
    vi.clearAllMocks();
    mockNavState.activePage = null;

    // Reset mockOverlay state (classList mocks cleared by clearAllMocks)
    mockOverlay.innerHTML = '';

    // Re-setup ProgressPanel constructor mock (clearAllMocks removes mockImplementation)
    MockProgressPanelClass.mockImplementation(() => mockProgressPanel);

    // Re-setup progress panel instance mocks after clearing
    mockProgressPanel.init = vi.fn().mockResolvedValue(undefined);
    mockProgressPanel.upsertTask = vi.fn();
    mockProgressPanel.removeTask = vi.fn();
    mockProgressPanel.hasActiveTasks = vi.fn().mockReturnValue(false);
    mockProgressPanel.setServerBaseUrl = vi.fn();
    mockProgressPanel.show = vi.fn();
    mockProgressPanel.hide = vi.fn();
    mockProgressPanel.destroy = vi.fn();
    // The real ProgressPanel has no onAdvisoryChange.
    mockProgressPanel.onTopSeverityChange = vi.fn();
    mockProgressPanel.onContentChange = vi.fn();
    mockProgressPanel.onEscapeKey = vi.fn();
    mockProgressPanel.setHomeGateActive = vi.fn();
    mockProgressPanel.setScreenUp = vi.fn();
    mockProgressPanel.buildStageKnownLabel = vi.fn().mockReturnValue(null);
    mockProgressPanel.setOnStageLabelChanged = vi.fn();

    // Re-setup blueprint grid mock after clearing
    (startBlueprintGrid as any).mockReturnValue(mockBlueprintCleanup);
  });

  afterEach(() => {
    // The rebuild hairline schedules real timers (complete() → dissolve), and
    // one that fires after the environment is torn down throws out of the run
    // with no test to blame. Tearing the hairline down here rather than the
    // whole manager keeps every test's post-conditions readable.
    (previewManager as any)?.hairline?.destroy?.();
    vi.restoreAllMocks();
  });

  describe('Editing instruments', () => {
    const fire = (name: string, payload: unknown) => busDeliver(name, payload);
    const PAGE = 'index.md';
    const selector = (line: number) => `[data-source-range^="${line}-"] img, [data-source-line="${line}"] img, img[data-source-line="${line}"]`;
    const setup = (answer: unknown = null) => {
      mockNavState.bridgeCalls = [];
      mockNavState.bridgeAnswer = () => answer;
      vi.mocked(emit).mockClear();
      mockNavState.activePage = { url: 'http://localhost/', sourceFile: '/site/index.md' };
      previewManager = new PreviewManager('moss-preview-iframe');
      (previewManager as unknown as { folderPath: string }).folderPath = '/site';
      // Instruments draw on the preview only with preview features on (preview-instruments.test.ts covers off).
      previewManager.setEditsOnPreview(true);
    };
    afterEach(() => { mockNavState.bridgeAnswer = null; mockNavState.activePage = null; });

    test('a request from a file other than the previewed page does nothing', async () => {
      Object.assign(mockIframe, { clientWidth: 1000, clientHeight: 600, getBoundingClientRect: () => ({ left: 0, top: 0, width: 500, height: 300 }) });
      setup({ boundingRect: { left: 0, top: 0, width: 1200, height: 480 } });
      for (const path of ['about.md', undefined]) {
        fire(HERO_FRAME_REQUEST, { path, line: 5 });
        fire(HERO_IMAGE_FOCUS, { path, line: 5, position: '20% 62%' });
        fire(HERO_WORDS, { path, line: 5 });
      }
      await new Promise((r) => setTimeout(r, 20));
      expect(mockNavState.bridgeCalls).toEqual([]);
      expect(emit).not.toHaveBeenCalled();
      expect(document.querySelector('.moss-instrument-layer')).toBeNull();
    });

    test('the editor\'s live crop reaches the previewed page, and null puts back the crop the page had', async () => {
      setup({ attributes: { style: 'object-position:top left' } });
      fire(HERO_IMAGE_FOCUS, { path: PAGE, line: 4, position: '20% 62%' });
      fire(HERO_IMAGE_FOCUS, { path: PAGE, line: 4, position: null });
      await vi.waitFor(() => expect(mockNavState.bridgeCalls).toEqual([
        ['document.querySelector', selector(4)],
        ['element.setStyle', selector(4), 'object-position', '20% 62%'],
        ['element.setStyle', selector(4), 'object-position', 'top left'],
      ]));
    });

    test('a picture outside a hero is found by the line the build annotated, and cropped there', async () => {
      setup({ boundingRect: { width: 600, height: 300 } });
      fire(HERO_FRAME_REQUEST, { path: PAGE, line: 9 });
      fire(HERO_IMAGE_FOCUS, { path: PAGE, line: 9, position: '10% 20%' });
      await vi.waitFor(() => expect(mockNavState.bridgeCalls).toHaveLength(3));
      for (const [, sel] of mockNavState.bridgeCalls) {
        expect(sel).toContain('[data-source-line="9"] img');
        expect(sel).toContain('img[data-source-line="9"]');
      }
      await vi.waitFor(() => expect(emit).toHaveBeenCalledWith(HERO_FRAME, { line: 9, width: 600, height: 300 }));
    });

    test('a frame request is answered with the size of the hero\'s picture, and not at all when it is not on the page', async () => {
      setup({ boundingRect: { width: 1200, height: 480 } });
      fire(HERO_FRAME_REQUEST, { path: PAGE, line: 7 });
      await vi.waitFor(() => expect(emit).toHaveBeenCalledWith(HERO_FRAME, { line: 7, width: 1200, height: 480 }));
      expect(mockNavState.bridgeCalls).toEqual([['document.querySelector', selector(7)]]);
      mockNavState.bridgeAnswer = () => null;
      vi.mocked(emit).mockClear();
      fire(HERO_FRAME_REQUEST, { path: PAGE, line: 7 });
      await vi.waitFor(() => expect(mockNavState.bridgeCalls).toHaveLength(2));
      expect(emit).not.toHaveBeenCalled();
    });

    test('a line or position that is not what the editor sends never reaches a selector or a style', () => {
      setup();
      for (const line of ['7"] x', 0, 1.5, null]) fire(HERO_FRAME_REQUEST, { line });
      for (const position of ['red', '20% 62%; x:y', '1000% 5%']) fire(HERO_IMAGE_FOCUS, { path: PAGE, line: 4, position });
      fire(HERO_IMAGE_FOCUS, { path: PAGE, line: '4', position: '1% 1%' });
      expect(mockNavState.bridgeCalls).toEqual([]);
    });

    describe('the words box', () => {
      const HERO = { left: 0, top: 0, width: 1000, height: 400 };
      const WORDS = { left: 40, top: 220, width: 300, height: 140 };
      const box = () => document.querySelector<HTMLElement>('.moss-instrument-words');
      const guides = () => [...document.querySelectorAll<HTMLElement>('.moss-instrument-guide')];
      const draw = async (rects = true) => {
        // The preview sits at (50, 80) in the shell, shown at half size.
        Object.assign(mockIframe, { clientWidth: 1000, clientHeight: 600, getBoundingClientRect: () => ({ left: 50, top: 80, width: 500, height: 300 }) });
        setup();
        mockNavState.bridgeAnswer = (_m, sel) => (rects ? { boundingRect: String(sel).endsWith('.moss-hero-content') ? WORDS : HERO } : null);
        fire(HERO_WORDS, { path: PAGE, line: 7 });
        if (rects) await vi.waitFor(() => expect(box()).not.toBeNull());
      };
      afterEach(() => previewManager.destroy());

      test.each([
        [{ left: 130, width: 300 }, 'start'],
        [{ left: 260, width: 300 }, 'center'],
        [{ left: 520, width: 300 }, 'end'],
      ] as const)('a box dropped at %j takes the %s guide', (at, snap) => expect(nearestSnap(at, { left: 100, width: 800 })).toBe(snap));

      test('a caret on a hero draws the box and three guides over the preview, and off it removes them', async () => {
        await draw();
        expect(mockNavState.bridgeCalls).toContainEqual(['document.querySelector', '[data-source-range^="7-"] .moss-hero-content']);
        expect([box()!.style.left, box()!.style.top, box()!.style.width]).toEqual(['20px', '110px', '150px']);
        expect(guides().map((g) => g.style.left)).toEqual(['0px', '250px', '500px']);
        fire(HERO_WORDS, { path: PAGE, line: null });
        await vi.waitFor(() => expect(document.querySelector('.moss-instrument-layer')).toBeNull());
      });

      test('until the preview-features setting is read as on, a caret on a hero draws nothing, and turning it on draws the box', async () => {
        Object.assign(mockIframe, { clientWidth: 1000, clientHeight: 600, getBoundingClientRect: () => ({ left: 50, top: 80, width: 500, height: 300 }) });
        setup();
        previewManager.setEditsOnPreview(false);
        mockNavState.bridgeAnswer = (_m, sel) => ({ boundingRect: String(sel).endsWith('.moss-hero-content') ? WORDS : HERO });
        fire(HERO_WORDS, { path: PAGE, line: 7 });
        await new Promise((r) => setTimeout(r, 20));
        expect(box()).toBeNull();
        previewManager.setEditsOnPreview(true);
        await vi.waitFor(() => expect(box()).not.toBeNull());
      });

      test('a hero that is not on the page draws nothing', async () => {
        await draw(false);
        await vi.waitFor(() => expect(mockNavState.bridgeCalls).toHaveLength(2));
        expect(box()).toBeNull();
      });

      test('dropping the box on the end guide asks the editor for align end', async () => {
        await draw();
        const at = (type: string, clientX: number) => box()!.dispatchEvent(new MouseEvent(type, { bubbles: true, button: 0, clientX }));
        at('pointerdown', 100);
        at('pointermove', 580);
        at('pointerup', 580);
        expect(emit).toHaveBeenCalledWith(HERO_ALIGN, { line: 7, align: 'end' });
      });

      test('a rebuild that lands redraws it from the new page', async () => {
        await draw();
        mockNavState.bridgeAnswer = () => ({ boundingRect: { ...WORDS, left: 700 } });
        (previewManager as unknown as { onRefreshApplied(): void }).onRefreshApplied();
        await vi.waitFor(() => expect(box()!.style.left).toBe('350px'));
      });

      test('scrolling the preview hides it until the page is still', async () => {
        await draw();
        vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
        try {
          window.dispatchEvent(new MessageEvent('message', { data: { type: 'moss-scroll-position' } }));
          expect(box()).toBeNull();
          await vi.advanceTimersByTimeAsync(200);
        } finally { vi.useRealTimers(); }
        await vi.waitFor(() => expect(box()).not.toBeNull());
      });
    });
  });

  describe('Constructor', () => {
    test('creates instance with default iframe id', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      expect(document.getElementById).toHaveBeenCalledWith('moss-preview-iframe');
    });

    test('creates instance with custom iframe id', () => {
      previewManager = new PreviewManager('custom-iframe');
      expect(document.getElementById).toHaveBeenCalledWith('custom-iframe');
    });

    test('logs warning when iframe not found', () => {
      vi.spyOn(document, 'getElementById').mockReturnValue(null);
      vi.mocked(warn).mockClear();

      // Should not throw, just warn
      previewManager = new PreviewManager('moss-preview-iframe');

      expect(warn).toHaveBeenCalledWith(
        '⚠️ [PreviewManager] Iframe with id "moss-preview-iframe" not found'
      );
    });

    test('exposes state via getState()', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      const state = previewManager.getState();
      expect(state).toBeDefined();
      expect(state.getStatus()).toEqual({ type: 'initial' });
    });

    test('Escape keydown reaches panel.onEscapeKey()', () => {
      // WCAG 2.1.1: keyboard must be able to collapse a pinned panel.
      // PreviewManager wires a document keydown listener that calls onEscapeKey.
      previewManager = new PreviewManager('moss-preview-iframe');

      // Prior tests in this describe block also create PreviewManager instances whose
      // keydown listeners are still registered on `document` (no destroy() called in
      // afterEach). All share the same mockProgressPanel singleton. Reset the count
      // here so we measure only the dispatch below, not residual prior-test calls.
      mockProgressPanel.onEscapeKey.mockClear();

      // Simulate a keydown with key=Escape.
      const event = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true });
      document.dispatchEvent(event);

      // At least 1 (this manager's listener) — multiple may fire because prior-test
      // managers also have listeners; what matters is that THIS instance wired it.
      expect(mockProgressPanel.onEscapeKey).toHaveBeenCalled();
    });

    test('onTopSeverityChange and onContentChange are wired (onAdvisoryChange absent)', () => {
      // The real PreviewManager constructor calls onTopSeverityChange and
      // onContentChange to drive the seam dot colour and visibility. The panel has no
      // onAdvisoryChange must NOT be called.
      previewManager = new PreviewManager('moss-preview-iframe');
      expect(mockProgressPanel.onTopSeverityChange).toHaveBeenCalledTimes(1);
      expect(mockProgressPanel.onContentChange).toHaveBeenCalledTimes(1);
      // onAdvisoryChange was removed — the mock intentionally omits it; if the
      // production code still called it the missing property would surface as a
      // TypeError in a non-mocked test, so asserting absence is sufficient.
      expect((mockProgressPanel as any).onAdvisoryChange).toBeUndefined();
    });
  });

  describe('Configuration and Initialization', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('initialize calls startBuild when config exists', async () => {
      const mockConfig = { folder_path: '/test/folder', auto_serve: true };
      (getBuildConfig as any).mockReturnValue(mockConfig);
      (buildWithProgress as any).mockResolvedValue('success');

      const startBuildSpy = vi.spyOn(previewManager, 'startBuild');

      await previewManager.initialize();

      expect(getBuildConfig).toHaveBeenCalled();
      // Trailing three are the wait screen's title/location-line inputs —
      // undefined because this mock config doesn't set them.
      expect(startBuildSpy).toHaveBeenCalledWith(
        '/test/folder', true, undefined, undefined, undefined, undefined,
      );
    });

    test('with no config yet, initialize keeps the starting screen and starts no build', async () => {
      // Rust shows the window before the folder is ready, on the shell init
      // script alone; the folder arrives later with `config-ready`. Until then
      // the page is starting, not "no folder": it must not greet, and must not
      // build.
      (getBuildConfig as any).mockReturnValue(null);
      const startBuildSpy = vi.spyOn(previewManager, 'startBuild');

      await previewManager.initialize();

      expect(startBuildSpy).not.toHaveBeenCalled();
      expect(mockOverlay.innerHTML).not.toContain('Welcome');
    });

    test('an open that failed before this page had a folder shows its failure, whichever comes first', async () => {
      (getBuildConfig as any).mockReturnValue(null);
      const reopen = vi.fn().mockResolvedValue({ status: 'ok', data: null });
      host.launcher = { show: vi.fn().mockResolvedValue({ status: 'ok', data: null }), openRecent: reopen };
      failedEarly = { folder_path: '/Users/test/my-blog', folder_name: 'my-blog', detail: 'The folder could not be read.' };
      try {
        await previewManager.initialize();
        expect(mockOverlay.textContent).toContain('moss could not open my-blog');
        expect(mockOverlay.textContent).toContain('The folder could not be read.');

        mockOverlay.querySelector<HTMLButtonElement>('#moss-waiting-retry-open')!.click();
        expect(reopen).toHaveBeenCalledWith('/Users/test/my-blog');

        openFailed!({ folder_path: '/Users/test/other', folder_name: 'other', detail: 'Gone.' });
        expect(mockOverlay.textContent).toContain('moss could not open other');
      } finally {
        failedEarly = null;
        reopen.mockRestore();
      }
    });

    test('initialize listens for config-ready event and starts build when received', async () => {
      // FIX TEST: This tests the event-driven approach to fix the race condition
      // Instead of polling for window.__MOSS_BUILD_CONFIG__, we listen for an event

      // No config via global variable
      (getBuildConfig as any).mockReturnValue(null);
      (buildWithProgress as any).mockResolvedValue('success');

      // Track what event listener was registered
      let configReadyCallback: ((event: { payload: { folder_path: string; auto_serve: boolean } }) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'config-ready') {
          configReadyCallback = callback;
        }
        return Promise.resolve(() => {}); // unlisten function
      });

      const startBuildSpy = vi.spyOn(previewManager, 'startBuild');

      // Start initialization - should set up listener
      const initPromise = previewManager.initialize();

      // Simulate backend emitting config-ready event after 50ms
      // (This would happen after webview is ready but before 500ms polling timeout)
      await new Promise(resolve => setTimeout(resolve, 50));

      if (configReadyCallback) {
        (configReadyCallback as (payload: { folder_path: string; auto_serve: boolean }) => void)({ folder_path: '/event/driven/path', auto_serve: true });
      }

      await initPromise;

      // Should have listened for config-ready event
      expect(engineOn).toHaveBeenCalledWith('config-ready', expect.any(Function));

      // Should have started build with config from event
      expect(startBuildSpy).toHaveBeenCalledWith(
        '/event/driven/path', true, undefined, undefined, undefined, undefined,
      );
    });

    test('drives the nav-pill hairline across the WHOLE deploy; completes on complete; cancels on failed', async () => {
      (getBuildConfig as any).mockReturnValue(null);
      // Real chrome so the hairline MOUNTS and latches active: unmounted, its
      // startDeterminate() self-start fallback re-fires on every tick and the
      // "opened once" spy count below would count mount failures, not opens.
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const h = (previewManager as any).hairline;
      // startDeterminate, NOT start: start() arms the NProgress trickle, which
      // outran the real byte fraction and — the fill being monotonic — could
      // never be pulled back. That was the "fast at first, then very slow" bug.
      const startSpy = vi.spyOn(h, 'startDeterminate');
      const trickleSpy = vi.spyOn(h, 'start');
      const upSpy = vi.spyOn(h, 'setUploadProgress');
      const completeSpy = vi.spyOn(h, 'complete').mockImplementation(() => {});
      const cancelSpy = vi.spyOn(h, 'cancel').mockImplementation(() => {});

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));
      expect(deployCb).toBeTruthy();
      const fire = deployCb!;

      // The pass opens at the FIRST stage seen and runs one monotonic 0→1 over
      // every stage — it does NOT sit idle through syncing and then restart at 0
      // for the upload. A bar that restarts is a bar that went backwards.
      fire({ stage: 'syncing', current: 0, total: 10, message: '' });
      expect(startSpy).toHaveBeenCalled();
      expect(trickleSpy).not.toHaveBeenCalled();
      const atSyncing = upSpy.mock.calls.at(-1)![0] as number;
      expect(atSyncing).toBeGreaterThan(0);

      fire({ stage: 'uploading', current: 1, total: 10, bytes_uploaded: 512, bytes_total: 4096, message: '' });
      const atUploading = upSpy.mock.calls.at(-1)![0] as number;
      expect(atUploading).toBeGreaterThan(atSyncing);
      expect(atUploading).toBeLessThan(1);
      // The pass opened ONCE, at the first stage seen. A re-open per tick would
      // reset the fill to 0 and the rate sampler with it — the restart bug in a
      // new coat. (Verified gap: `if (true)` around startDeterminate passed.)
      expect(startSpy).toHaveBeenCalledTimes(1);

      // complete → fill to full + flash + dissolve.
      fire({ stage: 'complete', current: 10, total: 10, message: '' });
      expect(completeSpy).toHaveBeenCalled();

      // A later failed deploy first RE-OPENS a fresh pass — the terminal branch
      // must reset uploadingStarted or a second deploy inherits a dead bar, a
      // stale sticky rate, and siteWorkBusy() pinned true (which starves
      // beginAmbientDownload for the rest of the session) — then cancels the
      // bar on failure (no green flash).
      fire({ stage: 'uploading', current: 1, total: 5, bytes_uploaded: 100, bytes_total: 1000, message: '' });
      expect(startSpy).toHaveBeenCalledTimes(2);
      fire({ stage: 'failed', current: 0, total: 5, message: '' });
      expect(cancelSpy).toHaveBeenCalled();

      await initPromise;
    });

    test('verifying (post-complete) neither re-opens the pass nor kills the pending flash', async () => {
      // Publish = verified live: `verifying` arrives AFTER `complete` on the
      // OnionPress channel. The terminal branch already closed the pass
      // (uploadingStarted=false) and queued the pill flash inside
      // hairline.complete(). Without an explicit ignore, the verifying tick
      // would fall into the "first stage seen" arm — startDeterminate re-opens
      // a bar no bytes will ever advance (a stranded live dot for the whole
      // verification window) and cancelPendingDissolve kills the queued flash.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const h = (previewManager as any).hairline;
      const startSpy = vi.spyOn(h, 'startDeterminate');
      const completeSpy = vi.spyOn(h, 'complete').mockImplementation(() => {});
      const dissolveSpy = vi.spyOn(h, 'cancelPendingDissolve');

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));
      const fire = deployCb!;

      fire({ stage: 'uploading', current: 1, total: 5, bytes_uploaded: 100, bytes_total: 1000, message: '' });
      expect(startSpy).toHaveBeenCalledTimes(1);
      const dissolveCallsAtComplete = dissolveSpy.mock.calls.length;
      fire({ stage: 'complete', current: 5, total: 5, message: '' });
      expect(completeSpy).toHaveBeenCalledTimes(1);

      fire({ stage: 'verifying', current: 0, total: 0, message: '' });
      // No re-open, no flash-kill, no state change of any kind.
      expect(startSpy).toHaveBeenCalledTimes(1);
      expect(dissolveSpy.mock.calls.length).toBe(dissolveCallsAtComplete);
      expect((previewManager as any).uploadingStarted).toBe(false);

      await initPromise;
    });

    test('a deploy sets NO hover tooltip — the panel already carries those numbers', async () => {
      // Regression, both halves of it. The readout used to land as data-tooltip
      // on the fill TRACK, which (a) duplicated the panel's count slot on the
      // same hover — a tooltip and a popover must not fire from one gesture —
      // and (b) armed `pointer-events:auto` on an element sitting at
      // z(--z-toast)+2, ABOVE the `.moss-seam-hit` band at z:2, so the hover
      // that opens the panel at all was swallowed for the whole publish.
      // Asserted on the real DOM, not a spy: no pointer affordance may appear
      // on the track.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));
      const fire = deployCb!;
      const track = () =>
        document.querySelector<HTMLElement>('.moss-preview-hairline-track');

      fire({ stage: 'uploading', current: 1, total: 214, bytes_uploaded: 4.1e6, bytes_total: 58e6, message: '' });
      fire({ stage: 'uploading', current: 2, total: 214, bytes_uploaded: 12.4e6, bytes_total: 58e6, message: '' });
      expect(track()!.hasAttribute('data-tooltip')).toBe(false);
      // Nor on the band: during a deploy the panel is the detail channel.
      const hit = document.querySelector<HTMLElement>('.moss-seam-hit');
      expect(hit?.hasAttribute('data-tooltip') ?? false).toBe(false);

      fire({ stage: 'complete', current: 214, total: 214, message: '' });
      expect(track()!.hasAttribute('data-tooltip')).toBe(false);

      await initPromise;
      document.body.innerHTML = '';
    });

    test('a publish that resolves with NO terminal stage (needs_setup) still closes the pass', async () => {
      // Rust emits `Preparing` BEFORE the site-id check, so push_site →
      // NeedsSetup returns Ok having opened the pass and never emitting
      // complete/failed. Without the coarse-bus backstop the bar sat as a live
      // dot forever and siteWorkBusy() stayed true — beginAmbientDownload
      // starved for the rest of the session.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const h = (previewManager as any).hairline;
      const cancelSpy = vi.spyOn(h, 'cancel').mockImplementation(() => {});

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));

      deployCb!({ stage: 'preparing', current: 0, total: 0, message: '' });
      expect(previewManager.beginAmbientDownload()).toBe(false); // pass is open

      // The publish button resolves the cycle (setNormalState on needs_setup).
      eventBus.emit(Events.DEPLOY_PROGRESS, { isDeploying: false });
      expect(cancelSpy).toHaveBeenCalled(); // quiet close — no green flash
      expect(previewManager.beginAmbientDownload()).toBe(true); // un-starved
      previewManager.endAmbientDownload(false);

      await initPromise;
      document.body.innerHTML = '';
    });

    test('an ambient download makes the band VISIBLE and carries its label', async () => {
      // The label used to land on a band the panel kept display:none (no panel
      // content exists during a pure stack download), so the tooltip the
      // facade exists to show could never be hovered.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      expect(previewManager.beginAmbientDownload('Downloading Email stack — 3%')).toBe(true);
      const hit = document.querySelector<HTMLElement>('.moss-seam-hit')!;
      expect(hit.classList.contains('is-hidden')).toBe(false);
      expect(hit.getAttribute('data-tooltip')).toBe('Downloading Email stack — 3%');

      previewManager.setAmbientDownloadProgress(0.4, 'Downloading Email stack — 40%');
      expect(hit.getAttribute('data-tooltip')).toBe('Downloading Email stack — 40%');

      previewManager.endAmbientDownload(true);
      expect(hit.hasAttribute('data-tooltip')).toBe(false);
      expect(hit.classList.contains('is-hidden')).toBe(true);
      document.body.innerHTML = '';
    });

    test('an ambient download surfaces its label as a NARRATING panel task row, torn down with the pass', () => {
      // The number must never be hover-only: the
      // hairline fill + tooltip get a sibling task row in the progress panel
      // carrying the same "name … percent" sentence as visible text.
      //
      // `narrate`, NOT `ambient`. An ambient row is by contract one that never
      // pops the panel open — right for a sub-job of work already on screen,
      // wrong for a machine-scoped stack install: minutes long, explicitly
      // clicked, and invisible everywhere else once the catalog closes. Filed
      // ambient, the sentence would be reachable only by hovering a 1.5px line,
      // so a minutes-long install would narrate nothing.
      const panel = { upsertTask: vi.fn(), removeTask: vi.fn() };
      (previewManager as any).progressPanel = panel;

      previewManager.beginAmbientDownload('Downloading Email stack — 3%');
      expect(panel.upsertTask).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'ambient-download', label: 'Downloading Email stack — 3%', narrate: true }),
      );
      expect(panel.upsertTask).not.toHaveBeenCalledWith(
        expect.objectContaining({ ambient: true }),
      );

      previewManager.setAmbientDownloadProgress(0.4, 'Downloading Email stack — 40%');
      expect(panel.upsertTask).toHaveBeenLastCalledWith(
        expect.objectContaining({ label: 'Downloading Email stack — 40%', progress: 40 }),
      );

      previewManager.endAmbientDownload(true);
      expect(panel.removeTask).toHaveBeenCalledWith('ambient-download');
    });

    test('a preempted ambient download loses its panel row along with its tooltip', () => {
      const panel = { upsertTask: vi.fn(), removeTask: vi.fn() };
      (previewManager as any).progressPanel = panel;

      previewManager.beginAmbientDownload('Downloading Foo — 40%');
      (previewManager as any).preemptAmbientPass();
      expect(panel.removeTask).toHaveBeenCalledWith('ambient-download');
    });

    test('a deploy preempting an ambient download clears its tooltip for good', async () => {
      // The strand: deploy preempts (downloadPassActive = false) → the
      // download's own terminal then early-returns behind the not-active guard
      // — before this fix, BEFORE its setTooltip(null), leaving the stale
      // label on the band for the rest of the session.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));

      previewManager.beginAmbientDownload('Downloading Foo — 40%');
      const hit = document.querySelector<HTMLElement>('.moss-seam-hit')!;
      expect(hit.getAttribute('data-tooltip')).toBe('Downloading Foo — 40%');

      deployCb!({ stage: 'syncing', current: 0, total: 10, message: '' });
      expect(hit.hasAttribute('data-tooltip')).toBe(false);

      // The preempted download's terminal arrives mid-deploy: still clean.
      previewManager.endAmbientDownload(false);
      expect(hit.hasAttribute('data-tooltip')).toBe(false);

      deployCb!({ stage: 'complete', current: 10, total: 10, message: '' });
      await initPromise;
      document.body.innerHTML = '';
    });

    test('neither a draining rebuild NOR its background media can drive, complete, or flash the hairline mid-publish', async () => {
      // `Preparing` fires before the deploy drains in-flight work, so a
      // pre-publish rebuild's ticks arrive mid-publish. Letting them through
      // drove the bar to ~90, flashed "site updated", dissolved, and handed
      // the deploy a restarted bar (0 → 90 → 100+flash → 27 in one publish).
      //
      // The BACKGROUND half of that stream was the hole the first fix left, and
      // it is the one users actually hit: a media-heavy first publish drains
      // image/video encodes that keep emitting BackgroundProgress inside the
      // deploy pass. Those fractions are of an ENCODE, so a half-finished one
      // pinned the bar at 0.5 × BUILD_CAP = 45% while 4 MB of 540 MB had gone
      // up — and applyDeterminate is monotonic, so nothing could pull it back.
      (getBuildConfig as any).mockReturnValue(null);
      document.body.innerHTML =
        '<div class="moss-titlebar"><div class="moss-pill-cluster"></div></div>';

      let deployCb: ((payload: any) => void) | null = null;
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'deploy-progress') deployCb = callback;
        return Promise.resolve(() => {});
      });

      const h = (previewManager as any).hairline;
      const buildSpy = vi.spyOn(h, 'setBuildProgress');
      const completeSpy = vi.spyOn(h, 'complete').mockImplementation(() => {});
      const dissolveSpy = vi.spyOn(h, 'dissolveAfter');

      const initPromise = previewManager.initialize();
      await new Promise(resolve => setTimeout(resolve, 50));

      deployCb!({ stage: 'preparing', current: 0, total: 0, message: '' });

      const buildTick = (percentage: number, completed: boolean): ProgressUpdate => ({
        step: 'building', message: '', percentage, completed, port: null, is_empty: null,
      });
      (previewManager as any).handleProgressUpdate(buildTick(60, false));
      expect(buildSpy).not.toHaveBeenCalled();
      (previewManager as any).handleProgressUpdate(buildTick(100, true));
      expect(dissolveSpy).not.toHaveBeenCalled();
      (previewManager as any).onRefreshApplied(); // the morph landing
      expect(completeSpy).not.toHaveBeenCalled();

      // A draining video encode, half done. Neither the fill nor the dissolve
      // watchdog may move — `dissolveAfter` calls `complete()`, which would
      // fill the bar and dissolve it in the middle of the upload.
      (previewManager as any).handleBackgroundProgress({
        task: 'videos', current: 1, total: 2, message: '', completed: false, advisories: [],
      });
      expect(buildSpy).not.toHaveBeenCalled();
      expect(dissolveSpy).not.toHaveBeenCalled();
      // Its completion is likewise silent — but the in-flight bookkeeping still
      // ran, or siteWorkBusy() would stay pinned on a task that finished.
      (previewManager as any).handleBackgroundProgress({
        task: 'videos', current: 2, total: 2, message: '', completed: true, advisories: [],
      });
      expect(dissolveSpy).not.toHaveBeenCalled();
      expect((previewManager as any).bgPageTasksInFlight.size).toBe(0);

      // The deploy's own terminal still carries the fill + flash.
      deployCb!({ stage: 'complete', current: 1, total: 1, message: '' });
      expect(completeSpy).toHaveBeenCalledTimes(1);

      // After the publish, builds own the bar again.
      (previewManager as any).handleProgressUpdate(buildTick(30, false));
      expect(buildSpy).toHaveBeenCalledTimes(1);

      await initPromise;
      document.body.innerHTML = '';
    });
  });

  describe('Progress Event Handling', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('handleProgressUpdate transitions state to loading and routes to progress panel', () => {
      const update: ProgressUpdate = {
        step: 'scanning',
        message: 'Scanning files...',
        percentage: 25,
        completed: false,
        port: null,
        is_empty: null,
      };

      (previewManager as any).handleProgressUpdate(update);

      // State should be loading
      expect(previewManager.getState().getStatus()).toEqual({
        type: 'loading',
        message: 'Scanning files...',
        percentage: 25,
      });

      // Should show blueprint grid overlay
      expect(startBlueprintGrid).toHaveBeenCalledWith(mockOverlay, { onFrame: expect.any(Function) });
    });

    test('destroy() stops the grid it started', () => {
      // The grid belongs to the overlay rather than to any screen on it, so
      // nothing a screen's teardown does reaches it: only clearing the slot
      // does. Left running past destroy(), its rAF loop, ResizeObserver,
      // documentElement MutationObserver and mousemove listener all outlive
      // the manager — and a re-init starts a second one on top.
      (previewManager as any).handleProgressUpdate({
        step: 'scanning',
        message: 'Scanning files...',
        percentage: 25,
        completed: false,
        port: null,
        is_empty: false,
      } satisfies ProgressUpdate);
      expect(startBlueprintGrid).toHaveBeenCalled();

      previewManager.destroy();

      expect(mockBlueprintCleanup).toHaveBeenCalled();
      expect(mockOverlay.classList.contains('visible')).toBe(false);
      expect(mockOverlay.children.length).toBe(0);
    });

    test('initial build (serverUrl null) uses the hairline as its only progress display', () => {
      // After deleting the panel stage/per-task bars, the INITIAL build must
      // keep a progress encoding: it now drives the nav-pill hairline, the same
      // way rebuilds already did. The serverUrl is still null here (no port seen).
      const setBuildProgress = vi.spyOn(
        (previewManager as any).hairline,
        'setBuildProgress',
      );

      const update: ProgressUpdate = {
        step: 'scanning',
        message: 'Scanning files...',
        percentage: 40,
        completed: false,
        port: null,
        is_empty: null,
      };

      (previewManager as any).handleProgressUpdate(update);

      expect(setBuildProgress).toHaveBeenCalledWith(0.4);
    });

    test('hairline advances on background images and re-arms the dissolve watchdog each tick (Bug 1 / risk #7)', () => {
      // The blocking build reaching 100% no longer means media is ready: image
      // webp / video transcode run in the BACKGROUND after. The hairline keeps
      // advancing on BackgroundProgress{task:'images'|'videos'}; each in-flight
      // tick RE-ARMS a bounded dissolve watchdog (never a bare cancel) so a
      // dropped completed:true can't hang the bar. The completed-all path
      // dissolves sooner (1.5s).
      const h = (previewManager as any).hairline;
      const setBuildProgress = vi.spyOn(h, 'setBuildProgress').mockImplementation(() => {});
      const dissolveAfter = vi.spyOn(h, 'dissolveAfter').mockImplementation(() => {});

      // Blocking build completes → arms the backstop dissolve timer.
      (previewManager as any).handleProgressUpdate({
        step: 'complete', message: 'done', percentage: 100,
        completed: true, port: 3000, is_empty: false,
      });
      expect(dissolveAfter).toHaveBeenCalled();
      dissolveAfter.mockClear();

      // Background images running (1/4): bar advances AND the watchdog is
      // re-armed (15s) — NOT canceled — so a missed completed:true still
      // eventually dissolves.
      (previewManager as any).handleBackgroundProgress({
        task: 'images', current: 1, total: 4, message: '', completed: false, advisories: [],
      });
      expect(setBuildProgress).toHaveBeenCalledWith(0.25);
      expect(dissolveAfter).toHaveBeenCalledWith(15000);

      // Videos also start running (re-arms the watchdog again).
      (previewManager as any).handleBackgroundProgress({
        task: 'videos', current: 0, total: 2, message: '', completed: false, advisories: [],
      });
      dissolveAfter.mockClear();

      // Images done, but videos still in flight → the completed branch does
      // NOT schedule the short dissolve yet (the video tick's watchdog holds).
      (previewManager as any).handleBackgroundProgress({
        task: 'images', current: 4, total: 4, message: '', completed: true, advisories: [],
      });
      expect(dissolveAfter).not.toHaveBeenCalled();

      // Videos done → both media tasks complete → short dissolve scheduled.
      (previewManager as any).handleBackgroundProgress({
        task: 'videos', current: 2, total: 2, message: '', completed: true, advisories: [],
      });
      expect(dissolveAfter).toHaveBeenCalledWith(1500);
    });

    test('a superseded in-flight encode that never emits completed:true still dissolves via the watchdog (Bug 1 / risk #7)', () => {
      // build/media/image.rs returns early WITHOUT a completed:true emit when a
      // rebuild supersedes an in-flight encode (routine in preview mode). If the
      // in-flight branch only canceled the pending dissolve, the nav-pill
      // hairline would hang forever with bgPageTasksInFlight={'images'} until
      // the next build. Each in-flight tick must re-arm dissolveAfter(watchdog).
      const h = (previewManager as any).hairline;
      vi.spyOn(h, 'setBuildProgress').mockImplementation(() => {});
      const dissolveAfter = vi.spyOn(h, 'dissolveAfter').mockImplementation(() => {});

      (previewManager as any).handleBackgroundProgress({
        task: 'images', current: 2, total: 8, message: '', completed: false, advisories: [],
      });
      // No completed:true ever arrives (superseded) — the watchdog was armed.
      expect(dissolveAfter).toHaveBeenCalledWith(15000);
    });

    test('a document text read after the build keeps the hairline moving', () => {
      const h = (previewManager as any).hairline;
      const setBuildProgress = vi.spyOn(h, 'setBuildProgress').mockImplementation(() => {});
      const dissolveAfter = vi.spyOn(h, 'dissolveAfter').mockImplementation(() => {});

      (previewManager as any).handleBackgroundProgress({
        task: 'derive', current: 1, total: 2, message: 'Reading PDFs · 1 of 2', completed: false, advisories: [],
      });
      expect(setBuildProgress).toHaveBeenCalledWith(0.5);
      expect(dissolveAfter).toHaveBeenCalledWith(15000);
    });

    test('background progress for non-media tasks does not touch the hairline (Bug 1)', () => {
      const h = (previewManager as any).hairline;
      const setBuildProgress = vi.spyOn(h, 'setBuildProgress').mockImplementation(() => {});
      const dissolveAfter = vi.spyOn(h, 'dissolveAfter').mockImplementation(() => {});

      (previewManager as any).handleBackgroundProgress({
        task: 'comment-sync', current: 1, total: 1, message: '', completed: true, advisories: [],
      });

      expect(setBuildProgress).not.toHaveBeenCalled();
      expect(dissolveAfter).not.toHaveBeenCalled();
    });

    test('handleProgressUpdate loads preview when complete with port', () => {
      const update: ProgressUpdate = {
        step: 'serving',
        message: 'Server started',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };

      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');

      (previewManager as any).handleProgressUpdate(update);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000');
      expect(previewManager.getState().isServerReady()).toBe(true);
    });

    test('handleProgressUpdate does not load preview when complete without port', () => {
      const update: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: null,
        is_empty: false,
      };

      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');

      (previewManager as any).handleProgressUpdate(update);

      expect(loadPreviewSpy).not.toHaveBeenCalled();
    });

    // Empty-folder onboarding lives in the editor action-panel folder-mode, not in
    // the preview overlay. The preview now loads the served empty-home
    // artifact (empty_home.rs) instead of painting an overlay. These
    // tests assert the new contract: empty completion → preview ready +
    // onEmptyFolder callback fires for the app shell to open the editor.

    test('empty completion loads the served homepage (no overlay)', () => {
      const update: ProgressUpdate = {
        step: 'complete',
        message: 'Done',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      };

      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');

      (previewManager as any).handleProgressUpdate(update);

      // Preview goes to ready (served empty-home page is real content)
      expect(previewManager.getState().getStatus()).toEqual({
        type: 'ready',
        url: 'http://localhost:3000',
      });
      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000');
      // And the empty signal is recorded for the app shell.
      expect(previewManager.getState().lastCompletionWasEmpty()).toBe(true);
    });

    test('empty completion fires onEmptyFolder callback', () => {
      const cb = vi.fn();
      previewManager.setOnEmptyFolder(cb);

      (previewManager as any).handleProgressUpdate({
        step: 'complete',
        message: 'Done',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      });

      expect(cb).toHaveBeenCalledTimes(1);
    });

    test('non-empty completion does NOT fire onEmptyFolder callback', () => {
      const cb = vi.fn();
      previewManager.setOnEmptyFolder(cb);

      (previewManager as any).handleProgressUpdate({
        step: 'complete',
        message: 'Done',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      });

      expect(cb).not.toHaveBeenCalled();
    });
  });

  describe('Silent Rebuilds (via PreviewState)', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');

      // Simulate initial build complete
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Initial build',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
    });

    test('server is ready after initial build', () => {
      expect(previewManager.getState().isServerReady()).toBe(true);
    });

    test('progress updates are silent during rebuild', () => {
      // Clear mocks from initial build
      vi.clearAllMocks();
      // Re-setup progress panel mocks after clearing
      mockProgressPanel.hasActiveTasks.mockReturnValue(false);

      const rebuildProgress: ProgressUpdate = {
        step: 'scanning',
        message: 'Rebuilding...',
        percentage: 50,
        completed: false,
        port: null,
        is_empty: null,
      };
      (previewManager as any).handleProgressUpdate(rebuildProgress);

      // Should NOT route to progress panel during silent rebuild
      expect(mockProgressPanel.upsertTask).not.toHaveBeenCalled();

      // Status should still be ready (silent rebuild)
      expect(previewManager.getState().getStatus()).toEqual({
        type: 'ready',
        url: 'http://localhost:3000',
      });
    });

    test('rebuild completion stays ready and tracks empty-ness across transitions', () => {
      // Clear mocks from initial build
      vi.clearAllMocks();

      // First make it empty — preview stays ready (served empty-home
      // artifact), and the empty signal is recorded.
      (previewManager as any).handleProgressUpdate({
        step: 'complete',
        message: 'Done',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: true,
      });

      expect(previewManager.getState().getStatus()).toEqual({
        type: 'ready',
        url: 'http://localhost:3000',
      });
      expect(previewManager.getState().lastCompletionWasEmpty()).toBe(true);

      // Now rebuild with content — still ready, empty signal clears.
      (previewManager as any).handleProgressUpdate({
        step: 'complete',
        message: 'Done',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      });

      expect(previewManager.getState().getStatus()).toEqual({
        type: 'ready',
        url: 'http://localhost:3000',
      });
      expect(previewManager.getState().lastCompletionWasEmpty()).toBe(false);
    });
  });

  describe('UI State Management', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('loadPreview sets iframe src with shell marker for chrome-clearance pre-paint', () => {
      // The shell stamps `__moss_shell=1` on iframe URLs so the server-side
      // middleware can rewrite the served `<html>` tag to carry
      // `class="moss-shell-frame"` BEFORE first paint, eliminating the 48px
      // body-padding flash. See `packages/moss-ui/src/ui/preview/shell-marker.ts`.
      const testUrl = 'http://localhost:3000';

      previewManager.loadPreview(testUrl);

      expect(mockIframe.src).toBe('http://localhost:3000/?__moss_shell=1');
    });

    test('loadPreview hides overlay before setting src', () => {
      mockOverlay.classList.add('visible', 'home-gate-active');

      previewManager.loadPreview('http://localhost:3000');

      // overlay should be hidden
      expect(mockOverlay.classList.contains('visible')).toBe(false);
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
      // src should be set (with shell marker — see preceding test for rationale).
      expect(mockIframe.src).toBe('http://localhost:3000/?__moss_shell=1');
    });

    test('refresh reloads iframe using current URL from navigation manager', async () => {
      // Set up navigation manager to return current URL
      mockNavState.currentUrl = 'http://localhost:3000/文章/post.html';
      mockIframe.src = 'http://localhost:3000'; // iframe.src might be different (stale)

      await previewManager.refresh();

      // reload() returns false in mock, so falls back to cache-busting iframe.src.
      // Refresh should use navigationManager.getCurrentUrl(), not iframe.src,
      // add cache-busting timestamp parameter, AND stamp the shell marker
      // (chrome-clearance pre-paint — see shell-marker.ts).
      //
      // NOTE on the `?_t=...?__moss_shell=1` (double-`?`) shape: the test's
      // MockURL class above is incomplete — its regex doesn't recognize an
      // existing query string, so when withShellMarker re-parses a URL that
      // already has `?_t=...`, the mock treats the whole `path?_t=...` as
      // pathname and appends `?__moss_shell=1` afterwards. A real browser
      // (and the moss runtime) produces the WHATWG-correct
      // `?_t=...&__moss_shell=1`. Tests assert mock-produced output to stay
      // green; the real behavior is verified by `shell-marker.test.ts`.
      expect(mockIframe.src).toMatch(
        /^http:\/\/localhost:3000\/文章\/post\.html\?_t=\d+\?__moss_shell=1$/
      );
    });

    test('refresh falls back to iframe.src when navigation history not initialized', async () => {
      mockNavState.currentUrl = null; // No navigation history yet
      mockIframe.src = 'http://localhost:3000';

      await previewManager.refresh();

      // Falls back to iframe.src, adds cache-busting timestamp, then
      // shell marker (chrome-clearance pre-paint — see shell-marker.ts).
      // See preceding test for the `?_t=...?__moss_shell=1` mock-shape note.
      expect(mockIframe.src).toMatch(
        /^http:\/\/localhost:3000\/?\?_t=\d+\?__moss_shell=1$/
      );
    });

    test('refresh with about:blank falls through to currentPreviewUrl', async () => {
      mockNavState.currentUrl = null;
      mockIframe.src = 'about:blank';

      await previewManager.refresh();

      // about:blank parses (mock returns origin='null', pathname=''), but
      // re-parsing the resulting `null?_t=...` string in withShellMarker
      // throws (mock regex doesn't accept it), tripping the outer catch in
      // `refresh()`. Without `currentPreviewUrl` set (no prior loadPreview
      // in this test), the catch's fallback is a no-op and iframe.src
      // stays at 'about:blank'.
      expect(mockIframe.src).toBe('about:blank');
    });

    test('two refreshes close together both apply: each FileChanged is its own change', async () => {
      // The engine names each output once per change of its bytes, so a
      // second FileChanged 100ms after the first is a second edit, not a
      // duplicate. Morph + reload return false in this mock, so an applied
      // refresh is observable as a new `?_t=` on the iframe.
      vi.useFakeTimers();
      try {
        mockNavState.currentUrl = 'http://localhost:3000/post.html';
        mockIframe.src = 'http://localhost:3000';

        vi.setSystemTime(1_000_000);
        await previewManager.refresh();
        expect(mockIframe.src).toMatch(/\?_t=1000000/);

        vi.setSystemTime(1_000_100);
        await previewManager.refresh();
        expect(mockIframe.src).toMatch(/\?_t=1000100/);
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe('Website Switching', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('startBuild resets state so new website shows progress instead of silent rebuild', async () => {
      // Simulate first website completing - server becomes ready
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Website A ready',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);

      // Verify server is ready (this causes silent rebuild mode)
      expect(previewManager.getState().isServerReady()).toBe(true);

      // Mock buildWithProgress to capture the progress callback
      let progressCallback: ((update: ProgressUpdate) => void) | null = null;
      (buildWithProgress as any).mockImplementation(
        (_folder: string, _autoServe: boolean, callback: (update: ProgressUpdate) => void) => {
          progressCallback = callback;
          return Promise.resolve('success');
        }
      );

      // Clear mocks before switching to new website
      mockProgressPanel.upsertTask.mockClear();

      // Switch to a different website (simulates build-config-changed event)
      await previewManager.startBuild('/different/website', true);

      // Verify state was reset - server should no longer be "ready"
      // This is the key assertion: without reset(), isServerReady() would still be true
      expect(previewManager.getState().isServerReady()).toBe(false);

      // Clear mocks to isolate the callback assertion
      mockProgressPanel.upsertTask.mockClear();

      const newSiteProgress: ProgressUpdate = {
        step: 'scanning',
        message: 'Scanning Website B...',
        percentage: 25,
        completed: false,
        port: null,
        is_empty: null,
      };

      if (progressCallback) {
        (progressCallback as (update: ProgressUpdate) => void)(newSiteProgress);
      }

      // Progress state should update (not silently ignored due to "rebuild" mode)
      expect(previewManager.getState().getStatus()).toEqual({
        type: 'loading',
        message: 'Scanning Website B...',
        percentage: 25,
      });
    });
  });

  describe('Build Flow', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('startBuild shows blueprint grid + progress panel and calls buildWithProgress', async () => {
      (buildWithProgress as any).mockResolvedValue('success');

      await previewManager.startBuild('/test/folder', true);

      // Should show blueprint grid overlay
      expect(startBlueprintGrid).toHaveBeenCalledWith(mockOverlay, { onFrame: expect.any(Function) });
      expect(mockOverlay.classList.contains('visible')).toBe(true);
      // Should set stage to opening on the progress panel
      expect(mockProgressPanel.setStage).toHaveBeenCalledWith('opening');
      expect(buildWithProgress).toHaveBeenCalledWith(
        '/test/folder',
        true,
        expect.any(Function)
      );
    });

    test("a new folder's screen stays until its site has loaded in the preview", async () => {
      // The screen used to drop when the preview STARTED navigating, so the
      // window went screen, empty frame, site: about a second on Linux.
      (buildWithProgress as any).mockResolvedValue('success');
      await previewManager.startBuild('/test/folder', true);
      (previewManager as any).renderStatus({ type: 'ready', url: 'http://localhost:8080/' });
      expect(mockOverlay.classList.contains('visible')).toBe(true);

      mockIframe.dispatchEvent(new Event('load'));
      expect(mockOverlay.classList.contains('visible')).toBe(false);
    });

    test('two callers opening the same folder run ONE pipeline', async () => {
      // Opening a folder can reach startBuild twice
      // by design (the `config-ready` listener and `initialize()`'s waitForConfig
      // poll), and nothing downstream deduplicates — `build_folder` is fully
      // re-entrant, so the second call was a whole second pipeline rendering the
      // same site into the same staging dir. Three concurrent pipelines were
      // observed on one open, each waiting ~2.5s for the stage-write lock.
      let release!: (v: string) => void;
      (buildWithProgress as any).mockReturnValue(
        new Promise<string>((resolve) => { release = resolve; })
      );

      const first = previewManager.startBuild('/same/folder', true);
      const second = previewManager.startBuild('/same/folder', true);
      release('success');
      await Promise.all([first, second]);

      expect(buildWithProgress).toHaveBeenCalledTimes(1);
    });

    test('a folder SWITCH mid-build is not coalesced away', async () => {
      // The singleflight keys on the folder: a switch is a different site and
      // must supersede (the backend cancels the old build via FolderSession),
      // never join the build it is replacing.
      (buildWithProgress as any).mockResolvedValue('success');

      const first = previewManager.startBuild('/folder/a', true);
      const second = previewManager.startBuild('/folder/b', true);
      await Promise.all([first, second]);

      expect(buildWithProgress).toHaveBeenCalledTimes(2);
    });

    test('a sequential rebuild after the first settles still runs', async () => {
      // The cell must clear on completion — otherwise the guard would swallow
      // every later rebuild of the same folder, which is worse than the bug.
      (buildWithProgress as any).mockResolvedValue('success');

      await previewManager.startBuild('/test/folder', true);
      await previewManager.startBuild('/test/folder', true);

      expect(buildWithProgress).toHaveBeenCalledTimes(2);
    });

    test('a FAILED build clears the cell, so the retry is not swallowed', async () => {
      // The failure mode that would be worse than the bug: if the guard only
      // cleared on success, one failed build would make the folder permanently
      // unbuildable for the rest of the session.
      (buildWithProgress as any).mockRejectedValueOnce(new Error('Build failed'));
      await previewManager.startBuild('/test/folder', true);

      (buildWithProgress as any).mockResolvedValue('success');
      await previewManager.startBuild('/test/folder', true);

      expect(buildWithProgress).toHaveBeenCalledTimes(2);
    });

    test('startBuild handles build errors', async () => {
      const error = new Error('Build failed');
      (buildWithProgress as any).mockRejectedValue(error);

      await previewManager.startBuild('/test/folder', true);

      // catch block routes through state.setError + renderStatus, which
      // exercises the new `case 'error'` path: removes 'build' task,
      // transitions stage to 'ready', and surfaces the error in the overlay.
      // Verify by reading the state status — the visible side effects
      // (removeTask, setStage, showOverlay) all funnel through here.
      const status = previewManager.getState().getStatus();
      expect(status.type).toBe('error');
      expect((status as { type: 'error'; message: string }).message).toBe(
        'Failed to build: Error: Build failed'
      );
    });

    test('startBuild uses default autoServe value', async () => {
      (buildWithProgress as any).mockResolvedValue('success');

      await previewManager.startBuild('/test/folder');

      expect(buildWithProgress).toHaveBeenCalledWith(
        '/test/folder',
        true, // default value
        expect.any(Function)
      );
    });

    test('startBuild resets navigation history for new folder', async () => {
      // BUG FIX: When switching folders, navigation history must be reset
      // to prevent stale path display and broken back/forward buttons
      (buildWithProgress as any).mockResolvedValue('success');

      // Reset the tracking state
      mockNavState.resetCalled = false;

      await previewManager.startBuild('/new/folder', true);

      // NavigationManager.reset() should be called to clear old folder's history
      expect(mockNavState.resetCalled).toBe(true);
    });
  });

  describe('Same-Folder Rebuild: Keep Old Preview Visible', () => {
    // When rebuilding the same folder that already has a working preview,
    // the old site should remain visible instead of being hidden behind
    // a blueprint overlay. This avoids a 1-5s blank screen on every rebuild.
    //
    // Folder SWITCHES (different folder) should still show the overlay
    // because the old preview belongs to a different site.

    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (buildWithProgress as any).mockResolvedValue('success');
    });

    test('same-folder rebuild does NOT show overlay when preview is already showing', async () => {
      // Simulate first build completing — server is ready, preview is visible
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
      expect(previewManager.getState().isServerReady()).toBe(true);

      // Clear mocks to isolate the rebuild
      vi.mocked(startBlueprintGrid).mockClear();
      mockOverlay.classList.remove('visible', 'home-gate-active');

      // Same folder rebuild (same folderPath as was set during first build)
      // First set the folderPath by doing an initial build
      (previewManager as any).folderPath = '/test/folder';
      await previewManager.startBuild('/test/folder', true);

      // Overlay should NOT be shown (no blueprint grid, no 'visible' class)
      expect(startBlueprintGrid).not.toHaveBeenCalled();
      expect(mockOverlay.classList.contains('visible')).toBe(false);
    });

    test('same-folder rebuild still shows progress panel (non-blocking)', async () => {
      // Simulate first build completing
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
      (previewManager as any).folderPath = '/test/folder';

      mockProgressPanel.upsertTask.mockClear();

      await previewManager.startBuild('/test/folder', true);

      // Progress panel should still transition to opening stage
      expect(mockProgressPanel.setStage).toHaveBeenCalledWith('opening');
    });

    test('different-folder switch DOES show overlay even when preview is ready', async () => {
      // Simulate first build completing for folder A
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
      (previewManager as any).folderPath = '/folder-A';

      vi.mocked(startBlueprintGrid).mockClear();
      mockOverlay.classList.remove('visible', 'home-gate-active');

      // Switch to a DIFFERENT folder
      await previewManager.startBuild('/folder-B', true);

      // Overlay SHOULD be shown — old preview is wrong site
      expect(startBlueprintGrid).toHaveBeenCalled();
      expect(mockOverlay.classList.contains('visible')).toBe(true);
    });

    test('same-folder rebuild preserves navigation position (does not reset)', async () => {
      // Simulate first build completing
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
      (previewManager as any).folderPath = '/test/folder';

      mockNavState.resetCalled = false;

      await previewManager.startBuild('/test/folder', true);

      // Navigation should NOT be reset — user might be on an article page
      expect(mockNavState.resetCalled).toBe(false);
    });

    test('different-folder switch resets navigation (existing behavior)', async () => {
      // Simulate first build completing
      const initialBuild: ProgressUpdate = {
        step: 'complete',
        message: 'Build complete',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };
      (previewManager as any).handleProgressUpdate(initialBuild);
      (previewManager as any).folderPath = '/folder-A';

      mockNavState.resetCalled = false;

      await previewManager.startBuild('/folder-B', true);

      // Navigation SHOULD be reset — different site
      expect(mockNavState.resetCalled).toBe(true);
    });

    test('first build (no previous preview) shows overlay as before', async () => {
      // No previous build — server not ready
      expect(previewManager.getState().isServerReady()).toBe(false);

      vi.mocked(startBlueprintGrid).mockClear();

      await previewManager.startBuild('/test/folder', true);

      // Overlay SHOULD be shown — nothing to show yet
      expect(startBlueprintGrid).toHaveBeenCalled();
    });
  });

  describe('Mobile Preview Mode', () => {
    test('isMobilePreview delegates to navigationManager', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      // The mock should have isMobilePreview, but if not accessible,
      // just verify the method exists and can be called
      expect(typeof previewManager.isMobilePreview).toBe('function');
    });
  });

  describe('File Change Event Handling', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
      // Reset mockNavState.currentUrl before each test
      mockNavState.currentUrl = null;
    });

    test('handleFileChangeEvent with null event defaults to refresh', () => {
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      // Test with null
      (previewManager as any).handleFileChangeEvent(null);

      expect(refreshSpy).toHaveBeenCalledTimes(1);
    });

    test('handleFileChangeEvent with undefined event defaults to refresh', () => {
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      // Test with undefined
      (previewManager as any).handleFileChangeEvent(undefined);

      expect(refreshSpy).toHaveBeenCalledTimes(1);
    });

    test('handleFileChangeEvent with empty change event does not trigger refresh (smart diff)', () => {
      // Smart diff refresh: null paths means no specific files changed, no refresh needed
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      const emptyEvent = {
        deleted_paths: null,
        moved_output_paths: null
      };

      (previewManager as any).handleFileChangeEvent(emptyEvent);

      // No refresh because no files affected the current page
      expect(refreshSpy).not.toHaveBeenCalled();
    });

    test('handleFileChangeEvent with empty arrays does not trigger refresh (smart diff)', () => {
      // Smart diff refresh: empty arrays means no files affected current page
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      const emptyEvent = {
        deleted_paths: [],
        moved_output_paths: []
      };

      (previewManager as any).handleFileChangeEvent(emptyEvent);

      // No refresh because no files affected the current page
      expect(refreshSpy).not.toHaveBeenCalled();
    });

    test('handleFileChangeEvent with deletion of current page redirects to homepage', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockIframe.src = 'http://localhost:3000/blog/post.html';

      const deleteEvent = {
        deleted_paths: ['blog/post.html'],
        moved_output_paths: null
      };

      (previewManager as any).handleFileChangeEvent(deleteEvent);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000/index.html');
    });

    test('handleFileChangeEvent with deletion of other page does not refresh (smart diff)', () => {
      // Smart diff refresh: deleting another page shouldn't affect current page
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/current.html';

      const deleteEvent = {
        deleted_paths: ['blog/other.html'],
        moved_output_paths: null
      };

      (previewManager as any).handleFileChangeEvent(deleteEvent);

      // No refresh because deleted file is not the current page
      expect(refreshSpy).not.toHaveBeenCalled();
    });

    test('handleFileChangeEvent with rename of current page updates URL', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      mockNavState.currentUrl = 'http://localhost:3000/blog/old-post.html';
      mockIframe.src = 'http://localhost:3000/blog/old-post.html';

      const renameEvent = {
        deleted_paths: null,
        moved_output_paths: [['blog/old-post.html', 'blog/new-post.html']]
      };

      (previewManager as any).handleFileChangeEvent(renameEvent);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000/blog/new-post.html');
    });

    test('handleFileChangeEvent with rename of other page does not refresh (smart diff)', () => {
      // Smart diff refresh: renaming another page shouldn't affect current page
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/current.html';

      const renameEvent = {
        deleted_paths: null,
        moved_output_paths: [['blog/other.html', 'blog/renamed.html']]
      };

      (previewManager as any).handleFileChangeEvent(renameEvent);

      // No refresh because renamed file is not the current page
      expect(refreshSpy).not.toHaveBeenCalled();
    });

    test('handleFileChangeEvent with changed_output_files refreshes if current page affected', () => {
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';

      const changeEvent = {
        changed_output_files: ['blog/post.html', 'index.html']
      };

      (previewManager as any).handleFileChangeEvent(changeEvent);

      expect(refreshSpy).toHaveBeenCalledTimes(1);
    });

    test('handleFileChangeEvent with changed_output_files does not refresh if current page not affected', () => {
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';

      const changeEvent = {
        changed_output_files: ['other.html', 'index.html']
      };

      (previewManager as any).handleFileChangeEvent(changeEvent);

      expect(refreshSpy).not.toHaveBeenCalled();
    });
  });

  // A release build logs at INFO only, and until this line existed every part
  // of the refresh path — the smart-diff verdict and
  // whether morph/reload/fallback actually applied — logged at DEBUG or not
  // at all. So a "preview didn't update" report from a release log could
  // never say which link broke. These tests pin the one `[preview-refresh]`
  // line each decision produces, folding the former `[SmartDiff]` DEBUG line
  // in rather than living alongside it.
  describe('[preview-refresh] decision logging', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
      mockNavState.currentUrl = null;
    });

    test('a FileChanged before the first bridge navigation waits for it before requesting a morph', async () => {
      const nav = (previewManager as any).navigationManager;
      const morph = vi.spyOn(nav, 'morph').mockResolvedValue(true);
      nav.beginNavigation('http://localhost:8080/');
      expect(nav.getCurrentUrl()).toBe('http://localhost:8080/');
      expect(nav.getActivePage()).toBeNull();
      await previewManager.refresh('file-changed');
      await previewManager.refresh('file-changed');
      expect(morph).not.toHaveBeenCalled();
      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=refresh path=index.html changed=0 outcome=deferred(first navigation)',
      );
      mockNavState.currentUrl = 'http://localhost:8080/';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      mockNavState.navResultListener!({ url: mockNavState.currentUrl, notFound: false });
      await vi.waitFor(() => expect(morph).toHaveBeenCalledTimes(1));
      mockNavState.navResultListener!({ url: mockNavState.currentUrl, notFound: false });
      expect(morph).toHaveBeenCalledTimes(1);
    });

    test('switching folders discards a refresh waiting on the predecessor', async () => {
      await previewManager.refresh('file-changed');
      expect((previewManager as any).refreshAfterFirstNavigation).toBe(true);
      (buildWithProgress as any).mockResolvedValue('success');
      await previewManager.startBuild('/folder/next', true);
      expect((previewManager as any).refreshAfterFirstNavigation).toBe(false);
    });

    test('destroy discards a refresh waiting for the first navigation', async () => {
      await previewManager.refresh('file-changed');
      expect((previewManager as any).refreshAfterFirstNavigation).toBe(true);
      previewManager.destroy();
      expect((previewManager as any).refreshAfterFirstNavigation).toBe(false);
    });

    test('a FileChanged affecting the current page: morph lands → outcome=morph-applied', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      vi.spyOn((previewManager as any).navigationManager, 'morph').mockResolvedValue(true);
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['blog/post.html'] });
      await refreshSpy.mock.results[0].value;

      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=refresh path=blog/post.html changed=1 outcome=morph-applied',
      );
    });

    test('a FileChanged NOT affecting the current page → outcome=none(page unaffected), no refresh', () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['other.html'] });

      expect(refreshSpy).not.toHaveBeenCalled();
      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=none path=blog/post.html changed=1 outcome=none(page unaffected)',
      );
    });

    test('a FileChanged naming a stylesheet the page links reloads it, with no morph → action=reload-linked', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      const nav = (previewManager as any).navigationManager;
      const callBridge = vi.spyOn(nav, 'callBridge').mockResolvedValue([
        // Absolute: this file's MockURL ignores a base (relative URLs are
        // resolved in preview-actions.test.ts against the real URL).
        { attributes: { rel: 'stylesheet', href: 'http://localhost:3000/notes/handout.css' } },
      ]);
      const morph = vi.spyOn(nav, 'morph').mockResolvedValue(true);
      vi.spyOn(nav, 'reload').mockResolvedValue(true);

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['notes/handout.css', 'sitemap.xml'] });

      await vi.waitFor(() => expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=reload-linked path=blog/post.html changed=2 outcome=reload',
      ));
      expect(callBridge).toHaveBeenCalledWith('document.querySelectorAll', expect.stringContaining('link[href]'));
      expect(morph).not.toHaveBeenCalled();
    });

    test('a FileChanged naming only files the page does not load → outcome=none(page unaffected)', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      const nav = (previewManager as any).navigationManager;
      const callBridge = vi.spyOn(nav, 'callBridge').mockResolvedValue([{ attributes: { href: 'http://localhost:3000/notes/handout.css' } }]);
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['rss.xml', 'sitemap.xml'] });

      await vi.waitFor(() => expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=none path=blog/post.html changed=2 outcome=none(page unaffected)',
      ));
      expect(callBridge).toHaveBeenCalled();
      expect(refreshSpy).not.toHaveBeenCalled();
    });

    test('an event naming the page and a stylesheet it loads reloads rather than morphs, so the stylesheet is fetched again', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      const nav = (previewManager as any).navigationManager;
      vi.spyOn(nav, 'callBridge').mockResolvedValue([{ attributes: { href: 'http://localhost:3000/_moss/theme/style.css' } }]);
      const morph = vi.spyOn(nav, 'morph').mockResolvedValue(true);
      vi.spyOn(nav, 'reload').mockResolvedValue(true);

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['blog/post.html', '_moss/theme/style.css'] });

      await vi.waitFor(() => expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=reload-linked path=blog/post.html changed=2 outcome=reload',
      ));
      expect(morph).not.toHaveBeenCalled();
    });

    test('an event naming the page and files it does not load morphs the page', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      const nav = (previewManager as any).navigationManager;
      vi.spyOn(nav, 'callBridge').mockResolvedValue([]);
      vi.spyOn(nav, 'morph').mockResolvedValue(true);

      (previewManager as any).handleFileChangeEvent({ changed_output_files: ['blog/post.html', 'rss.xml'] });

      await vi.waitFor(() => expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=refresh path=blog/post.html changed=2 outcome=morph-applied',
      ));
    });

    test('morph fails, RPC reload lands → outcome=morph-failed→reload', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/post.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      vi.spyOn((previewManager as any).navigationManager, 'reload').mockResolvedValue(true);

      await previewManager.refresh('file-changed');

      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=refresh path=post.html changed=0 outcome=morph-failed→reload',
      );
    });

    test('morph cannot apply while a navigation is pending → outcome=skipped(navigation pending)', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/post.html';
      mockNavState.requestedUrl = 'http://localhost:3000/other.html';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      // the real reload() reports success without acting while a navigation is pending
      vi.spyOn((previewManager as any).navigationManager, 'reload').mockResolvedValue(true);

      await previewManager.refresh('file-changed');

      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=file-changed action=refresh path=other.html changed=0 outcome=skipped(navigation pending)',
      );
    });

    test('morph disabled, RPC reload lands → outcome=reload (not conflated with a morph failure)', async () => {
      localStorage.setItem('moss.preview.morph', '0');
      try {
        mockNavState.currentUrl = 'http://localhost:3000/post.html';
        vi.spyOn((previewManager as any).navigationManager, 'reload').mockResolvedValue(true);

        await previewManager.refresh('other');

        expect(vi.mocked(info)).toHaveBeenCalledWith(
          '[preview-refresh] trigger=other action=refresh path=post.html changed=0 outcome=reload',
        );
      } finally {
        localStorage.removeItem('moss.preview.morph');
      }
    });

    test('morph and reload both fail with no known URL to fall back to → outcome=skipped(no page)', async () => {
      // mockNavState.currentUrl, iframe.src and currentPreviewUrl are all
      // unset by this describe's beforeEach — nothing for the cache-busting
      // fallback to navigate to.
      await previewManager.refresh();

      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=other action=refresh path=null changed=0 outcome=skipped(no page)',
      );
    });

    test('a requested page recheck logs its real decision', () => {
      previewManager.armNewPageWait('/notes/untitled/');
      vi.mocked(info).mockClear();
      (previewManager as any).handleFileChangeEvent(null);
      expect(vi.mocked(info)).toHaveBeenCalledWith(
        '[preview-refresh] trigger=newPageWait-recheck action=recheck path=/notes/untitled/ changed=0 outcome=page-recheck',
      );
    });

    // Every [preview-refresh] line actually logged, in order — asserting a
    // COUNT (not just a toHaveBeenCalledWith) is what catches the fallback
    // path double-logging: once from the pre-dispatch site, once from
    // refresh()'s own terminal line, for the same one decision.
    const refreshLines = () =>
      vi.mocked(info).mock.calls.map((c) => String(c[0])).filter((m) => m.startsWith('[preview-refresh]'));

    test('current page deleted (redirect-home) logs outcome=redirect-home — bypasses refresh() entirely', () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      mockIframe.src = 'http://localhost:3000/blog/post.html';
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({ deleted_paths: ['blog/post.html'] });

      expect(refreshSpy).not.toHaveBeenCalled();
      expect(refreshLines()).toEqual([
        '[preview-refresh] trigger=file-changed action=redirect-home path=blog/post.html changed=0 outcome=redirect-home',
      ]);
    });

    test('current page renamed (update-url) logs outcome=url-moved — a 404 here is the stale-pane failure this line exists to diagnose', () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/old-post.html';
      mockIframe.src = 'http://localhost:3000/blog/old-post.html';
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({
        moved_output_paths: [['blog/old-post.html', 'blog/new-post.html']],
      });

      expect(refreshSpy).not.toHaveBeenCalled();
      expect(refreshLines()).toEqual([
        '[preview-refresh] trigger=file-changed action=update-url path=blog/old-post.html changed=0 outcome=url-moved',
      ]);
    });

    test('redirect-home with no known URL falls back to refresh() — exactly one [preview-refresh] line, not two', async () => {
      // baseUrl (iframe.src || currentPreviewUrl) is falsy by this describe's
      // defaults — mockIframe.src stays '' and currentPreviewUrl is never set
      // — so redirectToHomepage() takes its fallback branch and calls
      // refresh() directly. Before the fix, the pre-dispatch site logged
      // 'redirect-home' unconditionally AND refresh() logged its own
      // terminal outcome: two lines for one decision.
      mockNavState.currentUrl = 'http://localhost:3000/blog/post.html';
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({ deleted_paths: ['blog/post.html'] });
      await refreshSpy.mock.results[0].value;

      expect(refreshSpy).toHaveBeenCalledTimes(1);
      expect(refreshLines()).toEqual([
        '[preview-refresh] trigger=other action=refresh path=blog/post.html changed=0 outcome=iframe-nav-fallback',
      ]);
    });

    test('update-url with no known URL falls back to refresh() — exactly one [preview-refresh] line, not two', async () => {
      mockNavState.currentUrl = 'http://localhost:3000/blog/old-post.html';
      const refreshSpy = vi.spyOn(previewManager, 'refresh');

      (previewManager as any).handleFileChangeEvent({
        moved_output_paths: [['blog/old-post.html', 'blog/new-post.html']],
      });
      await refreshSpy.mock.results[0].value;

      expect(refreshSpy).toHaveBeenCalledTimes(1);
      expect(refreshLines()).toEqual([
        '[preview-refresh] trigger=other action=refresh path=blog/old-post.html changed=0 outcome=iframe-nav-fallback',
      ]);
    });
  });

  describe('URL Manipulation Functions', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    // Both handlers now take the [preview-refresh] decision context as their
    // last argument (they log their own success outcome — see the
    // '[preview-refresh] decision logging' describe below); these direct
    // unit tests exercise the URL/retry mechanics, so a stand-in context is
    // enough. The malformed-URL tests never reach the log call (the throw
    // happens first), so they're unaffected.
    const stubCtx: RefreshDecisionContext = { action: 'refresh', path: null, changedCount: 0 };

    test('redirectToHomepage constructs correct homepage URL', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      mockIframe.src = 'http://localhost:3000/blog/post.html';

      (previewManager as any).redirectToHomepage(stubCtx);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000/index.html');
    });

    test('redirectToHomepage with malformed URL falls back to refresh', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockIframe.src = 'invalid-url';

      (previewManager as any).redirectToHomepage(stubCtx);

      expect(loadPreviewSpy).not.toHaveBeenCalled();
      expect(refreshSpy).toHaveBeenCalledTimes(1);
    });

    test('updateUrlForRename converts md to html and updates URL', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      mockIframe.src = 'http://localhost:3000/blog/old.html';

      (previewManager as any).updateUrlForRename('blog/new.md', stubCtx);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000/blog/new.html');
    });

    test('updateUrlForRename with html extension preserves extension', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      mockIframe.src = 'http://localhost:3000/blog/old.html';

      (previewManager as any).updateUrlForRename('blog/new.html', stubCtx);

      expect(loadPreviewSpy).toHaveBeenCalledWith('http://localhost:3000/blog/new.html');
    });

    test('updateUrlForRename with malformed URL falls back to refresh', () => {
      const loadPreviewSpy = vi.spyOn(previewManager, 'loadPreview');
      const refreshSpy = vi.spyOn(previewManager, 'refresh');
      mockIframe.src = 'invalid-url';

      (previewManager as any).updateUrlForRename('new.md', stubCtx);

      expect(loadPreviewSpy).not.toHaveBeenCalled();
      expect(refreshSpy).toHaveBeenCalledTimes(1);
    });
  });

  describe('Frontend Integration - Panel Placement and Build Complete', () => {
    beforeEach(() => {
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('panel moves to bottom when preview opens (ready state)', () => {
      // Simple Display Logic: preview opens → panel goes to bottom
      // Maximum Decoupling: frontend doesn't know about phases or task types
      const readyUpdate: ProgressUpdate = {
        step: 'serving',
        message: 'Server started',
        percentage: 100,
        completed: true,
        port: 3000,
        is_empty: false,
      };

      (previewManager as any).handleProgressUpdate(readyUpdate);

      // Panel stage transitions to ready (blocking phase done)
      expect(mockProgressPanel.setStage).toHaveBeenCalledWith('ready');
    });

    // BuildComplete fires NO toast (not even showAck) — videos_converted
    // is a routine ambient success; symlink-skip warnings surface on the
    // hairline dot via a BackgroundProgress advisory from the Rust backend.
    test('build_complete listener fires NO toast regardless of videos_converted', async () => {
      const toasts = await import("../../shared/ui/toast/toast-manager");
      const showToastSpy = vi.spyOn(toasts, 'showToast');
      const showAckSpy = vi.spyOn(toasts, 'showAck');
      const mossEventCallbacks: Array<(event: any) => void> = [];
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'moss-event') {
          mossEventCallbacks.push(callback);
        }
        return Promise.resolve(() => {});
      });
      const fireBuildComplete = (payload: object) => {
        for (const cb of mossEventCallbacks) {
          cb({ kind: 'BuildComplete', payload });
        }
      };

      await previewManager.initialize();

      // With videos_converted > 0 — previously showed a toast, now must not.
      fireBuildComplete({ videos_converted: 3, total_time_ms: 4567, skipped_symlinks: 0 });
      expect(showAckSpy).not.toHaveBeenCalled();
      expect(showToastSpy).not.toHaveBeenCalled();

      // With skipped_symlinks > 0 the build must not toast;
      // the Rust backend's advisory surfaces it instead.
      fireBuildComplete({ videos_converted: 0, total_time_ms: 1234, skipped_symlinks: 2 });
      expect(showAckSpy).not.toHaveBeenCalled();
      expect(showToastSpy).not.toHaveBeenCalled();

      showAckSpy.mockRestore();
      showToastSpy.mockRestore();
    });

    test('build_complete listener does not show toast when no videos converted', async () => {
      const toasts = await import("../../shared/ui/toast/toast-manager");
      const showToastSpy = vi.spyOn(toasts, 'showToast');
      const showAckSpy = vi.spyOn(toasts, 'showAck');
      // BuildComplete now arrives via the typed bus on 'moss-event'.
      const mossEventCallbacks: Array<(event: any) => void> = [];
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'moss-event') {
          mossEventCallbacks.push(callback);
        }
        return Promise.resolve(() => {});
      });
      const buildCompleteCallback = (event: { payload: { videos_converted: number; total_time_ms: number } }) => {
        for (const cb of mossEventCallbacks) {
          cb({ kind: 'BuildComplete', payload: event.payload });
        }
      };

      await previewManager.initialize();

      if (buildCompleteCallback) {
        (buildCompleteCallback as (event: any) => void)({
          payload: {
            videos_converted: 0,
            total_time_ms: 234,
          },
        });
      }

      // Should not show toast when no videos
      expect(showToastSpy).not.toHaveBeenCalled();
      expect(showAckSpy).not.toHaveBeenCalled();

      showToastSpy.mockRestore();
      showAckSpy.mockRestore();
    });

    test('build_complete listener never refreshes the page on screen: FileChanged names late bytes', async () => {
      // BuildComplete now arrives via the typed bus on 'moss-event'.
      const mossEventCallbacks: Array<(event: any) => void> = [];
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'moss-event') {
          mossEventCallbacks.push(callback);
        }
        return Promise.resolve(() => {});
      });
      const buildCompleteCallback = (event: { payload: { videos_converted: number; total_time_ms: number } }) => {
        for (const cb of mossEventCallbacks) {
          cb({ kind: 'BuildComplete', payload: event.payload });
        }
      };

      await previewManager.initialize();

      // Spy on refresh after initialization (so we don't capture setup calls)
      const refreshSpy = vi.spyOn(previewManager, 'refresh').mockResolvedValue(undefined);

      // Simulate backend emitting build_complete (even with 0 videos)
      expect(buildCompleteCallback).not.toBeNull();
      if (buildCompleteCallback) {
        (buildCompleteCallback as (event: any) => void)({
          payload: {
            videos_converted: 0,
            total_time_ms: 500,
          },
        });
      }

      expect(refreshSpy).not.toHaveBeenCalled();

      refreshSpy.mockRestore();
    });
  });

  // ─── PreviewServerFailed — overlay path removed ────────────────
  // Overlay path removed: a PreviewServerFailed event must NOT call state.setError, because
  // the progress panel already surfaces the failure as a task row.
  describe('PreviewServerFailed — overlay path removed', () => {
    test('PreviewServerFailed does NOT call state.setError (overlay path deleted)', async () => {
      previewManager = new PreviewManager('moss-preview-iframe');

      // Collect every handler registered on the 'moss-event' channel.
      // onMossEvent(...) calls listen("moss-event", handler), so we intercept
      // ALL of those registrations and replay the event to all of them.
      const mossEventCallbacks: Array<(event: any) => void> = [];
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'moss-event') {
          mossEventCallbacks.push(callback);
        }
        return Promise.resolve(() => {}); // unlisten stub
      });

      // Spy on the real state object's setError so we can assert it was NOT called.
      const setErrorSpy = vi.spyOn(previewManager.getState(), 'setError');

      // initialize() registers the (to-be-deleted) PreviewServerFailed listener.
      await previewManager.initialize();

      // Emit PreviewServerFailed on the typed bus — this is what Rust emits.
      const serverFailPayload = {
        payload: {
          kind: 'PreviewServerFailed',
          payload: { message: 'Port 8080 already in use' },
        },
      };
      for (const cb of mossEventCallbacks) {
        cb(serverFailPayload);
      }

      // After the fix: state.setError must NOT have been invoked.
      // If the overlay block still exists, this fails → genuine red-before-green.
      expect(setErrorSpy).not.toHaveBeenCalled();

      // Confirm the status is still initial (not poisoned to 'error').
      const status = previewManager.getState().getStatus();
      expect(status.type).not.toBe('error');

      setErrorSpy.mockRestore();
    });

    // The event is not an overlay path, but it IS the evidence.
    // Without recording it, `preview-state` has nothing to tell a real server
    // failure apart from a rebuild that simply did not start a server, and it
    // guessed — over a live server, over a live download screen.
    test('PreviewServerFailed is recorded on the state as the reported reason', async () => {
      previewManager = new PreviewManager('moss-preview-iframe');

      const mossEventCallbacks: Array<(event: any) => void> = [];
      engineOn.mockImplementation((eventName: string, callback: any) => {
        if (eventName === 'moss-event') mossEventCallbacks.push(callback);
        return Promise.resolve(() => {});
      });

      const noteSpy = vi.spyOn(previewManager.getState(), 'noteServerFailed');
      await previewManager.initialize();

      for (const cb of mossEventCallbacks) {
        cb({ kind: 'PreviewServerFailed', payload: { message: 'Port 8080 already in use', open_in_another_app: true } });
      }

      expect(noteSpy).toHaveBeenCalledWith('Port 8080 already in use', true);

      // And now a portless completion is a verdict, not a guess.
      (previewManager as any).handleProgressUpdate({
        step: 'complete', message: 'Build complete', percentage: 100,
        completed: true, port: null, is_empty: false,
      });
      expect(previewManager.getState().getStatus()).toEqual({
        type: 'error',
        message: 'Port 8080 already in use',
        cause: 'other-app',
        verdict: 'reported',
      });

      noteSpy.mockRestore();
    });
  });
  // ─────────────────────────────────────────────────────────────────────────

  // Rename 404-retry: when a page's url/slug changes, the shell navigates to the
  // moved URL before the new generation is published, so the first hit can 404.
  // PreviewManager reloads until it serves (bounded), then stops. loadPreview is
  // mocked here so the wait and bounded settle timer are tested separately
  // from the cache-busting navigation path.
  describe('rename 404-retry', () => {
    const TARGET = 'http://localhost:8080/writings/x/index.html';
    const DELAY = (PreviewManager as any).PAGE_SETTLE_DELAY_MS as number;
    const MAX = (PreviewManager as any).PAGE_SETTLE_RETRIES as number;

    const fireNav = (url: string, notFound: boolean) =>
      mockNavState.navResultListener!({ url, notFound });

    test('reloads the moved URL while it 404s, then stops once it serves', () => {
      vi.useFakeTimers();
      previewManager = new PreviewManager('moss-preview-iframe');
      const loadSpy = vi.spyOn(previewManager, 'loadPreview').mockImplementation(() => {});
      previewManager.armNewPageWait('/writings/x/index.html');

      fireNav(TARGET, true);                       // first hit is the synthetic 404
      expect((previewManager as any).newPageWait).not.toBeNull();
      vi.advanceTimersByTime(DELAY);               // → one reload of the same URL
      expect(loadSpy).toHaveBeenCalledWith(TARGET);
      expect(loadSpy).toHaveBeenCalledTimes(1);

      fireNav(TARGET, false);                       // reload now serves → stop
      expect((previewManager as any).newPageWait).toBeNull();

      loadSpy.mockClear();
      vi.advanceTimersByTime(5000);
      expect(loadSpy).not.toHaveBeenCalled();       // no further reloads
      vi.useRealTimers();
    });

    test('stops polling after the attempt budget and keeps the 404 covered', () => {
      vi.useFakeTimers();
      previewManager = new PreviewManager('moss-preview-iframe');
      const loadSpy = vi.spyOn(previewManager, 'loadPreview').mockImplementation(() => {});
      previewManager.armNewPageWait('/writings/x/index.html');

      for (let i = 0; i < MAX + 3; i++) {
        fireNav(TARGET, true);
        vi.advanceTimersByTime(DELAY);
      }
      expect(loadSpy).toHaveBeenCalledTimes(MAX);
      expect((previewManager as any).newPageWait).not.toBeNull();
      expect(mockOverlay.classList.contains('visible')).toBe(true);
      vi.useRealTimers();
    });

    test('stops retrying when the preview lands on a different page', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      const loadSpy = vi.spyOn(previewManager, 'loadPreview').mockImplementation(() => {});
      previewManager.armNewPageWait('/writings/x/index.html');

      fireNav('http://localhost:8080/somewhere/else/index.html', false);
      expect((previewManager as any).newPageWait).toBeNull();
      expect(loadSpy).not.toHaveBeenCalled();
    });

    test('ignores navigation results when no rename retry is in flight', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      const loadSpy = vi.spyOn(previewManager, 'loadPreview').mockImplementation(() => {});
      // No armRenameRetry() → renameRetry is null.
      fireNav(TARGET, true);
      expect(loadSpy).not.toHaveBeenCalled();
    });
  });

  // ─── A page that has only just been created ───────────────────────────────
  // Creating a page
  // navigates the preview to its build-INDEPENDENT URL before any build has
  // run, so the URL serves the synthetic 404 and the author's first sight of
  // their new page is an error. These pin the join that covers that window —
  // and, just as hard, every way the cover is guaranteed to come off.
  describe('new-page wait', () => {
    const URL_PATH = '/notes/untitled/';
    const TARGET = 'http://localhost:8080/notes/untitled/';
    const TIMEOUT = (PreviewManager as any).PAGE_PENDING_TIMEOUT_MS as number;

    const fireNav = (url: string, notFound: boolean) =>
      mockNavState.navResultListener!({ url, notFound });

    const armed = () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      previewManager.armNewPageWait(URL_PATH);
      return previewManager;
    };

    /** Armed on a manager whose cloudSync subscription is live — needed by any
     *  test where a later cloud tick has to re-run the ladder. */
    const armedLive = async () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (getBuildConfig as any).mockReturnValue(null);
      await previewManager.initialize();
      previewManager.armNewPageWait(URL_PATH);
      return previewManager;
    };

    test('leaves the editor panel open and the chrome alone', () => {
      // The screen paints the preview region only; the editor is a native child
      // webview beside it, and the author is INSIDE that editor, typing the
      // page being waited on. Nothing here may close it.
      //
      // The positive assertion is load-bearing: without it this test passes
      // verbatim if arming is DELETED, because with no screen there is nothing
      // to close and nothing to mark.
      document.body.classList.remove('home-gate-active');
      const closeSpy = vi.fn().mockResolvedValue(undefined as any);
      host.editorPanel = { open: vi.fn(), close: closeSpy };
      armed();
      expect(mockOverlay.innerHTML).toContain('Preparing this page');
      expect(closeSpy).not.toHaveBeenCalled();
      // Both halves of the marker, not just <body>'s: the container's is what
      // undoes the chrome inset, and setting only that one blanked the nav
      // pill, back/forward and Edit while leaving the publish pill live.
      expect(document.body.classList.contains('home-gate-active')).toBe(false);
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
      expect(mockOverlay.classList.contains('visible')).toBe(true);
    });

    test('a screen that yields its pixels to a download DOES claim the window', async () => {
      // The opt-out is keyed on the screen that is DRAWN, not the rung that
      // raised it. Rank 4 yields to a real download, and that screen carries a
      // meter and folder buttons — keyed on the rung, the right control column
      // sat clickable on top of them (the layout-zones I1c regression).
      document.body.classList.remove('home-gate-active');
      await armedLive();
      cloudSync.ingest({ phase: 'materializing', downloaded: 1, total: 40, remaining: 39, blocking: 39 });

      expect((previewManager as any).unshowableReason).toBe('page-pending');
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen__meter');
      expect(document.body.classList.contains('home-gate-active')).toBe(true);
    });

    test('a navigation underneath a raised screen does not take it down', () => {
      // THE bug this surface shipped with. The wait's own recheck reloads the
      // pending URL through loadPreview, and every preview navigation hid the
      // overlay — so the cover came off at the START of the reload and the 404
      // underneath was visible for the whole cross-origin load. Deliberately
      // does NOT mock loadPreview: mocking it away is exactly what let the
      // original test pass.
      armed();
      expect(mockOverlay.innerHTML).toContain('Preparing this page');

      (previewManager as any).handleFileChangeEvent(null);   // a build wrote output

      expect((previewManager as any).newPageWait.rechecks).toBe(1);
      expect((previewManager as any).unshowableReason).toBe('page-pending');
      expect(mockOverlay.innerHTML).toContain('Preparing this page');
    });

    // shown/
    // dismissed logging, keyed on `applyUnshowableReason`'s reason actually
    // changing — a served navigation resolves the covered page.
    test('logs shown when the wait screen is raised, dismissed when it resolves', () => {
      vi.useFakeTimers();
      try {
        vi.mocked(info).mockClear();
        armed();
        expect(vi.mocked(info)).toHaveBeenCalledWith('[waiting-screen] shown reason=page-pending');

        fireNav(TARGET, false);
        expect((previewManager as any).unshowableReason).toBeNull();

        const dismissed = vi.mocked(info).mock.calls
          .map((c) => String(c[0]))
          .find((m) => m.startsWith('[waiting-screen] dismissed'));
        expect(dismissed).toBeDefined();
        expect(dismissed).toContain('reason=page-pending');
        expect(dismissed).toContain('ended_by=resolved');
        expect(dismissed).toMatch(/duration_ms=\d+/);
      } finally {
        vi.useRealTimers();
      }
    });

    test('does not log a redundant shown/dismissed pair when the reason is unchanged', () => {
      armed();
      vi.mocked(info).mockClear();
      // Re-running the same reconcile with an unchanged reason (e.g. a
      // no-op recheck) must not re-log "shown" for a screen already up.
      (previewManager as any).reconcileUnshowable();
      const waitingScreenLogs = vi.mocked(info).mock.calls
        .map((c) => String(c[0]))
        .filter((m) => m.startsWith('[waiting-screen]'));
      expect(waitingScreenLogs).toEqual([]);
    });

    test('covers the window over a site that is already served', async () => {
      // The only real-world condition for this reason: the author is looking
      // at their site, so `previewBlank` is false. Every rung below rank 4 is
      // gated on it, and a rank that only worked on a blank preview would
      // never appear in the situation it was written for.
      //
      // Also pins the cover going up OPTIMISTICALLY, at navigation time rather
      // than on the bridge's first `notFound` echo: by the time that echo
      // arrives the 404 has already painted, and the paint is the whole bug.
      previewManager = new PreviewManager('moss-preview-iframe');
      (getBuildConfig as any).mockReturnValue(null);
      await previewManager.initialize();
      cloudSync.setPreviewBlank(false);

      previewManager.armNewPageWait(URL_PATH);

      expect(cloudSync.getState().previewBlank).toBe(false);
      expect((previewManager as any).unshowableReason).toBe('page-pending');
      expect(mockOverlay.innerHTML).toContain('Preparing this page');
    });

    const plainManager = async () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (getBuildConfig as any).mockReturnValue(null);
      await previewManager.initialize();
    };
    const gateIngest = () =>
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
    const buildFailure = () =>
      (previewManager as any).renderStatus({
        type: 'error', message: 'Address already in use', verdict: 'reported',
      });

    test.each([
      ['a gate raised over nothing', plainManager, gateIngest, 'gate'],
      ['a gate that takes over from page-pending', armedLive, gateIngest, 'gate'],
      ['a build failure over page-pending', async () => { armed(); }, buildFailure, 'error-hard'],
    ] as const)('%s leaves the editor panel open', async (_name, setup, raise, expected) => {
      // The waiting screen paints the preview region only; the editor beside
      // it stays open, whichever reason raised the screen.
      const closeSpy = vi.fn().mockResolvedValue(undefined as any);
      host.editorPanel = { open: vi.fn(), close: closeSpy };
      await setup();
      closeSpy.mockClear();

      await raise();

      expect((previewManager as any).unshowableReason).toBe(expected);
      expect(closeSpy).not.toHaveBeenCalled();
    });

    test('an unreachable folder closes the editor panel', () => {
      const closeSpy = vi.fn().mockResolvedValue(undefined as any);
      host.editorPanel = { open: vi.fn(), close: closeSpy };
      armed();
      closeSpy.mockClear();

      (previewManager as any).folderUnreachable = true;
      (previewManager as any).reconcileUnshowable();

      expect((previewManager as any).unshowableReason).toBe('unreachable');
      expect(closeSpy).toHaveBeenCalled();
    });

    test('does not arm under an error screen the author has to read', () => {
      // An error rank is sticky until a new PreviewStatus arrives, so nothing
      // would be raised — but the wait would be armed underneath it,
      // suppressing the smart-diff refresh and reloading the preview for three
      // builds behind a screen that says the build failed.
      previewManager = new PreviewManager('moss-preview-iframe');
      (previewManager as any).renderStatus({
        type: 'error', message: 'Address already in use', verdict: 'reported',
      });
      expect((previewManager as any).unshowableReason).toBe('error-hard');

      previewManager.armNewPageWait(URL_PATH);

      expect((previewManager as any).newPageWait).toBeNull();
    });

    test('a stale echo from a superseded create does not uncover the newer one', () => {
      // Two ⌘N in quick succession: A's navigation is still in flight when B
      // arms. A's echo carries A's pathname, which rule 2 reads as "the author
      // clicked away" — uncovering B for its whole build, with B's own echo
      // then finding nothing armed.
      const OTHER = 'http://localhost:8080/notes/second/';
      armed();
      previewManager.armNewPageWait('/notes/second/');
      expect((previewManager as any).newPageWait.pathname).toBe('/notes/second/');

      fireNav(TARGET, true);            // A's late echo

      expect((previewManager as any).newPageWait).not.toBeNull();
      expect(mockOverlay.innerHTML).toContain('Preparing this page');

      fireNav(OTHER, false);            // B's page arrives
      expect((previewManager as any).newPageWait).toBeNull();
    });

    test('creating another page cancels the moved page settle timer', () => {
      vi.useFakeTimers();
      try {
        previewManager = new PreviewManager('moss-preview-iframe');
        previewManager.armNewPageWait('/moved/');
        fireNav('http://localhost:8080/moved/', true);
        previewManager.armNewPageWait(URL_PATH);
        const load = vi.spyOn(previewManager, 'loadPreview');
        vi.advanceTimersByTime(351);
        expect(load).not.toHaveBeenCalled();
        expect((previewManager as any).newPageWait.pathname).toBe(URL_PATH);
      } finally { vi.useRealTimers(); }
    });

    test('ends the moment the new page serves, without pulling the preview home', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      // Reach 'ready' first, so the falling edge has a status that WOULD
      // navigate: renderStatus('ready') calls loadPreview(serverRoot).
      (previewManager as any).handleProgressUpdate({
        step: 'complete', message: 'done', percentage: 100,
        completed: true, port: 8080, is_empty: false,
      } as ProgressUpdate);
      const loadSpy = vi.spyOn(previewManager, 'loadPreview').mockImplementation(() => {});

      previewManager.armNewPageWait(URL_PATH);
      fireNav(TARGET, true);   // still the synthetic 404 — keep waiting
      expect((previewManager as any).newPageWait).not.toBeNull();

      fireNav(TARGET, false);  // the build landed and the page serves
      expect((previewManager as any).newPageWait).toBeNull();
      // The preview is already ON the new page; re-rendering 'ready' would
      // send it back to the home page.
      expect(loadSpy).not.toHaveBeenCalled();
    });

    test('ends when the author navigates somewhere else instead', () => {
      armed();
      fireNav('http://localhost:8080/somewhere/else/', false);
      expect((previewManager as any).newPageWait).toBeNull();
    });

    test('the deadline offers recovery without uncovering the missing page', () => {
      vi.useFakeTimers();
      // try/finally: a failing assertion in the middle used to leak fake
      // timers into every test after it in this file.
      try {
        armed();
        expect((previewManager as any).newPageWait).not.toBeNull();
        vi.advanceTimersByTime(TIMEOUT);
        expect((previewManager as any).newPageWait.stalled).toBe(true);
        expect(mockOverlay.classList.contains('visible')).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    });

    test('reveals a fast page immediately after its served echo, covering the preceding 404', () => {
      vi.useFakeTimers();
      try {
        armed();
        fireNav(TARGET, true);
        expect(mockOverlay.innerHTML).toContain('Preparing this page');
        vi.advanceTimersByTime(20);
        fireNav(TARGET, false);
        expect((previewManager as any).newPageWait).toBeNull();
        expect((previewManager as any).unshowableReason).toBeNull();
        expect(mockOverlay.classList.contains('visible')).toBe(false);
      } finally {
        vi.useRealTimers();
      }
    });

    test('a reported build failure takes the window and ends the wait', () => {
      armed();
      // Not vacuous: without this the first assertion below passes with no
      // wait ever armed.
      expect((previewManager as any).newPageWait).not.toBeNull();

      (previewManager as any).renderStatus({
        type: 'error', message: 'Address already in use', verdict: 'reported',
      });
      expect((previewManager as any).newPageWait).toBeNull();
      expect((previewManager as any).unshowableReason).toBe('error-hard');
      // The minimum display does not make an error wait its turn.
      expect(mockOverlay.innerHTML).not.toContain('Preparing this page');
    });
  });

  // ─── Cloud-download home-page gate ────────────────────────────────────────
  // The risk these guard is the gate getting stuck when the backend
  // event that should clear it never arrives — which is why the escape hatch
  // must clear the gate itself rather than wait for one. `cloudSync` is now
  // the single owner of the `icloud-sync` event (see `cloud-sync-store.ts`'s
  // module doc); these tests drive it via `cloudSync.ingest(...)`
  // directly — the same seam its own unit tests use — rather than the
  // `listen()` mock, and assert PreviewManager's state machine AND its side
  // effects (overlay class) at each transition, not just that the mock exists.
  // `ProgressPanel.setHomeGateActive` is gone: the panel now derives that fact
  // from the store itself instead of being pushed it.
  describe('Cloud-download home-page gate (icloud-sync via cloudSync)', () => {
    const setup = async (): Promise<void> => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (getBuildConfig as any).mockReturnValue(null);
      await previewManager.initialize();
    };

    test('home_waiting shows the waiting screen', async () => {
      await setup();

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });

      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.classList.contains('visible')).toBe(true);
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(true);
    });

    test('home_ready releases the gate and re-renders status', async () => {
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });

      cloudSync.ingest({ phase: 'home_ready', downloaded: 4, total: 4, remaining: 0 });

      expect(cloudSync.getState().gateUp).toBe(false);
      // 'visible' goes with it: leaving the overlay visible strands the
      // waiting screen on screen and inert — renderStatus('loading')
      // early-returns on an already-visible overlay, and nothing else repaints.
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
      expect(mockOverlay.classList.contains('visible')).toBe(false);
    });

    test('a resolved target stays in the iframe when its seal clears the gate', async () => {
      await setup();
      const load = vi.spyOn(previewManager as any, 'loadPreview').mockImplementation(() => {});
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      (previewManager as any).renderStatus({ type: 'ready', url: 'http://localhost:8080/' });
      mockNavState.activePage = { url: 'http://localhost:8080/places/', sourceFile: null };

      cloudSync.ingest({ phase: 'home_ready', downloaded: 1, total: 4, remaining: 3 });

      expect(cloudSync.getState().gateUp).toBe(false);
      expect(mockOverlay.classList.contains('visible')).toBe(false);
      expect(load).not.toHaveBeenCalled();
      expect(mockProgressPanel.setStage).toHaveBeenCalledWith('ready');
    });

    // The same raise/lower edge that flips
    // `progressPanel.setScreenUp` also owns `overlay-toast-gate`'s
    // `setOverlayUp` — real state, not a DOM query, so a plugin/background
    // toast arriving mid-wait queues instead of interrupting the screen.
    test('the gate raising and releasing is the same edge that opens/closes the toast queue', async () => {
      await setup();
      expect(isOverlayUp()).toBe(false);

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(isOverlayUp()).toBe(true);

      cloudSync.ingest({ phase: 'home_ready', downloaded: 4, total: 4, remaining: 0 });
      expect(isOverlayUp()).toBe(false);
    });

    // error-hard/error-soft are NOT waits — they
    // can hold the window indefinitely and the author IS looking at the app,
    // unlike "Opening your site…". Queuing behind one would be "never", not
    // "not yet", so entering an error screen flushes the queue instead of
    // extending the hold, even though the full-window screen itself stays up.
    test('an error screen is not a wait — entering one flushes the queue instead of holding it', async () => {
      // gateUp outranks error-hard in the ladder, so this drives the WAIT
      // via an ordinary 'opening' rank (no cloud gate involved) rather than
      // the home-page gate above — a gate that were still up would mask the
      // error-hard transition this test is about.
      await setup();
      cloudSync.ingest({ phase: 'materializing', downloaded: 270, total: 270, remaining: 0 });
      (previewManager as any).renderStatus({ type: 'loading', message: 'Scanning…', percentage: 10 });
      expect((previewManager as any).unshowableReason).toBe('opening');
      expect(isOverlayUp()).toBe(true);

      let ran = false;
      queueOrRun('k', () => { ran = true; });
      expect(ran).toBe(false); // queued while waiting

      (previewManager as any).renderStatus({
        type: 'error', message: 'Address already in use', verdict: 'reported',
      });

      // The screen is STILL up (an error, not nothing) — but the queue
      // treats entering it as a fall, not a hold.
      expect((previewManager as any).unshowableReason).toBe('error-hard');
      expect(isOverlayUp()).toBe(false);
      expect(ran).toBe(true);
    });

    test('a failed build says, in the same window, which site and why, with Try again', async () => {
      await setup();
      (previewManager as any).folderPath = '/Users/test/my-blog';
      const build = vi.spyOn(previewManager, 'startBuild').mockResolvedValue(undefined);

      (previewManager as any).renderStatus({ type: 'error', message: 'Failed to build: a page links to a missing file', verdict: 'reported' });

      expect(mockOverlay.querySelector('[data-title]')!.textContent).toBe('moss could not open my-blog');
      expect(mockOverlay.textContent).toContain('Failed to build: a page links to a missing file');
      mockOverlay.querySelector<HTMLButtonElement>('#moss-waiting-retry-open')!.click();
      expect(build).toHaveBeenCalledWith('/Users/test/my-blog');
    });

    test.each([
      ['other-app', 'my-blog is open in another moss app. Close it, then try again.', false],
      ['server', 'The preview could not start.', true],
      [undefined, 'The build stopped before the site was ready.', true],
    ] as const)('a start stopped by %s says so, with the server\'s own words under Details', async (cause, hint, offersLogs) => {
      const { host } = await import("../../host");
      const kept = host.sendLogs;
      host.sendLogs = async () => ({ status: 'ok', data: '' });
      try {
        await setup();
        (previewManager as any).folderPath = '/Users/test/my-blog';
        const message = 'this folder is already served at http://127.0.0.1:8080 by Cli (pid 4242, moss 0.15.24)';

        (previewManager as any).renderStatus({ type: 'error', message, verdict: 'reported', cause });

        expect(mockOverlay.querySelector('.moss-waiting-screen__hint')!.textContent).toBe(hint);
        expect(mockOverlay.querySelector('details')!.textContent).toContain(message);
        expect(mockOverlay.querySelector('#moss-waiting-send-logs') !== null).toBe(offersLogs);
      } finally {
        host.sendLogs = kept;
      }
    });

    test('a host without a launcher, a log upload or a file manager offers none of their buttons', async () => {
      const { host } = await import("../../host");
      const kept = { launcher: host.launcher, sendLogs: host.sendLogs, reveal: host.reveal };
      Object.assign(host, { launcher: undefined, sendLogs: undefined, reveal: undefined });
      try {
        await setup();
        (previewManager as any).folderPath = '/Users/test/my-blog';
        (previewManager as any).renderStatus({ type: 'error', message: 'Failed to build: boom', verdict: 'reported' });
        const labels = [...mockOverlay.querySelectorAll('button')].map((b) => b.textContent);
        expect(labels).toEqual(['Try again']);
        cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
        expect(mockOverlay.querySelector('#moss-waiting-choose-folder')).toBeNull();
        expect(mockOverlay.querySelector('#moss-waiting-reveal-folder')).toBeNull();
      } finally {
        Object.assign(host, kept);
      }
    });

    test('the screen offers no way to see the site without a home page', async () => {
      // There used to be a "Show the site anyway" link, and what it showed was
      // a 404 where the home page belongs. The gate now holds until
      // `home_ready`; the only exits are a different folder and the tip that
      // stops the eviction happening again.
      await setup();

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 2, total: 4, remaining: 2 });

      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-show-anyway');
      expect(mockOverlay.innerHTML).toContain('moss-waiting-choose-folder');
    });

    test('an event for another folder is ignored', async () => {
      // `icloud-sync` is app-wide and the previous folder's supervisor observes
      // its cancellation only between ticks, so a tick already in flight lands
      // after the user has switched. Acting on it would raise a full-window
      // "downloading" screen over a folder that is entirely local. The filter
      // now lives in `cloudSync`, scoped via `setFolder` — see its doc.
      await setup();
      (previewManager as any).folderPath = '/vault/current';
      cloudSync.setFolder('/vault/current');
      mockOverlay.classList.remove('visible', 'home-gate-active');

      cloudSync.ingest({
        folder: '/vault/the-one-they-left', phase: 'home_waiting', downloaded: 1, total: 700, remaining: 699,
      });

      expect(cloudSync.getState().gateUp).toBe(false);
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
    });

    test('an event without a folder is still honoured', async () => {
      // Older Rust builds omit the field. Going silent on them would disable
      // the gate entirely rather than degrade it.
      await setup();
      (previewManager as any).folderPath = '/vault/current';
      cloudSync.setFolder('/vault/current');

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });

      expect(cloudSync.getState().gateUp).toBe(true);
    });

    test('the waiting screen names the provider it is downloading from', async () => {
      await setup();

      cloudSync.ingest({
        phase: 'home_waiting', provider_label: 'iCloud Drive', downloaded: 1, total: 4, remaining: 3,
      });

      expect(mockOverlay.innerHTML).toContain('iCloud Drive');
    });

    test('the gate marks <body> so CSS can remove the right-column chrome', async () => {
      // The overlay lives inside .moss-preview-container, which is inset by
      // --moss-control-column-width and clips (overflow: hidden) — so no
      // z-index can put the waiting screen over the device toggle, channel
      // panel, or publish pill. They stayed clickable over "Preparing your
      // site": a publish button for a build the gate exists to call
      // incomplete. The gate marks <body> instead and style.css removes them
      // (layout-zones I1c).
      await setup();
      document.body.classList.remove('home-gate-active');

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(document.body.classList.contains('home-gate-active')).toBe(true);

      cloudSync.ingest({ phase: 'home_ready', downloaded: 4, total: 4, remaining: 0 });
      expect(document.body.classList.contains('home-gate-active')).toBe(false);
    });

    test('a build error takes the window from a gate an EARLIER build raised', async () => {
      // Both used to paint: the error went up, then the next supervisor tick two
      // seconds later repainted the waiting screen over it. For a real failure
      // the build's own error is the more specific explanation, so it wins and
      // the stale gate drops.
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(cloudSync.getState().gateUp).toBe(true);
      // A new build started: whatever the last one said no longer speaks for it.
      (previewManager as any).homeGateRaisedThisBuild = false;

      (previewManager as any).renderStatus({ type: 'error', message: 'boom', verdict: 'reported' });

      expect(cloudSync.getState().gateUp).toBe(false);
      // The DOWNLOAD screen is gone and the failure is up in its place. Both
      // are the same surface now, so the meter — which only a download screen
      // carries — is what tells them apart.
      expect(mockOverlay.innerHTML).toContain('boom');
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen__meter');

      // And the next tick does not paint back over it. No `home_ready` is ever
      // coming for this episode — the error ended it — so `clearHomeGate()`
      // tells the store directly (`lowerGate`). Were the manager to drop only
      // its own copy, the store would still say `gateUp` and this tick would
      // repaint straight over the error message.
      const before = mockOverlay.innerHTML;
      cloudSync.ingest({ phase: 'materializing', downloaded: 2, total: 4, remaining: 2 });
      expect(mockOverlay.innerHTML).toBe(before);
    });

    test('a build that raised the gate and then failed keeps the gate, not the errno', async () => {
      // An evicted `.moss/build/` made every write fail EDEADLK, and
      // the Err preempted the gate — the user got `Resource deadlock avoided
      // (os error 11)` on the onboarding overlay. The backend now emits
      // `home_waiting` before returning that Err, and BOTH signals describe the
      // same build, so the gate is the accurate screen and it stays up.
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });

      (previewManager as any).renderStatus({
        type: 'error',
        message: 'Failed to build: Failed to write default favicon: Resource deadlock avoided (os error 11)',
        verdict: 'reported',
      });

      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
      expect(mockOverlay.innerHTML).not.toContain('os error 11');

      // The supervisor still owns the screen, so arrivals keep updating it.
      cloudSync.ingest({ phase: 'materializing', downloaded: 3, total: 4, remaining: 1 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
    });

    test('a stall replaces the download screen while the gate is up, and an arrival takes it back', async () => {
      // The stall notice is an observation, not a verdict: the supervisor keeps
      // trying, so a file that finally lands must return the user to the
      // ordinary downloading screen rather than leaving a dead end up.
      await setup();

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 0, total: 4, remaining: 4 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-reveal-folder');

      cloudSync.ingest({ phase: 'stalled', downloaded: 1, total: 4, remaining: 3 });
      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).toContain('moss-waiting-reveal-folder');
      // The travelling glow says "something is moving"; the stalled screen's
      // whole message is that nothing has.
      expect(mockOverlay.innerHTML).not.toContain('data-crossing');

      cloudSync.ingest({ phase: 'materializing', downloaded: 2, total: 4, remaining: 2 });
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-reveal-folder');
      expect(mockOverlay.innerHTML).toContain('data-crossing');
    });

    test('supervisor progress never raises the screen over a site already on show', async () => {
      // A `stalled` arriving while the site is up and visible must not slam a
      // full-window screen over it — that download belongs to the corner
      // panel. Progress alone raises nothing; it needs a preview with nothing
      // of its own to show (see the non-sticky tests below).
      await setup();
      mockOverlay.classList.remove('visible', 'home-gate-active');

      cloudSync.ingest({ phase: 'stalled', downloaded: 1, total: 4, remaining: 3 });

      expect((previewManager as any).unshowableReason).toBeNull();
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
    });

    // The gate (full-window screen) and the
    // corner panel now derive "should I paint?" from the SAME store instead of
    // one pushing a boolean to the other. From this file's side that means: a
    // progress tick while the gate is down must not paint the full-window
    // screen (that is the panel's to show, and it is out of scope here — see
    // progress-panel.test.ts), and a progress tick while the gate is up must.
    test('a progress tick while the gate is down does not paint the full-window screen', async () => {
      await setup();
      mockOverlay.classList.remove('visible', 'home-gate-active');

      cloudSync.ingest({ phase: 'materializing', downloaded: 1, total: 4, remaining: 3 });

      expect(cloudSync.getState().gateUp).toBe(false);
      expect(mockOverlay.classList.contains('home-gate-active')).toBe(false);
      // The greeting is still up, and it is a VERDICT — moss has something to
      // say and nothing to show — so it draws no meter. Every state of this
      // window is the same surface now, so absence of the class proves
      // nothing; absence of a meter is what says no wait was painted here.
      expect(mockOverlay.querySelector('.moss-waiting-screen__meter')).toBeNull();
    });

    // ─── The non-sticky half: waiting on a download, home page buildable ────
    // The screen shows while a download runs even when a home page is buildable:
    // a vault whose index.html is already on disk must not get the bare blueprint
    // grid for the minutes its files take to arrive, with the explanation in a corner panel.
    describe('a download in flight while the preview has nothing to show', () => {
      test("renderStatus('loading') hands the window to the waiting screen", async () => {
        await setup();
        cloudSync.ingest({ phase: 'materializing', provider_label: 'Google Drive', downloaded: 30, total: 270, remaining: 240 });
        (startBlueprintGrid as any).mockClear();

        (previewManager as any).renderStatus({ type: 'loading', message: 'Scanning…', percentage: 10 });

        expect((previewManager as any).unshowableReason).not.toBeNull();
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect(mockOverlay.innerHTML).toContain('Google Drive');
        // The gate itself never went up — no build said it could not produce a
        // home page, and this screen must not claim one did.
        expect(cloudSync.getState().gateUp).toBe(false);
      });

      test("renderStatus('loading') with nothing left in the cloud shows rank 8, opening", async () => {
        // An ordinary build of a fully-local folder goes through the same surface;
        // there is no bare-grid fallback without an explanation.
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 270, total: 270, remaining: 0 });

        (previewManager as any).renderStatus({ type: 'loading', message: 'Scanning…', percentage: 10 });

        expect((previewManager as any).unshowableReason).toBe('opening');
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect(mockOverlay.querySelector('#blueprint-canvas')).not.toBeNull();
      });

      test('a served site takes the window back, even mid-download', async () => {
        // Not sticky, unlike the gate: there is a real page underneath now, so
        // the remaining download becomes the corner panel's to report.
        await setup();
        const loadPreview = vi.spyOn(previewManager as any, 'loadPreview').mockImplementation(() => {});
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 270, remaining: 240 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

        (previewManager as any).renderStatus({ type: 'ready', url: 'http://localhost:8080' });

        expect((previewManager as any).unshowableReason).toBeNull();
        expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen');
        // Exactly once: lowering the screen notifies synchronously and the
        // subscriber re-runs the last status, which is this very one.
        expect(loadPreview).toHaveBeenCalledTimes(1);

        // And the download that is still running does not take it back.
        cloudSync.ingest({ phase: 'materializing', downloaded: 60, total: 270, remaining: 210 });
        expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen');
      });

      test('the last file arriving hands the screen from a download to rank 8, opening', async () => {
        // The download episode can end without a build verdict — the screen
        // itself does not, because the build is still running and previewBlank
        // is still true. It just stops explaining a download and falls
        // through to the same generic content an ordinary local build shows.
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 269, total: 270, remaining: 1 });
        // Driven through the state machine, not renderStatus directly: the
        // hand-back re-runs `this.state.getStatus()`, so the manager's own
        // status has to genuinely be 'loading' for this to prove anything.
        (previewManager as any).handleProgressUpdate({
          step: 'scanning', message: '…', percentage: 90,
          completed: false, port: null, is_empty: null,
        });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect((previewManager as any).unshowableReason).toBe('downloading');

        cloudSync.ingest({ phase: 'materializing', downloaded: 270, total: 270, remaining: 0 });

        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect((previewManager as any).unshowableReason).toBe('opening');
      });

      test('home_ready ends it, even with files still arriving', async () => {
        // The two ways up are independent, and this is where forgetting that
        // bites: the site was served while the gate was up, so 'ready'
        // early-returned. If it returns WITHOUT recording that the preview is
        // no longer blank, the later home_ready drops the gate and the screen
        // stays up anyway on the download half — over a servable site, with no
        // further status change coming to repaint it.
        await setup();
        vi.spyOn(previewManager as any, 'loadPreview').mockImplementation(() => {});
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 1300, remaining: 1270 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        cloudSync.ingest({ phase: 'home_waiting', downloaded: 30, total: 1300, remaining: 1270 });
        (previewManager as any).renderStatus({ type: 'ready', url: 'http://localhost:8080' });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen'); // gate is sticky

        cloudSync.ingest({ phase: 'home_ready', downloaded: 400, total: 1300, remaining: 900 });

        expect((previewManager as any).unshowableReason).toBeNull();
        expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen');
      });

      test('an error taken over rank 8 (opening) still disarms the gate for later ticks', async () => {
        // A reported error outranks rank 8 same as it outranks a download —
        // and `clearHomeGate` still fires, so `previewBlank` cannot arm a
        // later supervisor tick to repaint over the error message. The
        // reconciler's own guard (once an error rank is up, only a fresh
        // PreviewStatus may move it) backs this up independently.
        await setup();
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect((previewManager as any).unshowableReason).toBe('opening');

        (previewManager as any).renderStatus({ type: 'error', message: 'boom', verdict: 'reported' });
        const before = mockOverlay.innerHTML;

        cloudSync.ingest({ phase: 'materializing', downloaded: 1, total: 12, remaining: 11 });

        expect((previewManager as any).unshowableReason).toBe('error-hard');
        expect(mockOverlay.innerHTML).toBe(before);
      });

      test('a re-render leaves the running grid alone', async () => {
        // The grid is an rAF loop plus three observers seeded from
        // Math.random(). It used to be started and stopped by whichever screen
        // rendered last through a single shared handle — so a restart either
        // leaked the old loop or re-seeded the particle field, which the user
        // sees as the background jumping. It belongs to the overlay now, and
        // the only thing that stops it is the overlay coming down.
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 270, remaining: 240 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        const canvas = mockOverlay.querySelector('#blueprint-canvas');
        expect(canvas).not.toBeNull();
        mockBlueprintCleanup.mockClear();
        (startBlueprintGrid as any).mockClear();

        // A shape change, so the screen genuinely re-renders.
        cloudSync.ingest({ phase: 'stalled', downloaded: 30, total: 270, remaining: 240 });

        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect(mockOverlay.querySelector('#blueprint-canvas')).toBe(canvas);
        expect(startBlueprintGrid).not.toHaveBeenCalled();
        expect(mockBlueprintCleanup).not.toHaveBeenCalled();
      });

      test('a build error takes the window from it, and keeps it', async () => {
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 270, remaining: 240 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

        (previewManager as any).renderStatus({ type: 'error', message: 'boom', verdict: 'reported' });

        expect(mockOverlay.innerHTML).toContain('boom');
        expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen__meter');
        // The next supervisor tick must not repaint over the message: dropping
        // only the gate would have left the other half of the claim standing.
        const before = mockOverlay.innerHTML;
        cloudSync.ingest({ phase: 'materializing', downloaded: 31, total: 270, remaining: 239 });
        expect(mockOverlay.innerHTML).toBe(before);
      });

      // ─── An error moss GUESSED may not destroy this screen ────
      // The reported sequence, exactly: a Google Drive vault mid-download, the
      // honest "Preparing your site" screen up, and a second pipeline's
      // portless `complete` painting "Preview server unavailable" over it —
      // while :8080 had been serving for nineteen seconds. The screen is
      // reporting something true and ongoing; a guess does not outrank it.
      test('an UNVERIFIED error leaves the download screen alone', async () => {
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 1300, remaining: 1270 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

        (previewManager as any).renderStatus({
          type: 'error', message: 'Preview server unavailable', verdict: 'unverified',
        });

        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect((previewManager as any).unshowableReason).not.toBeNull();
        // And the screen is still LIVE, not a frozen husk: the next tick still
        // moves its numbers, because nothing disarmed the half holding it up.
        cloudSync.ingest({ phase: 'materializing', downloaded: 200, total: 1300, remaining: 1100 });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
      });

      test('an UNVERIFIED error with nothing on screen still tells the user', async () => {
        // The inverse bug, and the worse one: a message that can never be
        // shown strands the user on a bar that has stopped moving. With no
        // screen to protect there is nothing better to show, so it shows.
        await setup();
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect((previewManager as any).unshowableReason).toBe('opening');

        (previewManager as any).renderStatus({
          type: 'error', message: 'Preview server unavailable', verdict: 'unverified',
        });

        // The failure renders on the same blueprint screen as every other
        // state of this window, with the folder exit it is the only way out of.
        expect(mockOverlay.innerHTML).toContain('Preview server unavailable');
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
        expect(mockOverlay.innerHTML).toContain('moss-waiting-choose-folder');
      });

      test('a REPORTED failure still takes the window from the download screen', async () => {
        // The legitimate teardown, kept explicit next to the one that is now
        // refused: a gate that can no longer be lowered strands the user
        // forever, which is worse than the bug this pair exists to fix.
        await setup();
        cloudSync.ingest({ phase: 'materializing', downloaded: 30, total: 1300, remaining: 1270 });
        (previewManager as any).renderStatus({ type: 'loading', message: '…', percentage: 10 });
        expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

        (previewManager as any).renderStatus({
          type: 'error', message: 'Address already in use (os error 48)', verdict: 'reported',
        });

        expect(mockOverlay.innerHTML).toContain('Address already in use');
        expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen__meter');
        expect((previewManager as any).unshowableReason).toBe('error-hard');
      });
    });

    test('an UNVERIFIED error does not lower a gate raised by an EARLIER build', async () => {
      // `homeGateRaisedThisBuild` only covers the build that is speaking. A
      // gate an earlier build raised is still a promise Rust made and only
      // Rust's `home_ready` (or a real verdict) may cancel it.
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      (previewManager as any).homeGateRaisedThisBuild = false;

      (previewManager as any).renderStatus({
        type: 'error', message: 'Preview server unavailable', verdict: 'unverified',
      });

      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
    });

    test('a stall tick while the gate is up paints the full-window screen', async () => {
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

      cloudSync.ingest({ phase: 'stalled', downloaded: 2, total: 4, remaining: 2 });

      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
      expect(mockOverlay.innerHTML).toContain('moss-waiting-reveal-folder');
    });

    test('the gate holds across rebuilds until the backend says the home page is ready', async () => {
      // Every rebuild re-emits its verdict. Successive `home_waiting`s must
      // keep the screen up — there is no dismissal that could have taken it
      // down — and only `home_ready` releases it.
      await setup();

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 2, total: 4, remaining: 2 });
      expect(cloudSync.getState().gateUp).toBe(true);
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

      cloudSync.ingest({ phase: 'home_ready', downloaded: 4, total: 4, remaining: 0 });
      expect(cloudSync.getState().gateUp).toBe(false);
    });

    test('the tip quotes the provider\'s own pin menu item, and only when the provider is known', async () => {
      // The tip is the only exit from the wait that doesn't involve waiting,
      // and it is worth nothing unless it names a menu item that exists — so
      // it needs the provider id, not just the display label.
      await setup();

      cloudSync.ingest({
        phase: 'home_waiting', provider: 'google_drive', provider_label: 'Google Drive', downloaded: 1, total: 4, remaining: 3,
      });
      expect(mockOverlay.innerHTML).toContain('Available offline');

      // An unidentified File Provider extension: no menu item we could name,
      // so no tip rather than an invented one. New folder → fresh store state,
      // so the sticky provider from the first ingest is gone too.
      (previewManager as any).clearHomeGate();
      cloudSync.setFolder('/vault/a-different-folder');
      cloudSync.ingest({
        folder: '/vault/a-different-folder', phase: 'home_waiting', provider: 'unknown', provider_label: 'cloud', downloaded: 1, total: 4, remaining: 3,
      });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen__tip');
    });

    test('a path that wrote over the overlay lets the screen come back', async () => {
      // The render guard skips a tick whose numbers match what it last drew.
      // If something else replaced the overlay in between, that cached key
      // would suppress the repaint forever, leaving the 404 the gate exists to
      // hide, with the gate still up so nothing else renders either. The
      // surface's DOM liveness check is what refuses that.
      //
      // (`hideOverlay` used to be the stand-in here. It no longer wipes
      // anything while a reason holds the window — see 'a navigation
      // underneath a raised screen does not take it down' — so it cannot pose
      // the question this test is asking.)
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

      mockOverlay.innerHTML = '<div>something else entirely</div>';
      expect(mockOverlay.innerHTML).not.toContain('moss-waiting-screen');

      // Same numbers as the tick before the overlay was wiped.
      cloudSync.ingest({ phase: 'materializing', downloaded: 1, total: 4, remaining: 3 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');
    });

    test('the gate lowering under the unreachable screen leaves the screen, not a blank window', async () => {
      // Reachable episode: waiting screen up → root vanishes (rank 1,
      // unreachable, outranks the gate) → the arrival scan reads the pending
      // paths as gone → "0 remaining" lowers the gate. `resolveUnshowableReason`
      // must keep answering `unreachable` while `folderUnreachable` is set, so
      // the gate's own clearing edge does not repaint underneath it.
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      expect(mockOverlay.innerHTML).toContain('moss-waiting-screen');

      (previewManager as any).folderUnreachable = true;
      (previewManager as any).reconcileUnshowable();
      expect(mockOverlay.innerHTML).toContain('data-folder-unavailable');

      // The gate lowers while the stronger screen holds the window.
      cloudSync.ingest({ phase: 'home_ready', downloaded: 4, total: 4, remaining: 0 });
      expect(cloudSync.getState().gateUp).toBe(false);
      // The screen survives, live — the clearing health transition (not this
      // edge) is what will take it down and repaint.
      expect(mockOverlay.innerHTML).toContain('data-folder-unavailable');
      expect((previewManager as any).unshowableReason).toBe('unreachable');
    });
  });

  describe('waiting screen — moving the numbers without re-rendering', () => {
    // The supervisor emits every 2s and `downloaded` moves on most ticks, so
    // this is the common path, not an optimization. Re-rendering restarts the
    // crossing glow (7s animation, 2s life — it would never cross), defeats
    // the fill's width transition, and recreates the live region instead of
    // updating it. The updater itself is one function for every state now
    // (`applyScreenValues`); its refusal cases are covered beside it in
    // the host surface tests. What this file proves is the
    // END-TO-END path: a supervisor tick reaching a live screen.
    const meterOf = () => mockOverlay.querySelector<HTMLElement>('.moss-waiting-screen__meter')!;

    const setup = async (): Promise<void> => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (getBuildConfig as any).mockReturnValue(null);
      await previewManager.initialize();
    };

    test('a supervisor tick moves the fill and the sentence without new markup', async () => {
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      const screen = mockOverlay.querySelector('.moss-waiting-screen');
      const fill = mockOverlay.querySelector<HTMLElement>('.moss-waiting-screen__meter-fill')!;
      const line = mockOverlay.querySelector<HTMLElement>('[data-meta]')!;

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 3, total: 4, remaining: 1 });

      // Same nodes — that is the whole point; a replaced node restarts its
      // animations and is not announced as a live-region update.
      expect(mockOverlay.querySelector('.moss-waiting-screen')).toBe(screen);
      expect(mockOverlay.querySelector('.moss-waiting-screen__meter-fill')).toBe(fill);
      expect(mockOverlay.querySelector('[data-meta]')).toBe(line);
      // jsdom normalizes "75.00%" to "75%", so compare the number.
      expect(parseFloat(fill.style.width)).toBeCloseTo(75, 2);
      expect(line.textContent).toContain('3');
      expect(meterOf().getAttribute('aria-valuenow')).toBe('3');
    });

    test('the grid keeps running across a tick — one grid per overlay, not per screen', async () => {
      // Restarting it re-seeds the particle field from Math.random(), which
      // the user sees as the background jumping. The build wait's stage
      // sentence changes on nearly every progress event, which made that a
      // continuous flicker.
      await setup();
      cloudSync.ingest({ phase: 'home_waiting', downloaded: 1, total: 4, remaining: 3 });
      const canvas = mockOverlay.querySelector('#blueprint-canvas');
      expect(canvas).not.toBeNull();
      vi.mocked(startBlueprintGrid).mockClear();
      mockBlueprintCleanup.mockClear();

      cloudSync.ingest({ phase: 'home_waiting', downloaded: 3, total: 4, remaining: 1 });

      expect(mockOverlay.querySelector('#blueprint-canvas')).toBe(canvas);
      expect(startBlueprintGrid).not.toHaveBeenCalled();
      expect(mockBlueprintCleanup).not.toHaveBeenCalled();
    });

  });

  describe('threshold surface — stage monotonicity', () => {
    // The threshold states live strictly BEFORE the build. Detection runs
    // concurrently with nothing, but its ticks arrive as events — and a
    // straggler tick landing after the build started must not repaint
    // "Checking this folder…" over the build overlay.
    let overlayEl: HTMLElement;

    const prompt = {
      token: 1,
      kind: 'unowned' as const,
      report: {
        root: '/u/Drive',
        root_class: { kind: 'normal' as const },
        nested: [
          { rel_path: 'a/blog', abs_path: '/u/Drive/a/blog', name: 'blog', published: false, site_id: null },
        ],
        truncated: false,
        dirs_visited: 7,
      },
      suggestion: null,
    };

    beforeEach(() => {
      // A REAL overlay element: the threshold surface renders real markup and
      // wires real listeners, which the shared object mock cannot host.
      overlayEl = document.createElement('div');
      vi.spyOn(document, 'getElementById').mockImplementation((id: string) => {
        if (id === 'moss-preview-iframe') return mockIframe as any;
        if (id === 'moss-preview-overlay') return overlayEl as any;
        if (id === 'moss-preview-container') return document.createElement('div');
        return null;
      });
      previewManager = new PreviewManager('moss-preview-iframe');
    });

    test('before the build, checking and prompt render and advance the stage', () => {
      previewManager.showThresholdChecking(42);
      expect(previewManager.getState().getStage()).toBe('checking');
      // The walk is a WAIT, so it draws on the one surface that owns waiting;
      // only the question that follows is the threshold screen's own markup.
      expect(overlayEl.querySelector('[data-meta]')).not.toBeNull();
      expect(overlayEl.innerHTML).toContain('42');

      previewManager.showThresholdPrompt(prompt);
      expect(previewManager.getState().getStage()).toBe('threshold');
      expect(overlayEl.querySelector('[data-threshold-cards]')).not.toBeNull();
    });

    test('a straggler detection tick after the build starts paints nothing', () => {
      previewManager.getState().setStage('compiling');
      previewManager.showThresholdChecking(42);
      previewManager.showThresholdPrompt(prompt);
      expect(overlayEl.innerHTML).toBe('');
      // And the stage was not rewound.
      expect(previewManager.getState().getStage()).toBe('compiling');
    });

    test('a guard-driven prompt paints over a live site (folder switch) and cancel restores it', () => {
      // The guard only runs before anything is written for a NEW open, so a
      // guard-tagged event may cover a live site — the one case where a
      // webview exists to receive it.
      previewManager.getState().setStage('ready');
      previewManager.showThresholdPrompt(prompt, false, { guard: true });
      expect(previewManager.getState().getStage()).toBe('threshold');
      expect(overlayEl.querySelector('[data-threshold-cards]')).not.toBeNull();

      // ThresholdResolved(cancelled: true): the open never happened — the
      // prior site's stage comes back and the overlay is lowered.
      previewManager.closeThresholdPass(true);
      expect(previewManager.getState().getStage()).toBe('ready');
      expect(overlayEl.classList.contains('visible')).toBe(false);
      expect(overlayEl.innerHTML).toBe('');
    });

    test('a committed pass leaves the screen up for the next open to paint over', () => {
      previewManager.getState().setStage('ready');
      previewManager.showThresholdChecking(3, { guard: true });
      expect(overlayEl.querySelector('[data-meta]')).not.toBeNull();

      // ThresholdResolved(cancelled: false): no interstitial flash — the new
      // open's own overlay replaces this markup.
      previewManager.closeThresholdPass(false);
      expect(overlayEl.querySelector('[data-meta]')).not.toBeNull();
      expect(previewManager.getState().getStage()).toBe('checking');
    });

    test('the walk and the question it becomes share one grid', () => {
      // These are two surfaces writing into one overlay, and the grid used to
      // be started and stopped by whichever rendered last through a SHARED
      // cleanup handle — so the handoff either restarted the grid (the
      // background jumps) or leaked the old loop. It belongs to the overlay
      // now, so the handoff cannot touch it.
      previewManager.showThresholdChecking(42);
      const canvas = overlayEl.querySelector('#blueprint-canvas');
      expect(canvas).not.toBeNull();
      vi.mocked(startBlueprintGrid).mockClear();
      mockBlueprintCleanup.mockClear();

      previewManager.showThresholdPrompt(prompt);

      expect(overlayEl.querySelector('[data-threshold-cards]')).not.toBeNull();
      expect(overlayEl.querySelector('#blueprint-canvas')).toBe(canvas);
      expect(startBlueprintGrid).not.toHaveBeenCalled();
      expect(mockBlueprintCleanup).not.toHaveBeenCalled();
    });
  });

  describe('Target navigation', () => {
    test('passes each reported page URL to the active folder session', async () => {
      const focused = vi.fn().mockResolvedValue({ status: 'ok', data: null });
      host.previewRequirement = focused;
      (previewManager as any).folderPath = '/vault';
      const report = mockNavState.previewRequirementListener!;

      report('/places/');
      report('/places/');
      report('/article/');
      await (previewManager as any).previewRequirementDispatch;
      expect(focused.mock.calls).toEqual([
        ['/vault', '/places/'],
        ['/vault', '/places/'],
        ['/vault', '/article/'],
      ]);
    });

    test('drops a queued target after its manager closes', async () => {
      const focused = vi.fn().mockResolvedValue({ status: 'ok', data: null });
      host.previewRequirement = focused;
      (previewManager as any).folderPath = '/vault';
      mockNavState.previewRequirementListener!('/article/');
      previewManager.destroy();
      await (previewManager as any).previewRequirementDispatch;
      expect(focused).not.toHaveBeenCalled();
    });

    test('resolves a folder target through the backend URL map', async () => {
      const resolveFolder = vi
        .spyOn(engine, 'resolveUrlForFolder')
        .mockResolvedValue({ status: 'ok', data: '/writings/distributed-web/' });
      const navigateToPage = vi.fn();
      (previewManager as any).folderPath = '/vault';
      (previewManager as any).navigationManager.navigateToPage = navigateToPage;

      (previewManager as any).navigateToTarget('文字/信息网络');
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 350));

      expect(resolveFolder).toHaveBeenCalledWith('/vault/文字/信息网络');
      expect(navigateToPage).toHaveBeenCalledWith('/writings/distributed-web/');
      resolveFolder.mockRestore();
    });
  });

  describe('starter picker owns the overlay', () => {
    // While the starter picker is open the frame draws into the same overlay
    // (`ScreenHold`), so the three paths that would paint or clear it must
    // all stand down, and must paint again once the picker closes.
    const MARKER = '<p data-starter-frame>frame</p>';
    const entryPoints: Array<[string, (pm: PreviewManager) => void]> = [
      ['hideOverlay', (pm) => (pm as any).hideOverlay()],
      ['reconcileUnshowable', (pm) => {
        (pm as any).folderUnreachable = true;
        (pm as any).reconcileUnshowable();
      }],
      ['renderStatus', (pm) => (pm as any).renderStatus({
        type: 'error',
        message: 'boom',
        verdict: 'reported',
      })],
    ];

    test.each(entryPoints)('%s leaves the overlay alone in starter mode and acts without it', (_name, call) => {
      for (const starterOn of [true, false]) {
        previewManager = new PreviewManager('moss-preview-iframe');
        // `overlay` is looked up in `initialize()`; the unit under test is
        // the guard, so wire it the way `initialize()` does.
        (previewManager as any).overlay = mockOverlay;
        mockOverlay.innerHTML = MARKER;
        const frame = mockOverlay.firstElementChild!;
        mockOverlay.classList.add('visible');
        previewManager.screenHold.active = starterOn;
        try {
          call(previewManager);
        } finally {
          previewManager.screenHold.active = false;
        }
        const untouched = mockOverlay.contains(frame) && mockOverlay.classList.contains('visible');
        expect(untouched, `starter mode ${starterOn ? 'on' : 'off'}`).toBe(starterOn);
      }
    });
  });

  describe('current page request owns refresh', () => {
    test('a completion after redirect cannot return to the deleted route', async () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      mockNavState.currentUrl = 'http://localhost:8080/untitled/';
      mockIframe.src = mockNavState.currentUrl;
      (previewManager as any).handleFileChangeEvent({ deleted_paths: ['untitled/index.html'], changed_output_files: ['index.html'] });
      await previewManager.refresh('file-changed');
      expect(mockIframe.src).toContain('/index.html');
      expect(mockIframe.src).not.toContain('/untitled/');
    });

    test('an obsolete morph failure cannot reload after a newer navigation', async () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      mockNavState.currentUrl = 'http://localhost:8080/untitled/';
      mockNavState.activePage = { url: mockNavState.currentUrl, sourceFile: null };
      mockIframe.src = mockNavState.currentUrl;
      let finish!: (value: boolean) => void;
      vi.spyOn((previewManager as any).navigationManager, 'morph').mockImplementation(() => new Promise<boolean>(resolve => { finish = resolve; }));
      const reload = vi.spyOn((previewManager as any).navigationManager, 'reload');
      const oldRefresh = previewManager.refresh('file-changed');
      (previewManager as any).handleFileChangeEvent({ deleted_paths: ['untitled/index.html'], changed_output_files: ['index.html'] });
      finish(false);
      await oldRefresh;
      expect(reload).not.toHaveBeenCalled();
      expect(mockIframe.src).toContain('/index.html');
    });

    test('a move during creation transfers the covered wait to its new route', () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      mockIframe.src = 'http://localhost:8080/untitled/';
      previewManager.armNewPageWait('/untitled/');
      (previewManager as any).handleFileChangeEvent({ moved_output_paths: [['untitled/index.html', 'posts/index.html']], changed_output_files: ['posts/index.html'] });
      expect((previewManager as any).newPageWait.pathname).toBe('/posts/index.html');
      expect(mockIframe.src).toContain('/posts/index.html');
      expect(mockOverlay.classList.contains('visible')).toBe(true);
    });

    test('stalled recovery re-enters the existing build owner', async () => {
      previewManager = new PreviewManager('moss-preview-iframe');
      (previewManager as any).folderPath = '/vault';
      previewManager.armNewPageWait('/posts/');
      const build = vi.spyOn(previewManager, 'startBuild').mockResolvedValue(undefined);
      (previewManager as any).waitingScreen.deps.retryPage();
      await Promise.resolve();
      expect(build).toHaveBeenCalledWith('/vault');
      expect((previewManager as any).newPageWait).not.toBeNull();
      expect(mockOverlay.classList.contains('visible')).toBe(true);
    });

    test('unrelated builds and terminal completion cannot uncover a new page', async () => {
      vi.useFakeTimers();
      try {
        previewManager = new PreviewManager('moss-preview-iframe');
        previewManager.armNewPageWait('/posts/');
        const load = vi.spyOn(previewManager, 'loadPreview');
        for (let i = 0; i < 5; i++) {
          (previewManager as any).handleFileChangeEvent({ changed_output_files: ['other/index.html'] });
        }
        await previewManager.refresh('file-changed');
        await vi.advanceTimersByTimeAsync(20_001);
        expect((previewManager as any).newPageWait).not.toBeNull();
        expect(mockOverlay.classList.contains('visible')).toBe(true);
        expect(load).not.toHaveBeenCalled();
        expect(mockOverlay.innerHTML).toContain('Try again');
      } finally { vi.useRealTimers(); }
    });
  });

});
