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
let input, out, outEnd = 0, memory = null;
// the time that the thread of the cut waits for the input file (read) and for the storage of the site (the temporary
// files and the tile files), in ms since the start: each step message gives both, and the page shows them for each step
let readMs = 0, storeMs = 0;
const timed = (add, f) => { const t = performance.now(); try { return f(); } finally { add(performance.now() - t); } };
const addRead = (ms) => { readMs += ms; }, addStore = (ms) => { storeMs += ms; };
const PIECE = 4 << 20, HELD = 64 << 20;
const held = new Map();      // tile -> { chunks, n }: the bytes that wait
let heldBytes = 0;
// the bytes that wait of tile t to the end of the file; a part that follows the last part of the tile makes it longer
function flushTile(t) {
  const h = held.get(t);
  if (!h) return;
  held.delete(t);
  heldBytes -= h.n;
  const b = h.chunks.length === 1 ? h.chunks[0] : new Uint8Array(h.n);
  if (h.chunks.length > 1) { let o = 0; for (const c of h.chunks) { b.set(c, o); o += c.length; } }
  if (!parts.has(t)) parts.set(t, []);
  const p = parts.get(t), lastPart = p[p.length - 1];
  if (lastPart && lastPart[0] + lastPart[1] === outEnd) lastPart[1] += b.length;
  else p.push([outEnd, b.length]);
  timed(addStore, () => writeAll(out, b, outEnd));
  outEnd += b.length;
}

self.pbfcut = {
  input_read(at, buf) {
    return timed(addRead, () => {
      if (input.handle) return input.handle.read(buf, { at });
      const b = new FileReaderSync().readAsArrayBuffer(input.file.slice(at, at + buf.length));
      buf.set(new Uint8Array(b));
      return b.byteLength;
    });
  },
  temp_create() {
    const i = free.pop();
    if (i === undefined) throw new Error(`all ${POOL} temporary files are in use`);
    pool[i].truncate(0);
    return i;
  },
  temp_read: (i, at, buf) => timed(addStore, () => pool[i].read(buf, { at })),
  temp_write: (i, at, buf) => timed(addStore, () => writeAll(pool[i], buf, at)),
  temp_drop: (i) => { free.push(i); },
  // the bytes of a tile wait in memory until 4 MiB (all tiles: at most 64 MiB): the cut gives a tile some 100 KB at a
  // time, mixed with the other tiles, and one part for each piece made France at zoom 8 73,356 parts; a page that
  // makes a Blob of each part (the .zip, Save all) was blocked for seconds
  tile_write(t, buf) {
    let h = held.get(t);
    if (!h) held.set(t, (h = { chunks: [], n: 0 }));
    h.chunks.push(buf.slice());      // a copy: buf is a view of the WebAssembly memory
    h.n += buf.length;
    heldBytes += buf.length;
    if (h.n >= PIECE) flushTile(t);
    if (heldBytes > HELD) for (const [u] of [...held].sort((a, b) => b[1].n - a[1].n)) { flushTile(u); if (heldBytes <= HELD / 2) break; }
  },
  step: (name) => postMessage({ type: "step", name, t: performance.now(), memory: memory?.buffer.byteLength, read: readMs, store: storeMs }),
};

// The plan for the map of the page (o.plan): the outlines of the tiles, cells or areas, before the cut. It reads only
// the header of the file, with the module of one thread. The page keeps one worker for the plans.
let planModule = null;
async function plan(o) {
  if (!planModule) {
    planModule = await import("./pkg/pbfcut_web.js?v=dev");
    await planModule.default({ module_or_path: new URL("./pkg/pbfcut_web_bg.wasm?v=dev", import.meta.url) });
  }
  let file = o.file ?? null;     // none: no input yet (the plan of a boundary or of the areas)
  if (o.region) {
    const regions = await (await navigator.storage.getDirectory()).getDirectoryHandle("regions");
    file = await (await regions.getFileHandle(o.region)).getFile();
  }
  input = { file };
  return JSON.parse(planModule.plan(file?.size ?? 0, o.kind, o.level, o.boundary, o.areas, o.max, o.limit));
}

// o: {file: File} or {region: name of a file in the OPFS folder "regions"}, and zoom, boundary (GeoJSON text or null),
// border (m), complete (types), threads; or o.plan: the plan for the map
onmessage = async ({ data: o }) => {
  if (o.plan) {
    try { postMessage({ type: "plan", id: o.id, plan: await plan(o) }); }
    catch (e) { postMessage({ type: "plan", id: o.id, error: String(e?.message ?? e) }); }
    return;
  }
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
    // a folder of its own for each cut (o.dir): a stopped cut can hold its files for a moment, and the folders of old
    // cuts go when they can (the page removes them too)
    for await (const name of root.keys()) if (name.startsWith("cut-") && name !== o.dir) await root.removeEntry(name, { recursive: true }).catch(() => {});
    const dir = await root.getDirectoryHandle(o.dir, { create: true });
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
    postMessage({ type: "stage", name: "cut", t: performance.now(), read: readMs, store: storeMs });
    const report = JSON.parse(m.cut(size, o.kind, o.level, o.boundary, o.border, o.complete, o.memory || 0, o.areas, o.compress));
    for (const t of [...held.keys()]) flushTile(t);
    timed(addStore, () => { out.flush(); out.close(); });
    for (const h of pool) h.close();
    input.handle?.close();
    await dir.removeEntry("temp", { recursive: true });
    postMessage({ type: "done", report, parts: [...parts], bytes: outEnd, memory: memory.buffer.byteLength, read: readMs, store: storeMs });
  } catch (e) {
    postMessage({ type: "error", message: String(e?.message ?? e) });
  }
};
