/**
 * The scratch site the closing-picture render gate builds: a footer.md whose
 * last block is a `:::hero` with a light and a dark picture, a short page, and
 * a page that asks for its contents open (which reserves a left column on the
 * body). All content is invented. Served by playwright/closing-picture.config.ts.
 */
import { deflateSync } from "node:zlib";
import { chunk } from "./contents-ruler-site";
import type { ScratchSiteSpec } from "./scratch-site";

type Rgb = [number, number, number];

/** A landscape: quiet flat sky over the top `horizon` of the frame, ground below. */
function landscapePng(width: number, height: number, sky: Rgb, ground: Rgb, horizon: number): Buffer {
  const rowOf = ([r, g, b]: Rgb) => {
    const row = Buffer.alloc(1 + width * 3);
    for (let x = 0; x < width; x++) row.set([r, g, b], 1 + x * 3);
    return row;
  };
  const rows = Array.from({ length: height }, (_, y) => rowOf(y < height * horizon ? sky : ground));
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(Buffer.concat(rows))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

export const HEADING = "The shelf is open. What should stand on it next?";
export const LINK_TEXT = "Propose a collection";

const filler = (n: number): string =>
  Array.from({ length: n }, () => "The harbour wall holds the morning for a while before it lets the light through.").join("\n\n");

export const CLOSING_PICTURE_GATE: ScratchSiteSpec = {
  name: "closing-picture-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\nfloating_nav = true\n`,
    "closing.png": landscapePng(1600, 700, [205, 218, 232], [60, 70, 64], 0.6),
    "closing-dark.png": landscapePng(1600, 700, [20, 28, 48], [8, 10, 14], 0.6),
    "index.md": `---
title: Harbour Shelf
uid: "cpg00101"
---

# Harbour Shelf

An invented site for the closing-picture render gate.
`,
    "Essays/Long.md": `---
title: A long essay
uid: "cpg00102"
contents: pictures
---

${["Quays", "Nets", "Ropes", "Lamps"].map((t) => `## ${t}\n\n${filler(8)}`).join("\n\n")}
`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top}
## ${HEADING}

[${LINK_TEXT}](https://example.org/new)

*Etching of the harbour at first light.*
:::
`,
  },
};
