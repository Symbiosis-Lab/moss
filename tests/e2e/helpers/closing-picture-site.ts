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

/** What makes a picture busy: sky hatching, a dark patch in the right half of the sky, or hatching along the foot. */
interface Busy {
  /** Every other sky row is this colour, as an etching's lines are. */
  hatch?: Rgb;
  /** A patch of this colour over the right half of the top 30% of the frame. */
  patch?: Rgb;
  /** Every other row of the bottom 18% is this colour. */
  foot?: Rgb;
  /** The top 4% of the frame is this colour. */
  strip?: Rgb;
}

/** A landscape: a flat sky over the top `horizon` of the frame, ground below. */
export function landscapePng(width: number, height: number, sky: Rgb, ground: Rgb, horizon: number, busy: Busy = {}): Buffer {
  const rowOf = ([r, g, b]: Rgb, patch?: Rgb) => {
    const row = Buffer.alloc(1 + width * 3);
    for (let x = 0; x < width; x++) row.set(patch && x >= width / 2 ? patch : [r, g, b], 1 + x * 3);
    return row;
  };
  const rows = Array.from({ length: height }, (_, y) => {
    const inSky = y < height * horizon;
    if (busy.strip && y < height * 0.04) return rowOf(busy.strip);
    if (inSky) return rowOf(busy.hatch && y % 2 === 0 ? busy.hatch : sky, y < height * 0.3 ? busy.patch : undefined);
    return rowOf(busy.foot && y >= height * 0.82 && y % 2 === 0 ? busy.foot : ground);
  });
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

export const SKY: Rgb = [205, 218, 232];
export const SKY_NIGHT: Rgb = [20, 28, 48];
export const GROUND: Rgb = [60, 70, 64];
export const CREDIT = "Etching of the harbour at first light, 1643. Public domain.";
export const HEADING = "The shelf is open. What should stand on it next?";
export const LINK_TEXT = "Propose a collection";

const filler = (n: number): string =>
  Array.from({ length: n }, () => "The harbour wall holds the morning for a while before it lets the light through.").join("\n\n");

export const CLOSING_PICTURE_GATE: ScratchSiteSpec = {
  name: "closing-picture-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\nfloating_nav = true\n`,
    "closing.png": landscapePng(1600, 700, SKY, GROUND, 0.7),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, [8, 10, 14], 0.7),
    "busy.png": landscapePng(1600, 700, SKY, GROUND, 0.7, { hatch: [10, 14, 24] }),
    "patch.png": landscapePng(1600, 700, SKY, GROUND, 0.7, { patch: [10, 14, 24] }),
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
    // The same picture three ways on ordinary pages: a hero that asks to be
    // measured over a quiet sky and over a hatched one, and one that does not ask.
    "Hero/Quiet.md": `---\ntitle: Quiet\nuid: "cpg00103"\n---\n\n:::hero {image=closing.png align=top mobile=overlay panel=auto}\n## Quiet sky\n\n[Open](https://example.org/a)\n:::\n`,
    "Hero/Busy.md": `---\ntitle: Busy\nuid: "cpg00104"\n---\n\n:::hero {image=busy.png align=top mobile=overlay panel=auto}\n## Busy sky\n\n[Open](https://example.org/b)\n:::\n`,
    "Hero/Patch.md": `---\ntitle: Patch\nuid: "cpg00106"\n---\n\n:::hero {image=patch.png align=top mobile=overlay panel=auto}\n## Dark patch on the right\n\n[Open](https://example.org/d)\n:::\n`,
    "Hero/Plain.md": `---\ntitle: Plain\nuid: "cpg00105"\n---\n\n:::hero {image=closing.png align=top mobile=overlay}\n## Plain hero\n\n[Open](https://example.org/c)\n:::\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top caption="${CREDIT}"}
## ${HEADING}

[${LINK_TEXT}](https://example.org/new)
:::
`,
  },
};

/** Pale picture pair whose foot is hatched in both: the credit cannot be set in ink there. */
export const FOOT_LIGHT: Rgb = [214, 220, 214];
export const FOOT_NIGHT: Rgb = [12, 16, 22];

