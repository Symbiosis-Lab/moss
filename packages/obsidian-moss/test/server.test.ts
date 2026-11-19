import * as http from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, expect, it } from "vitest";
import { pageFor, watchRebuilds } from "../src/desktop/server";

let server: http.Server | undefined;
afterEach(() => server?.closeAllConnections());

function serve(handler: http.RequestListener): Promise<string> {
  server = http.createServer(handler);
  return new Promise((resolve) => server!.listen(0, "127.0.0.1", () => resolve(`http://127.0.0.1:${(server!.address() as AddressInfo).port}`)));
}

it("asks the server which page a file builds to, and takes any other answer as none", async () => {
  const asked: string[] = [];
  const url = await serve((request, response) => {
    asked.push(request.url ?? "");
    if (request.url?.endsWith("/a%20note.md")) response.writeHead(307, { location: "/posts/a-note/" }).end();
    else response.writeHead(404).end();
  });
  expect(await pageFor(url, "posts/a note.md")).toBe("/posts/a-note/");
  expect(asked).toEqual(["/__moss/page/posts/a%20note.md"]);
  expect(await pageFor(url, "posts/unbuilt.md")).toBeNull();
  expect(await pageFor("http://127.0.0.1:1", "posts/a note.md")).toBeNull();
});

it("reports each rebuild and not the stream's keep-alive, until stopped", async () => {
  let stream: http.ServerResponse | undefined;
  const url = await serve((_request, response) => {
    response.writeHead(200, { "content-type": "text/event-stream" });
    response.write(": keep-alive\n\n");
    stream = response;
  });
  let rebuilds = 0;
  const stop = watchRebuilds(url, () => rebuilds++);
  await expect.poll(() => stream).toBeDefined();
  stream!.write("data: reload\n\n");
  await expect.poll(() => rebuilds).toBe(1);
  // A stream that drops is opened again, and the reopening counts as a rebuild that may have gone by.
  const first = stream!;
  first.destroy();
  await expect.poll(() => stream !== first, { timeout: 5000 }).toBe(true);
  await expect.poll(() => rebuilds).toBe(2);
  stop();
  await new Promise<void>((resolve) => stream!.on("close", () => resolve()));
});

it("does not keep asking a server that has no rebuild stream", async () => {
  let asked = 0;
  const url = await serve((_request, response) => { asked++; response.writeHead(404).end(); });
  let rebuilds = 0;
  const stop = watchRebuilds(url, () => rebuilds++);
  await new Promise((resolve) => setTimeout(resolve, 1500));
  stop();
  expect([asked, rebuilds]).toEqual([1, 0]);
});
