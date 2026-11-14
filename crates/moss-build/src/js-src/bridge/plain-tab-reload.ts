/**
 * A page open in a plain browser tab reloads when the preview server has
 * rebuilt it.
 *
 * A browser gives one origin six connections over plain HTTP, so a stream in
 * every tab would leave the seventh tab waiting for its page. One tab holds a
 * lock and the stream, and tells the others of each rebuild; when it goes
 * away the lock passes to another tab, which opens the stream in its turn.
 */
const SHARED = "moss-reload";

export function installPlainTabReload(
  win: Window & typeof globalThis,
  reload: () => void = () => win.location.reload(),
): void {
  const listen = (rebuilt: () => void) => {
    const stream = new win.EventSource("/__moss/reload");
    let opened = false;
    stream.onopen = () => {
      // Reopened after a drop: a rebuild may have gone by unseen.
      if (opened) rebuilt();
      opened = true;
    };
    stream.onmessage = rebuilt;
  };
  // Locks need a secure context, which a page served to another device on the network is not.
  const locks = win.navigator?.locks;
  if (!locks || typeof win.BroadcastChannel !== "function") return listen(reload);
  const tabs = new win.BroadcastChannel(SHARED);
  tabs.onmessage = reload;
  // The tab the lock passes to does not reload, though a rebuild may have
  // gone by between two holders: every reload hands the lock on, so the tabs
  // would reload each other in turn without end.
  const whileThisPageLives = () =>
    new Promise<never>(() =>
      listen(() => {
        tabs.postMessage(null);
        reload();
      }),
    );
  // A browser that refuses the lock leaves this tab its own stream.
  locks.request(SHARED, whileThisPageLives).catch(() => listen(reload));
}