/** A second closing picture whose foot is busy, so its credit sits on a small panel of its own. */
export const CLOSING_PICTURE_BUSY_GATE: ScratchSiteSpec = {
  name: "closing-picture-busy-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\n`,
    "closing.png": landscapePng(1600, 700, SKY, FOOT_LIGHT, 0.7, { foot: [10, 14, 24] }),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, FOOT_NIGHT, 0.7, { foot: [230, 235, 240] }),
    "index.md": `---\ntitle: Busy Foot\nuid: "cpb00101"\n---\n\n# Busy Foot\n\nAn invented site for the busy-foot case.\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};

/** A tall closing picture (1600x1050) with a dark patch in the right half of its sky: taller than a short window shows, and a phone's words keep a panel. */
export const PATCH: Rgb = [10, 14, 24];
export const CLOSING_PICTURE_SKY_GATE: ScratchSiteSpec = {
  name: "closing-picture-sky-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\n`,
    "closing.png": landscapePng(1600, 1050, SKY, GROUND, 0.9, { patch: PATCH }),
    "closing-dark.png": landscapePng(1600, 1050, SKY_NIGHT, [8, 10, 14], 0.9, { patch: [230, 235, 240] }),
    "index.md": `---\ntitle: Tall Shelf\nuid: "cps00101"\n---\n\n# Tall Shelf\n\nAn invented site for the tall closing picture.\n`,
    "Full.md": `---\ntitle: Full\nuid: "cps00102"\ncontent_width: full\n---\n\n# Full\n\nA page whose text column is the site's widest.\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};

/**
 * A closing picture with `fade=top`: a dark strip over the top 4% of a pale
 * sky, which neither ink reads on until the top fades into the page, and a
 * dark picture for the dark theme.
 */
export const CLOSING_PICTURE_FADE_GATE: ScratchSiteSpec = {
  name: "closing-picture-fade-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\n`,
    "closing.png": landscapePng(1600, 700, SKY, GROUND, 0.7, { strip: PATCH }),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, [8, 10, 14], 0.7),
    "index.md": `---\ntitle: Faded Shelf\nuid: "cpf00101"\n---\n\n# Faded Shelf\n\nAn invented site for the faded closing picture.\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top fade=top caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};

/** The busy pair with `fade=top`: hatched under the words too, so the words keep their panel and the credit its own at the foot. */
export const CLOSING_PICTURE_BUSY_FADE_GATE: ScratchSiteSpec = {
  name: "closing-picture-busy-fade-gate",
  files: {
    ...CLOSING_PICTURE_BUSY_GATE.files,
    "closing.png": landscapePng(1600, 700, SKY, FOOT_LIGHT, 0.7, { hatch: [10, 14, 24], foot: [10, 14, 24] }),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, FOOT_NIGHT, 0.7, { hatch: [230, 235, 240], foot: [230, 235, 240] }),
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top fade=top caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};


/** The capped picture's height, as its fence writes it and as the gate checks it. */
export const CAP = "clamp(440px, 50vw, 680px)";
export const capAt = (width: number) => Math.min(680, Math.max(440, width / 2));

/**
 * A faded closing picture with a cap and a position, the shape of a 1400x1073
 * print: a dark strip over its top 4%, which the cap crops away from 48rem and
 * a phone shows only inside the fade.
 */
export const CLOSING_PICTURE_CAPPED_GATE: ScratchSiteSpec = {
  name: "closing-picture-capped-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\n`,
    "closing.png": landscapePng(1400, 1073, SKY, GROUND, 0.7, { strip: PATCH }),
    "closing-dark.png": landscapePng(1400, 1073, SKY_NIGHT, [8, 10, 14], 0.7, { strip: [230, 235, 240] }),
    "index.md": `---\ntitle: Capped Shelf\nuid: "cpc00101"\n---\n\n# Capped Shelf\n\nAn invented site for the capped closing picture.\n`,
    "footer.md": `:::hero {image="closing.png|focus:50%,72%" dark=closing-dark.png align=top fade=top max-height="${CAP}" caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};

/** The closing picture with `[site] archival = true`: its edition block lands on the picture's foot. */
export const CLOSING_PICTURE_ARCHIVAL_GATE: ScratchSiteSpec = {
  name: "closing-picture-archival-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\narchival = true\n`,
    "closing.png": landscapePng(1600, 700, SKY, GROUND, 0.7),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, [8, 10, 14], 0.7),
    "index.md": `---\ntitle: Archive Shelf\nuid: "cpa00101"\n---\n\n# Archive Shelf\n\nAn invented site for the archival closing-picture gate.\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};

/** The archival closing picture with a credit: the credit and the edition stack at the foot. */
export const CLOSING_PICTURE_ARCHIVAL_CAPTION_GATE: ScratchSiteSpec = {
  name: "closing-picture-archival-caption-gate",
  files: {
    ".moss/config.toml": `schema_version = 5\n\n[site]\nlang = "en"\narchival = true\n`,
    "closing.png": landscapePng(1600, 700, SKY, GROUND, 0.7),
    "closing-dark.png": landscapePng(1600, 700, SKY_NIGHT, [8, 10, 14], 0.7),
    "index.md": `---\ntitle: Archive Shelf\nuid: "cpa00102"\n---\n\n# Archive Shelf\n\nAn invented site for the archival closing-picture gate with a credit.\n`,
    "footer.md": `:::hero {image=closing.png dark=closing-dark.png align=top caption="${CREDIT}"}\n## ${HEADING}\n\n[${LINK_TEXT}](https://example.org/new)\n:::\n`,
  },
};
