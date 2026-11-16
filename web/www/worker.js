// The worker of the cut. It owns the browser objects that the WebAssembly module (src/lib.rs) uses through the object
// `pbfcut`: the input file, a pool of OPFS files for the temporary files, and one OPFS file for all tile files. The
// tile files go one after the other into that file; `parts` keeps where each one is: tile number -> [[start, length]].
// The page makes a new worker for each cut and stops it after the cut (also for the button Stop).

const POOL = 160;                    // the temporary files at one time: a sync access handle is made before the cut
const pool = [], free = [];
const parts = new Map();
// a write to OPFS writes all bytes, or stops the cut: a full storage can give a short write or a wrong count
function writeAll(h, buf, at) {
  for (let n = 0; n < buf.length;) {
    const w = h.write(buf.subarray(n), { at: at + n });
    if (!(w > 0 && w <= buf.length - n)) {
      throw new Error(`a write to the storage of the site failed (it wrote ${w} of ${buf.length - n} bytes): the storage is possibly full`);
    }
    n += w;
  }
  return buf.length;
}
let input, out, outEnd = 0, last = -1, memory = null;

self.pbfcut = {
  input_read(at, buf) {
    if (input.handle) return input.handle.read(buf, { at });
    const b = new FileReaderSync().readAsArrayBuffer(input.file.slice(at, at + buf.length));
    buf.set(new Uint8Array(b));
    return b.byteLength;
  },
  temp_create() {
    const i = free.pop();
    if (i === undefined) throw new Error(`all ${POOL} temporary files are in use`);
    pool[i].truncate(0);
    return i;
  },
  temp_read: (i, at, buf) => pool[i].read(buf, { at }),
  temp_write: (i, at, buf) => writeAll(pool[i], buf, at),
  temp_drop: (i) => { free.push(i); },
  tile_write(t, buf) {
    if (t !== last) {
      if (!parts.has(t)) parts.set(t, []);
      parts.get(t).push([outEnd, 0]);
      last = t;
    }
    writeAll(out, buf, outEnd);
    const p = parts.get(t);
    p[p.length - 1][1] += buf.length;
    outEnd += buf.length;
  },
  step: (name) => postMessage({ type: "step", name, t: performance.now(), memory: memory?.buffer.byteLength }),
};

// o: {file: File} or {region: name of a file in the OPFS folder "regions"}, and zoom, boundary (GeoJSON text or null),
// border (m), complete (types), threads
onmessage = async ({ data: o }) => {
  try {
    const threads = self.crossOriginIsolated && o.threads > 1;
    const pkg = threads ? "pkg-threads" : "pkg";
    const m = await import(`./${pkg}/pbfcut_web.js?v=dev`);
    await m.default({ module_or_path: new URL(`./${pkg}/pbfcut_web_bg.wasm?v=dev`, import.meta.url) });
    postMessage({ type: "stage", name: threads ? `starting ${o.threads} threads` : "one thread" });
    if (threads) await m.initThreadPool(o.threads);
    memory = m.wasm_memory();
    // a shared memory goes to the page, which reads its size while the cut runs; else the size comes with each step
    postMessage({ type: "memory", memory: threads ? memory : null, bytes: memory.buffer.byteLength });

    const root = await navigator.storage.getDirectory();
    await root.removeEntry("cut", { recursive: true }).catch(() => {});
    const dir = await root.getDirectoryHandle("cut", { create: true });
    const tmp = await dir.getDirectoryHandle("temp", { create: true });
    postMessage({ type: "stage", name: `opening ${POOL} temporary files in OPFS` });
    for (let i = 0; i < POOL; i++) {
      pool.push(await (await tmp.getFileHandle(`${i}`, { create: true })).createSyncAccessHandle());
      free.push(POOL - 1 - i);
    }
    out = await (await dir.getFileHandle("tiles", { create: true })).createSyncAccessHandle();
    let size;
    if (o.region) {
      const regions = await root.getDirectoryHandle("regions");
      input = { handle: await (await regions.getFileHandle(o.region)).createSyncAccessHandle() };
      size = input.handle.getSize();
    } else {
      input = { file: o.file };
      size = o.file.size;
    }
    postMessage({ type: "stage", name: "cut", t: performance.now() });
    const report = JSON.parse(o.kind === "area"
      ? m.cut_area(size, o.boundary, o.name, 0)
      : m.cut(size, o.zoom, o.boundary, o.border, o.complete, 0));
    out.flush();
    out.close();
    for (const h of pool) h.close();
    input.handle?.close();
    await dir.removeEntry("temp", { recursive: true });
    postMessage({ type: "done", report, parts: [...parts], bytes: outEnd, memory: memory.buffer.byteLength });
  } catch (e) {
    postMessage({ type: "error", message: String(e?.message ?? e) });
  }
};
