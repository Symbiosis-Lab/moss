// What a host asks the preview server it started. Neither read takes a token.
import * as http from "node:http";

/** The server path of the page a file in the folder builds to; null when it has no page yet or this moss cannot say. */
export function pageFor(server: string, file: string): Promise<string | null> {
  const encoded = file.split("/").map(encodeURIComponent).join("/");
  return new Promise((resolve) => {
    http.get(`${server}/__moss/page/${encoded}`, (response) => {
      response.resume();
      resolve(response.statusCode === 307 ? response.headers.location ?? null : null);
    }).on("error", () => resolve(null));
  });
}

/**
 * Calls `rebuilt` after each rebuild until the returned function is called. A
 * stream that drops while the server lives is opened again, and counts as a
 * rebuild, since one may have gone by unseen.
 */
export function watchRebuilds(server: string, rebuilt: () => void): () => void {
  let request: http.ClientRequest;
  let again: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const listen = (reopened: boolean) => {
    const reopen = () => {
      clearTimeout(again);
      if (!stopped) again = setTimeout(() => listen(true), 1000);
    };
    request = http.get(`${server}/__moss/reload`, (response) => {
      // A server without the stream answers once and is not asked again.
      if (response.statusCode !== 200) {
        response.resume();
        return;
      }
      if (reopened) rebuilt();
      // Keep-alive frames are comments; only a rebuild carries data.
      response.on("data", (chunk) => { if (/^data:/m.test(String(chunk))) rebuilt(); });
      response.on("close", reopen);
    });
    request.on("error", reopen);
  };
  listen(false);
  return () => {
    stopped = true;
    clearTimeout(again);
    request.destroy();
  };
}
