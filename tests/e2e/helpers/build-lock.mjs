import fs from "node:fs";
import path from "node:path";

const nap = new Int32Array(new SharedArrayBuffer(4));
/** Playwright parses configs synchronously, so there is no event loop to yield to. */
const sleepSync = (ms) => Atomics.wait(nap, 0, 0, ms);

const readPid = (lock) => {
  try {
    return fs.readFileSync(path.join(lock, "pid"), "utf8");
  } catch {
    return null;
  }
};

/** The pid file's contents when the lock's holder is gone: its pid has exited,
 *  or it never wrote one within graceMs of making the lock (it died between the
 *  two). Null while the holder lives. */
function goneHolder(lock, graceMs) {
  const pid = readPid(lock);
  if (pid === null) {
    try {
      return Date.now() - fs.statSync(lock).mtimeMs > graceMs ? "" : null;
    } catch {
      return null; // released meanwhile: the next mkdir decides
    }
  }
  try {
    process.kill(Number(pid), 0);
    return null;
  } catch (e) {
    return e.code === "ESRCH" ? pid : null;
  }
}

/** Takes `lock`, a directory made atomically, or waits for another process's
 *  build: `ready()` returns its result once published. A lock whose holder is
 *  gone (killed mid-build, its lock left behind) is removed and taken, and a lock
 *  released without a result is taken too, so a waiter never times out on a
 *  build nobody is running.
 *  @template T
 *  @param {string} lock
 *  @param {() => T | null} ready
 *  @param {{ tag?: string, deadlineMs?: number, graceMs?: number, pollMs?: number }} [opts]
 *  @returns {{ held: true } | { held: false, value: T }} held: this process builds, then removes `lock`. */
export function takeOrAwait(lock, ready, { tag = "", deadlineMs = 150_000, graceMs = 5_000, pollMs = 500 } = {}) {
  fs.mkdirSync(path.dirname(lock), { recursive: true });
  const deadline = Date.now() + deadlineMs;
  for (;;) {
    try {
      fs.mkdirSync(lock);
      fs.writeFileSync(path.join(lock, "pid"), String(process.pid));
      return { held: true };
    } catch (e) {
      if (e.code !== "EEXIST") throw e;
    }
    const value = ready();
    if (value) return { held: false, value };
    const gone = goneHolder(lock, graceMs);
    // Re-read just before removing: another waiter may have taken it over since.
    if (gone !== null && (readPid(lock) ?? "") === gone) {
      fs.rmSync(lock, { recursive: true, force: true });
      continue;
    }
    if (Date.now() > deadline) throw new Error(`${tag} timed out waiting for another process to build the scratch site`);
    sleepSync(pollMs);
  }
}
