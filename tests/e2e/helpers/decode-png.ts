import { inflateSync } from "node:zlib";

/** Decodes an 8-bit, non-interlaced PNG, as Playwright screenshots are, and throws on anything else. */
export function decodePng(png: Buffer): { width: number; height: number; at: (x: number, y: number) => [number, number, number, number] } {
  if (png.length < 33 || png.toString("ascii", 1, 4) !== "PNG") throw new Error(`not a nonempty PNG (${png.length} bytes)`);
  let offset = 8;
  let width = 0;
  let height = 0;
  let colorType = 0;
  const idatChunks: Buffer[] = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      const bitDepth = data.readUInt8(8);
      colorType = data.readUInt8(9);
      const interlace = data.readUInt8(12);
      if (bitDepth !== 8 || interlace !== 0) throw new Error(`unsupported PNG: bitDepth=${bitDepth} interlace=${interlace}`);
    } else if (type === "IDAT") {
      idatChunks.push(data);
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + length; // length + type(4) + data + crc(4)
  }
  const channels = colorType === 6 ? 4 : colorType === 2 ? 3 : (() => { throw new Error(`unsupported PNG colorType ${colorType}`); })();
  if (width < 1 || height < 1) throw new Error(`empty PNG dimensions ${width}x${height}`);
  const raw = inflateSync(Buffer.concat(idatChunks));
  const stride = width * channels;
  const pixels = Buffer.alloc(height * stride);
  const paeth = (a: number, b: number, c: number): number => {
    const p = a + b - c;
    const pa = Math.abs(p - a);
    const pb = Math.abs(p - b);
    const pc = Math.abs(p - c);
    if (pa <= pb && pa <= pc) return a;
    return pb <= pc ? b : c;
  };
  for (let y = 0; y < height; y++) {
    const rowStart = y * (stride + 1);
    const filterType = raw[rowStart];
    const prevRow = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
    for (let i = 0; i < stride; i++) {
      const x = raw[rowStart + 1 + i];
      const a = i >= channels ? pixels[y * stride + i - channels] : 0; // left
      const b = prevRow ? prevRow[i] : 0; // above
      const c = prevRow && i >= channels ? prevRow[i - channels] : 0; // upper-left
      let value: number;
      switch (filterType) {
        case 0: value = x; break;
        case 1: value = x + a; break;
        case 2: value = x + b; break;
        case 3: value = x + Math.floor((a + b) / 2); break;
        case 4: value = x + paeth(a, b, c); break;
        default: throw new Error(`unsupported PNG filter type ${filterType}`);
      }
      pixels[y * stride + i] = value & 0xff;
    }
  }
  return {
    width,
    height,
    at: (x: number, y: number) => {
      const i = y * stride + x * channels;
      return [pixels[i], pixels[i + 1], pixels[i + 2], channels === 4 ? pixels[i + 3] : 255];
    },
  };
}
