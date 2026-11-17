// One .zip file of many files, for the browsers that cannot save into a folder (Firefox, Safari, phones). The files
// are "stored" (method 0, no compression): the blocks of an .osm.pbf file are compressed already. The zip is a Blob
// made of the parts of the files as they are (slices of the OPFS file of the cut), so the page holds only the headers
// in memory, also for a cut of some GB. The cut gives the CRC-32 of each file, so the zip reads no file again. ZIP64 comes when a size or
// a place passes 4 GiB (or with `zip64` true, for the test).

const TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

// the CRC-32 of a Blob, read in pieces of 8 MiB
async function crc32(blob) {
  let c = 0xffffffff;
  for (let at = 0; at < blob.size; at += 8 << 20) {
    const b = new Uint8Array(await blob.slice(at, at + (8 << 20)).arrayBuffer());
    for (let i = 0; i < b.length; i++) c = TABLE[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

// little-endian bytes: [[value, size in bytes], ...]
function bytes(fields) {
  const out = new Uint8Array(fields.reduce((n, [, size]) => n + size, 0));
  const v = new DataView(out.buffer);
  let at = 0;
  for (const [x, size] of fields) {
    if (size === 2) v.setUint16(at, x, true);
    else if (size === 4) v.setUint32(at, x, true);
    else v.setBigUint64(at, BigInt(x), true);
    at += size;
  }
  return out;
}

const MAX = 0xffffffff;

// files: [{ name, blob, crc? }]; onProgress(done files, all files). Gives a Blob of the zip.
export async function zipBlob(files, onProgress = () => {}, zip64 = false) {
  const now = new Date();
  const time = (now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1);
  const date = ((now.getFullYear() - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate();
  const parts = [], central = [];
  let at = 0;
  for (const [i, f] of files.entries()) {
    const name = new TextEncoder().encode(f.name);
    // the cut gives the CRC-32 of each file (it computes it while it writes); else one read of the file
    const size = f.blob.size, crc = f.crc ?? await crc32(f.blob);
    const big = zip64 || size >= MAX || at >= MAX;
    // a local header: with ZIP64, the sizes are in the extra field
    const extra = big ? bytes([[1, 2], [16, 2], [size, 8], [size, 8]]) : new Uint8Array();
    const local = bytes([[0x04034b50, 4], [big ? 45 : 20, 2], [0x0800, 2], [0, 2], [time, 2], [date, 2], [crc, 4],
      [big ? MAX : size, 4], [big ? MAX : size, 4], [name.length, 2], [extra.length, 2]]);
    parts.push(local, name, extra, f.blob);
    // its entry in the central directory: with ZIP64, the sizes and the place of the local header in the extra field
    const cextra = big ? bytes([[1, 2], [24, 2], [size, 8], [size, 8], [at, 8]]) : new Uint8Array();
    central.push(bytes([[0x02014b50, 4], [45, 2], [big ? 45 : 20, 2], [0x0800, 2], [0, 2], [time, 2], [date, 2], [crc, 4],
      [big ? MAX : size, 4], [big ? MAX : size, 4], [name.length, 2], [cextra.length, 2], [0, 2], [0, 2], [0, 2], [0, 4],
      [big ? MAX : at, 4]]), name, cextra);
    at += local.length + name.length + extra.length + size;
    onProgress(i + 1, files.length);
  }
  const dirSize = central.reduce((n, b) => n + b.length, 0), dirAt = at, n = files.length;
  const end = [];
  if (zip64 || dirAt >= MAX || n >= 0xffff) {
    // the ZIP64 end of the central directory, and its locator
    end.push(bytes([[0x06064b50, 4], [44, 8], [45, 2], [45, 2], [0, 4], [0, 4], [n, 8], [n, 8], [dirSize, 8], [dirAt, 8]]));
    end.push(bytes([[0x07064b50, 4], [0, 4], [dirAt + dirSize, 8], [1, 4]]));
    end.push(bytes([[0x06054b50, 4], [0, 2], [0, 2], [0xffff, 2], [0xffff, 2], [MAX, 4], [MAX, 4], [0, 2]]));
  } else {
    end.push(bytes([[0x06054b50, 4], [0, 2], [0, 2], [n, 2], [n, 2], [dirSize, 4], [dirAt, 4], [0, 2]]));
  }
  return new Blob([...parts, ...central, ...end], { type: "application/zip" });
}
