/**
 * The scratch site the header-over-ink render gate builds: three pages whose
 * opening hero carries `header=over`, over a pale, a dark and a mid-tone
 * picture, each with faint invented lines, a fourth with the words at the top,
 * and a footer with a picture wider than a phone above its closing picture. Served by
 * playwright/header-over-ink.config.ts.
 */
import { deflateSync } from "node:zlib";
import type { ScratchSiteSpec } from "./scratch-site";
import { chunk } from "./contents-ruler-site";

/** A picture of one grey with a line a shade off it every 40px, both ways. */
function lined(width: number, height: number, grey: number, line: number): Buffer {
  const rows: Buffer[] = [];
  for (let y = 0; y < height; y++) {
    const row = Buffer.alloc(1 + width * 3, y % 40 < 2 ? line : grey);
    row[0] = 0;
    for (let x = 0; x < width; x += 40) row.fill(line, 1 + x * 3, 1 + (x + 2) * 3);
    rows.push(row);
  }
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

/** Page path, and the ink the build should choose for it. */
export const PAGES = [
  { path: "/", name: "pale", ink: "dark" },
  { path: "/dusk/", name: "dusk", ink: "light" },
  { path: "/haze/", name: "haze", ink: "none" },
] as const;

const page = (title: string, uid: string, picture: string) => `---
title: ${title}
uid: "${uid}"
---

:::hero {image=${picture} header=over}
# ${title}
:::

Body text below the hero.
`;

export const HEADER_OVER_INK_GATE: ScratchSiteSpec = {
  name: "header-over-ink-gate",
  files: {
    "pale.png": lined(1600, 700, 246, 228),
    "dusk.png": lined(1600, 700, 24, 44),
    "haze.png": lined(1600, 700, 128, 140),
    "index.md": page("Pale chart", "hoi00001", "pale.png"),
    "dusk.md": page("Dusk", "hoi00002", "dusk.png"),
    "haze.md": page("Haze", "hoi00003", "haze.png"),
    "top.md": `---
title: Top
uid: "hoi00005"
---

:::hero {image=pale.png header=over align=top mobile=overlay}
## Words at the top of the picture
:::

Body text below the hero.
`,
    "mark.png": lined(800, 800, 90, 110),
    "footer.md": `![[mark.png|A mark]]

Come and visit.

:::hero {image=dusk.png alt="Dusk" align=top fade=top max-height="clamp(400px, 30vw, 460px)"}
:::
`,
    "About.md": `---
title: About
uid: "hoi00004"
---

About this invented site.
`,
    ".moss/config.toml": `schema_version = 5

[site]
lang = "en"
`,
    ".moss/theme/style.css": null,
  },
};
