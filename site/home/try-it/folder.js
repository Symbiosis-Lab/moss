// The visitor's folder, written out as a zip or into a directory they pick.
// Kept free of the DOM and of the wasm so Node can test it.

const CRC_TABLE = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});

function crc32(bytes) {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

async function bytesOf(data) {
  if (typeof data === 'string') return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  return new Uint8Array(await data.arrayBuffer());
}

/**
 * A stored (uncompressed) zip of `files` under one top folder. Pictures are
 * already compressed, so deflating would cost time for nothing. Names are
 * flagged UTF-8 so a folder named in Chinese unpacks with its name.
 * @param {string} folder
 * @param {Array<{path: string, data: string | Blob | Uint8Array}>} files
 */
export async function zipFolder(folder, files) {
  const enc = new TextEncoder();
  const parts = [];
  const central = [];
  let offset = 0;
  const entries = [{ path: `${folder}/`, bytes: new Uint8Array(0) }];
  for (const f of files) entries.push({ path: `${folder}/${f.path}`, bytes: await bytesOf(f.data) });
  for (const { path, bytes } of entries) {
    const name = enc.encode(path);
    const crc = crc32(bytes);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true);
    local.setUint16(6, 0x0800, true);
    local.setUint32(14, crc, true);
    local.setUint32(18, bytes.length, true);
    local.setUint32(22, bytes.length, true);
    local.setUint16(26, name.length, true);
    parts.push(new Uint8Array(local.buffer), name, bytes);
    const head = new DataView(new ArrayBuffer(46));
    head.setUint32(0, 0x02014b50, true);
    head.setUint16(4, 20, true);
    head.setUint16(6, 20, true);
    head.setUint16(8, 0x0800, true);
    head.setUint32(16, crc, true);
    head.setUint32(20, bytes.length, true);
    head.setUint32(24, bytes.length, true);
    head.setUint16(28, name.length, true);
    head.setUint32(38, path.endsWith('/') ? 0x10 : 0, true);
    head.setUint32(42, offset, true);
    central.push(new Uint8Array(head.buffer), name);
    offset += 30 + name.length + bytes.length;
  }
  const size = central.reduce((n, p) => n + p.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, entries.length, true);
  end.setUint16(10, entries.length, true);
  end.setUint32(12, size, true);
  end.setUint32(16, offset, true);
  return new Blob([...parts, ...central, new Uint8Array(end.buffer)], { type: 'application/zip' });
}

/**
 * Writes `files` into a new folder named `folder` inside `dir`, a
 * FileSystemDirectoryHandle (from the directory picker, or the origin's
 * private storage in tests). Returns the folder's handle.
 */
export async function writeFolder(dir, folder, files) {
  const root = await dir.getDirectoryHandle(folder, { create: true });
  for (const f of files) {
    const handle = await root.getFileHandle(f.path, { create: true });
    const out = await handle.createWritable();
    await out.write(await bytesOf(f.data));
    await out.close();
  }
  return root;
}

/**
 * `text` with the picture line the editor writes for a dropped picture
 * appended as its own paragraph. `destination` is the engine's escaped path.
 */
export function withPicture(text, destination) {
  const body = text.replace(/\s*$/, '');
  return `${body}${body ? '\n\n' : ''}![](${destination})\n`;
}
