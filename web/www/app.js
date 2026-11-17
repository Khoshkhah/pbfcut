// The page: it chooses the input and the settings, runs the cut in a worker (worker.js), shows the progress, and saves
// the tile files. The tile files stay in one OPFS file ("cut/tiles"); `parts` says where each file is in it.

import { zipBlob } from "./zip.js?v=dev";

const $ = (id) => document.getElementById(id);
// the steps of each kind of cut, in their order: the library names each step at its end
const STEPS = {
  tiles: [
    ["1", "Read 1: the nodes inside"], ["2", "Read 1: the ways"],
    ["3a", "the members of the relations"], ["3b", "the members in the file"], ["3c", "the tiles of the relations"],
    ["3d", "the members of the multipolygons"], ["4", "the nodes from outside"],
    ["5a", "Read 2: the nodes to the tiles"], ["5d", "Read 2: the ways to the tiles"], ["5e", "Read 2: the relations, and the tile files"],
  ],
};
STEPS.h3 = STEPS.areas = STEPS.tiles;     // the same cut, with cells or areas in place of tiles
const kind = () => document.querySelector("input[name=kind]:checked").value;
for (const r of document.querySelectorAll("input[name=kind]")) r.addEventListener("change", () => {
  for (const e of document.querySelectorAll("[data-kind]")) e.hidden = !e.dataset.kind.split(" ").includes(kind());
  resetNote();
  ready();
});
$("boundary").addEventListener("change", () => { resetNote(); ready(); });
$("zoom").addEventListener("change", () => { resetNote(); ready(); });
$("res").addEventListener("change", () => { resetNote(); ready(); });
$("areas").addEventListener("change", ready);
// The relay: a Cloudflare Worker of the owner that sends Geofabrik's files with CORS headers (Geofabrik sends none).
// It accepts only files of download.geofabrik.de and only some origins. ?relay=<address> gives another relay
// (serve.py has one: ?relay=relay).
const relay = new URLSearchParams(location.search).get("relay") ?? "https://duckosm-relay.khoshkhah.workers.dev";
const mib = (b) => b >= 1 << 30 ? `${(b / 2 ** 30).toFixed(2)} GiB` : `${(b / 2 ** 20).toFixed(1)} MiB`;
const regionsDir = () => navigator.storage.getDirectory().then((d) => d.getDirectoryHandle("regions", { create: true }));
const fileName = (id) => id.replace(/[^a-z0-9_-]/gi, "_") + ".osm.pbf";

let regions = [], region = null, regionReady = false, worker = null, timer = null;

// ---- the storage of the site holds the files of this page only while the page is open. A browser does not wait for
// work that is not synchronous while a page closes, so the page removes all at close as far as it can, and at the next
// load it removes what stays. Each open page holds the lock "pbfcut" (shared): a page removes the files only when no
// other page of the site is open (an exclusive lock), so a second tab does not remove the files of a cut.
const removeAll = async () => {
  const root = await navigator.storage.getDirectory();
  for await (const name of root.keys()) await root.removeEntry(name, { recursive: true }).catch(() => {});
};
const cleaned = navigator.locks.request("pbfcut", { ifAvailable: true }, (lock) => lock && removeAll())
  .then(() => { navigator.locks.request("pbfcut", { mode: "shared" }, () => new Promise(() => {})); });
addEventListener("pagehide", () => {
  worker?.terminate();
  removeAll();
});

// ---- light or dark: the choice of the person, else the setting of the system
$("theme").addEventListener("click", () => {
  const dark = document.documentElement.dataset.theme
    ? document.documentElement.dataset.theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.dataset.theme = dark ? "light" : "dark";
  try { localStorage.setItem("theme", document.documentElement.dataset.theme); } catch {}
  // a new style with no diff: "style.load" comes, and the layers of the plan come again with the colours of the theme
  styleReady = false;
  map?.setStyle(style(), { diff: false });
});

// ---- the zoom list: each zoom with the size of its tiles (a square) at the equator
// all zooms to 18: the limit of 20,000 files turns off the zooms that are too fine for the area (see the plan)
for (let z = 0; z <= 18; z++) {
  const km = 40075 / 2 ** z, w = km >= 10 ? Math.round(km).toLocaleString("en") + " km" : km >= 1 ? km.toFixed(1) + " km" : Math.round(km * 1000) + " m";
  // a figure space before a zoom of one digit, and two em spaces after it: the numbers line up (option text has no CSS)
  $("zoom").append(new Option(`${String(z).padStart(2, "\u2007")}\u2003\u2003${w} × ${w}`, z, false, z === 8));
}

// ---- the H3 resolutions: each with the mean area of its cells (hexagons), as in the H3 library
// all resolutions 0 to 15 (the mean areas of the H3 library); the limit of 20,000 files turns off the fine ones
const H3_KM2 = [4357449, 609788, 86802, 12393, 1770, 252.9, 36.13, 5.161, 0.7373, 0.1053, 0.01505, 0.00215, 0.000307,
  0.0000439, 0.00000627, 0.000000895];
for (let r = 0; r <= 15; r++) {
  const a = H3_KM2[r], area = a >= 100 ? Math.round(a).toLocaleString("en") + " km²" : a >= 1 ? a.toFixed(1) + " km²"
    : a >= 0.01 ? a.toPrecision(2) + " km²" : (a * 1e6 >= 10 ? Math.round(a * 1e6).toLocaleString("en") : (a * 1e6).toFixed(1)) + " m²";
  $("res").append(new Option(`${String(r).padStart(2, "\u2007")}\u2003\u2003${area}`, r, false, r === 7));
}

// ---- the help popups (the buttons "?")
// each popup: a title and its items [term, text]
const HELP = {
  input: ["Input", [
    ["Geofabrik region", "Geofabrik (download.geofabrik.de) is a website that gives free OpenStreetMap files of the countries and regions of the world. Search a region, then click Get or Download."],
    ["Get", "Puts the file into the storage of this site, through a relay (Geofabrik does not let web pages read its files)."],
    ["Download", "Saves the file on your computer, at the place that you choose, as fast as Get (Chrome and Edge). Other browsers download it directly from Geofabrik, which is slower. Then select it as a local file."],
    ["Local file", "A .osm.pbf file on this computer. The page does not upload it. Its blocks can be zlib blocks (as most files) or raw blocks; LZMA, LZ4 and Zstandard blocks stop the cut with an error."],
  ]],
  settings: ["Settings", [
    ["Tiles", "The square tiles of web maps. One file for each tile: z_x_y.osm.pbf."],
    ["H3 cells", "The hexagons of the H3 grid. One file for each cell, named with its H3 index."],
    ["Areas", "No grid: one file for each feature of a GeoJSON FeatureCollection, named by its property \"name\". The areas must not overlap. A file with one Polygon gives one area, named after the file."],
    ["Zoom", "The size of a tile, at the equator. At latitude 60° a tile is half as wide and half as high."],
    ["Resolution", "The mean area of an H3 cell."],
    ["Border", "Each file also gets the data this many metres around its tile or cell."],
    ["Threads", "The number of threads of the cut. Default: the number of cores."],
    ["zlib compression", "On (default): normal .osm.pbf files with zlib blocks, best to keep or to save. Off: raw blocks; the cut is about 20 % faster and the files are about 1.6 times larger (Sweden, zoom 8: 16.9 s and 1.5 GiB, against 20.8 s and 0.9 GiB). Each OSM program reads both."],
    ["Boundary", "Optional: a GeoJSON, .poly (osmosis) or WKT file; the cut keeps only the data inside it. Without it, a Geofabrik region (also a local file with the name of a Geofabrik file) uses the outline of the region; another file uses its bounding box."],
    ["GeoJSON", "In longitude and latitude (WGS 84): a Polygon, a MultiPolygon, a Feature or a FeatureCollection."],
  ]],
  progress: ["Progress", [
    ["Memory", "The WebAssembly memory of the cut."],
    ["Storage", "The files of this site: the input from Get, the temporary files and the result."],
    ["Steps", "Each step gets its time when it ends. Move the pointer on a step to see its details."],
  ]],
  files: ["Files", [
    ["Save all to a folder", "Writes all files into a folder that you select (Chrome and Edge on a computer)."],
    ["Download all as .zip", "One .zip file with all files, in each browser. The files are stored as they are, with no more compression: their blocks are compressed already."],
    ["A file name", "Downloads that file."],
    ["Refresh or close", "The page removes all its files. Save them first."],
  ]],
};
for (const b of document.querySelectorAll("button.help")) b.addEventListener("click", (e) => {
  e.preventDefault();     // a "?" in a label does not select its field
  const [title, items] = HELP[b.dataset.help];
  $("help-title").textContent = title;
  $("help-text").replaceChildren(...items.flatMap(([term, text]) => [
    Object.assign(document.createElement("dt"), { textContent: term }),
    Object.assign(document.createElement("dd"), { textContent: text }),
  ]));
  $("help-box").showModal();
});

// ---- 1. the input
const source = () => document.querySelector("input[name=src]:checked").value;
for (const r of document.querySelectorAll("input[name=src]")) r.addEventListener("change", () => {
  $("src-region").hidden = source() !== "region";
  $("src-file").hidden = source() !== "file";
  ready();
});
fetch("regions.json?v=dev").then((r) => r.json()).then((list) => {
  const byId = new Map(list.map((x) => [x[0], x]));
  const path = (x) => (x[2] && byId.has(x[2]) ? path(byId.get(x[2])) + " / " : "") + x[1];
  regions = list.map(([id, name, parent, pbf]) => {
    const label = path([id, name, parent]);
    return { id, name, label, parent: label.slice(0, -name.length - 3), pbf, key: label.toLowerCase() };
  }).sort((a, b) => a.label.localeCompare(b.label));
  document.documentElement.dataset.regions = regions.length;     // for the test
  if ($("region").value) findRegions();
});
// the region search: each word must be in the path of the region ("europe / estonia"); a name that starts with the
// first word comes first, then a name that has it, then a path that has it; shorter paths first. One match: it is chosen at once. More: Enter or Tab takes the active
// one (the first), the arrows move, Esc closes the list, a click or a tap takes one.
const MAX_MATCHES = 8;
// 0: the name starts with the first word; 1: the name has it; 2: only the path has it
const rank = (x, words) => { const n = x.name.toLowerCase(); return n.startsWith(words[0]) ? 0 : n.includes(words[0]) ? 1 : 2; };
let matches = [], active = 0;
function findRegions() {
  const q = $("region").value.trim().toLowerCase();
  const exact = regions.find((x) => x.key === q);
  if (exact) return pickRegion(exact);
  const words = q.split(/[\s/,]+/).filter(Boolean);
  matches = !words.length ? [] : regions.filter((x) => words.every((w) => x.key.includes(w)))
    .sort((a, b) => rank(a, words) - rank(b, words) || a.label.length - b.label.length);
  active = 0;
  // one match: the region is chosen at once, but the text stays as typed (the next letters go on after it); the field
  // gets the full name at Enter, Tab, a click, or when it loses the focus
  if (matches.length === 1) {
    if (region !== matches[0]) setRegion(matches[0]);
  } else if (region) setRegion(null);
  showMatches(q !== "");
}
function showMatches(open) {
  const ul = $("region-list");
  ul.replaceChildren(...(matches.length ? matches.slice(0, MAX_MATCHES).map((x, i) => {
    const li = Object.assign(document.createElement("li"), { role: "option", id: `region-${i}` });
    li.setAttribute("aria-selected", i === active);
    li.append(x.name, Object.assign(document.createElement("span"), { textContent: x.parent }));
    li.addEventListener("mousedown", (e) => { e.preventDefault(); pickRegion(x); });   // before the field loses the focus
    return li;
  }) : [Object.assign(document.createElement("li"), { className: "none", textContent: "No region has these words." })]));
  ul.hidden = !open;
  $("region").setAttribute("aria-expanded", String(open));
  $("region").setAttribute("aria-activedescendant", open && matches.length ? `region-${active}` : "");
}
function pickRegion(x) {
  $("region").value = x.label;
  matches = [];
  showMatches(false);
  if (region !== x) setRegion(x);
}
$("region").addEventListener("input", findRegions);
$("region").addEventListener("keydown", (e) => {
  const open = !$("region-list").hidden && matches.length > 0;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (!matches.length) return;
    e.preventDefault();
    active = (active + (e.key === "ArrowDown" ? 1 : -1) + Math.min(matches.length, MAX_MATCHES)) % Math.min(matches.length, MAX_MATCHES);
    showMatches(true);
  } else if ((e.key === "Enter" || e.key === "Tab") && open) {
    if (e.key === "Enter") e.preventDefault();      // Tab takes the match and moves on, as usual
    pickRegion(matches[active]);
  } else if (e.key === "Escape") showMatches(false);
});
$("region").addEventListener("blur", () => {
  if (region) $("region").value = region.label;
  showMatches(false);
});
// the region of the input: the buttons Get and Download, and whether its file is in the storage of this site
async function setRegion(x) {
  resetNote();
  region = x;
  regionReady = false;
  $("region-actions").hidden = !region;
  if (region) {
    await cleaned;
    $("link").href = region.pbf;
    $("link").title = region.pbf.split("/").pop();
    // Geofabrik sends no CORS headers: a web page can read its files only through a relay
    $("get").disabled = !relay;
    $("get").title = relay ? "Put the file into the storage of this site" : "Not available: Geofabrik sends no CORS headers";
    $("get-text").textContent = "";
    const kept = await (await regionsDir()).getFileHandle(fileName(region.id)).then((h) => h.getFile(), () => null);
    if (kept && region === x) {
      regionReady = true;
      $("get-text").textContent = `The file is in the storage of this site (${mib(kept.size)}).`;
      $("get").textContent = "Get it again";
    } else $("get").textContent = "Get";
  }
  ready();
}
// the download through the relay into OPFS. One connection to Geofabrik is slow (Luxembourg, 48 MB: 30 s), so the file
// comes in 8 parts at the same time with `Range`, each part at its place in the file (11 s). "<region>-latest.osm.pbf" is a
// redirect that Geofabrik answers after about 30 s, each time. A dated file that is there answers in 0.5 s. So the page asks
// for the files of the last 5 days (UTC) at the same time, waits 1.5 s, and takes the newest that has answered: it is
// what "latest" is. If none has answered, the request for "latest" is the fallback. All parts are of the dated file
// (`Content-Location`), and each answer must have the same ETag. A part that fails starts again at the byte where it
// stopped. A server that gives no ranges gets the old single stream. A written file is complete (createWritable
// writes to a temporary file and replaces the file at close).
// The bytes go to the file in blocks of BLOCK: a write for each piece of the network stream (about 64 KB) made the write
// the slower half of a Get (Estonia from the cache of the relay, one connection: 3.5 s with pieces, 1.8-2.5 s with blocks).
const PARTS = 8, TRIES = 4, MIN_PART = 4 << 20, BLOCK = 8 << 20;
// the pieces of a stream, collected until `BLOCK` bytes; `put(bytes)` writes them
function blocks(put) {
  let buf = [], held = 0;
  const flush = async () => {
    if (!held) return 0;
    const b = new Uint8Array(held);
    let o = 0;
    for (const c of buf) { b.set(c, o); o += c.length; }
    const n = held;
    buf = []; held = 0;
    await put(b);
    return n;
  };
  return { add: async (v) => { buf.push(v); held += v.length; return held >= BLOCK ? flush() : 0; }, flush };
}
async function download(x, onProgress, signal, open = async () => (await (await regionsDir()).getFileHandle(fileName(x.id), { create: true })).createWritable()) {
  const ctl = new AbortController();     // the signal of the caller, and the stop of all parts when one part fails
  signal?.addEventListener("abort", () => ctl.abort());
  const get = (u, init) => fetch(`${relay}?url=${encodeURIComponent(u)}`, { signal: ctl.signal, ...init });
  const bad = async (r) => new Error(`the relay answered ${r.status}: ${(await r.text()).trim()}`);
  const dated = (back) => x.pbf.replace("-latest.", `-${new Date(Date.now() - back * 864e5).toISOString().slice(2, 10).replace(/-/g, "")}.`);
  const tries = [0, 1, 2, 3, 4].map((back) => {
    const t = { stop: new AbortController(), head: null };
    t.done = get(dated(back), { method: "HEAD", signal: t.stop.signal }).then((h) => (t.head = h), () => null);
    return t;
  });
  await Promise.race([Promise.all(tries.map((t) => t.done)), new Promise((ok) => setTimeout(ok, 1500))]);
  for (const t of tries) t.stop.abort();
  const head = tries.find((t) => t.head?.ok)?.head ?? await get(x.pbf, { method: "HEAD" });
  if (!head.ok) throw await bad(head);
  const total = Number(head.headers.get("Content-Length")) || 0;
  const url = head.headers.get("Content-Location");           // the dated file: all parts are of one version
  const etag = head.headers.get("ETag");
  // a file in the cache of the relay comes faster in one connection (Estonia, 119 MiB: 2 s, against 4.4 s in 8 parts); a
  // request for its first byte tells (X-Relay-Cache: HIT ...), and it also starts the fill of the cache when the file is not there
  let cached = false;
  if (url) {
    const probe = await get(url, { headers: { Range: "bytes=0-0" } }).catch(() => null);
    cached = /^HIT/.test(probe?.headers.get("X-Relay-Cache") ?? "");
    await probe?.body?.cancel().catch(() => {});
  }
  const parts = !cached && head.headers.get("Accept-Ranges") === "bytes" && url ? Math.min(PARTS, Math.floor(total / MIN_PART)) : 0;
  const out = await open();
  let got = 0;
  const part = async (i) => {
    const size = Math.ceil(total / parts), end = Math.min(total, (i + 1) * size) - 1;
    let pos = i * size;
    for (let t = 1; pos <= end; t++) {
      try {
        const r = await get(url, { headers: { Range: `bytes=${pos}-${end}` } });
        if (r.status !== 206) {
          const e = await bad(r);
          e.fatal = r.status < 500;      // 200 (no range) or a 4xx answer: a new try gives the same answer
          throw e;
        }
        if (etag && r.headers.get("ETag") !== etag) throw new Error("the file changed during the download");
        const reader = r.body.getReader();
        // `pos` is the next byte that is written, and `got` counts the written bytes: a part that starts again starts at
        // `pos` (the bytes of a block that was not written come again)
        const block = blocks(async (b) => {
          await out.write({ type: "write", position: pos, data: b });
          pos += b.length;
          got += b.length;
          onProgress(got, total);
        });
        try {
          for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            await block.add(value);
          }
        } finally {
          await block.flush();            // the bytes that came before a failure are good: they are written
        }
        if (pos <= end) throw new Error("a part ended early");
      } catch (e) {
        if (ctl.signal.aborted || e.fatal || t >= TRIES) throw e;
        await new Promise((ok) => setTimeout(ok, 500 * t));       // the next try starts at pos: the bytes written stay
      }
    }
  };
  try {
    if (parts) await Promise.all(Array.from({ length: parts }, (_, i) => part(i).catch((e) => { ctl.abort(); throw e; })));
    else {
      const r = await get(url || x.pbf);
      if (!r.ok) throw await bad(r);
      const reader = r.body.getReader();
      const block = blocks(async (b) => {
        await out.write(b);
        got += b.length;
        onProgress(got, total);
      });
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        await block.add(value);
      }
      await block.flush();
    }
    await out.close();
  } catch (e) {
    await out.abort().catch(() => {});
    throw e;
  }
  if (total && got !== total) throw new Error(`the download stopped after ${got} of ${total} bytes`);
  return got;
}
$("get").addEventListener("click", async () => {
  const x = region;
  $("get").disabled = true;
  await cleaned;
  $("get-bar").hidden = false;
  const start = performance.now();
  try {
    const got = await download(x, (n, total) => {
      $("get-bar").value = total ? n / total : 0;
      $("get-text").textContent = `${mib(n)}${total ? " of " + mib(total) : ""}, ${((performance.now() - start) / 1000).toFixed(1)} s`;
    });
    regionReady = region === x;
    $("get-text").textContent = `The file is in the storage of this site (${mib(got)}, ${((performance.now() - start) / 1000).toFixed(1)} s).`;
  } catch (e) {
    $("get-text").textContent = `The download stopped: ${e.message}`;
  }
  $("get").disabled = false;
  ready();
});
// Download: the file to the computer. With a save dialog (Chrome, Edge), the page writes the file at the place that the
// person picks, the same way as Get (through the relay, from its cache, or in parts): not into the storage of this site.
// Other browsers, or no relay: the plain link to Geofabrik (one connection, slow).
$("link").addEventListener("click", async (e) => {
  const x = region;
  if (!x || !relay || !window.showSaveFilePicker) return;
  e.preventDefault();
  let file;
  try { file = await showSaveFilePicker({ suggestedName: x.pbf.split("/").pop(), types: [{ description: "OSM PBF", accept: { "application/octet-stream": [".pbf"] } }] }); }
  catch { return; }                              // the dialog was closed
  $("link").classList.add("disabled");
  $("get-bar").hidden = false;
  const start = performance.now();
  try {
    const got = await download(x, (n, total) => {
      $("get-bar").value = total ? n / total : 0;
      $("get-text").textContent = `Download: ${mib(n)}${total ? " of " + mib(total) : ""}, ${((performance.now() - start) / 1000).toFixed(1)} s`;
    }, null, () => file.createWritable());
    $("get-text").textContent = `The file is saved as ${file.name} (${mib(got)}, ${((performance.now() - start) / 1000).toFixed(1)} s).`;
  } catch (e) {
    $("get-text").textContent = `The download stopped: ${e.message}`;
  }
  $("get-bar").hidden = true;
  $("link").classList.remove("disabled");
});
$("file").addEventListener("change", () => { resetNote(); ready(); });

// ---- 2. the settings
const isolated = self.crossOriginIsolated;
$("threads").value = isolated ? navigator.hardwareConcurrency || 4 : 1;
$("threads").disabled = !isolated;
$("threads-hint").textContent = isolated ? "" : "This page is not cross-origin isolated, so the cut uses one thread.";

function ready() {
  updateStart();
  askPlan();
}

// ---- the map before the cut: the outlines of the tiles, cells or areas that the cut uses (the same lists, from the
// library, in a worker of its own), on a map of OpenFreeMap. It shows from the start (the world), the count, the levels
// that the page can cut, and an error in the boundary or in the areas before the cut.
const MAX_FILES = 20000;      // the most tiles or cells of one cut on this page: a higher zoom or resolution is off
let planWorker = null, planId = 0, planTimer = null, map = null, lastPlan = null, tooMany = false;
// the style of the map can take layers (MapLibre's isStyleLoaded() stays false while base-map tiles load)
let styleReady = false, planWhere = "";
// the note when the page lowered the zoom or the resolution to the highest one that it can cut: it stays until the person
// changes the input, the boundary, the kind or the level
let lowered = "";
const resetNote = () => { lowered = ""; };
const dark = () => document.documentElement.dataset.theme
  ? document.documentElement.dataset.theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
const style = () => `https://tiles.openfreemap.org/styles/${dark() ? "fiord" : "positron"}`;
// the colours of the plan: the tiles or cells (blue), the boundary (rose, dashed); lighter on the dark map
const COLORS = { light: { line: "#1d4ed8", fill: "#3b82f6", fillOpacity: 0.14, area: "#e11d48" },
                 dark: { line: "#93c5fd", fill: "#60a5fa", fillOpacity: 0.16, area: "#fb7185" } };
const updateStart = () => {
  const ok = (source() === "file" ? $("file").files.length > 0 : regionReady)
    && (kind() !== "areas" || $("areas").files.length > 0);     // the areas need their file
  $("start").disabled = !ok || !!worker || tooMany;
};
// the levels above the highest one with at most MAX_FILES tiles or cells are off (null: all levels are on)
const levels = (top) => {
  for (const id of ["zoom", "res"]) for (const o of $(id).options) o.disabled = top !== null && id === (kind() === "h3" ? "res" : "zoom") && Number(o.value) > top;
};
// The outline of a Geofabrik region (the same polygon as its .poly file, from web/www/regions/): the boundary of a cut of
// that region when no boundary file is given. The region of the input: the chosen region, or a local file with the name
// of a Geofabrik file (<name>-latest.osm.pbf or <name>-<yymmdd>.osm.pbf) that one region has. Gives {name, text} or null.
const outlineCache = new Map();
async function regionOutline() {
  let r = source() === "region" ? region : null;
  const f = source() === "file" ? $("file").files[0] : null;
  const m = f?.name.match(/^(.+?)-(latest|\d{6})\.osm\.pbf$/);
  if (m) {
    const hits = regions.filter((x) => x.pbf.split("/").pop() === `${m[1]}-latest.osm.pbf`);
    if (hits.length === 1) r = hits[0];
  }
  if (!r) return null;
  if (!outlineCache.has(r.id)) {
    outlineCache.set(r.id, fetch(`regions/${r.id.replace(/[^a-z0-9_-]/gi, "_")}.json?v=dev`).then((x) => (x.ok ? x.text() : null), () => null));
  }
  const text = await outlineCache.get(r.id);
  return text ? { name: r.label.split(" / ").pop(), text } : null;
}
// the boundary of a cut: the boundary file, else the outline of the region, else none (the box of the file)
async function boundaryOf() {
  const f = $("boundary").files[0];
  if (f) return { text: await f.text(), where: "the boundary" };
  const o = await regionOutline();
  return o ? { text: o.text, where: `the boundary of ${o.name}` } : null;
}
function askPlan() {
  clearTimeout(planTimer);
  planTimer = setTimeout(async () => {
    const id = ++planId;
    const input = source() === "file" ? $("file").files[0] : regionReady ? region : null;
    const boundary = await boundaryOf();
    if (kind() === "areas" ? !$("areas").files.length : !(input || boundary)) {
      showPlan({ hint: kind() === "areas" ? "Choose the file of the areas: the map then shows them."
        : "Choose a region, a file or a boundary: the map then shows the tiles or cells of the cut." });
      return;
    }
    planWhere = boundary?.where ?? "the box of the file";
    const msg = {
      plan: true, id, kind: kind(), level: Number($(kind() === "h3" ? "res" : "zoom").value), max: MAX_FILES, limit: MAX_FILES,
      boundary: boundary?.text ?? null,
      areas: kind() === "areas" ? await areasText($("areas").files[0]) : null,
      ...(!input ? {} : source() === "file" ? { file: input } : { region: fileName(input.id) }),
    };
    planWorker ??= new Worker("worker.js?v=dev", { type: "module" });
    planWorker.onmessage = ({ data: m }) => { if (m.type === "plan" && m.id === planId) showPlan(m); };
    planWorker.postMessage(msg);
  }, 250);
}
function showPlan({ plan, error, hint }) {
  const text = $("plan-text");
  text.classList.toggle("bad", !!error);
  tooMany = false;
  if (hint || error) {
    text.textContent = error ? `This cut cannot start: ${error}` : hint;
    levels(null);
    lastPlan = null;
  } else {
    const word = { tiles: "tile", h3: "cell", areas: "area" }[kind()];
    const s = plan.count === 1 ? "" : "s";
    const where = kind() === "areas" ? "" : `${plan.count === 1 ? " that touches" : " that touch"} ${planWhere}`;
    levels(plan.max_level);
    // above the highest level that the page can cut: the page takes that level, and says so
    const sel = $(kind() === "h3" ? "res" : "zoom"), name = kind() === "h3" ? "resolution" : "zoom";
    if (kind() !== "areas" && plan.max_level !== null && Number(sel.value) > plan.max_level
        && [...sel.options].some((o) => Number(o.value) === plan.max_level)) {
      lowered = `The ${name} ${sel.value} gives more than ${MAX_FILES.toLocaleString("en")} ${word}s here, so the page chose ${plan.max_level}. `;
      sel.value = String(plan.max_level);
      askPlan();
      return;
    }
    tooMany = kind() !== "areas" && plan.count > MAX_FILES;
    text.classList.toggle("bad", tooMany);
    text.textContent = tooMany
      ? `More than ${MAX_FILES.toLocaleString("en")} ${word}s: this page cuts at most ${MAX_FILES.toLocaleString("en")}. Choose a lower ${kind() === "h3" ? "resolution" : "zoom"} or a smaller boundary.`
      : lowered + `${plan.count.toLocaleString("en")} ${word}${s}${where}.`
        + (kind() === "areas" ? "" : ` Only the ${word}s with data get a file.`);
    // too many: only the boundary (a part of the outlines looks like a cut of a part of the area)
    lastPlan = tooMany ? { ...plan, outlines: EMPTY } : plan;
    // the map zoom at which one tile or cell is a few pixels wide: its outlines fade in from there (an H3 cell of
    // resolution r is about as large as a tile of zoom 2r - 1)
    lastPlan.level = kind() === "areas" ? 0 : kind() === "h3" ? 2 * Number($("res").value) - 1 : Number($("zoom").value);
  }
  updateStart();
  drawPlan();
}
async function drawPlan() {
  if (!window.maplibregl) {
    document.head.append(Object.assign(document.createElement("link"), { rel: "stylesheet", href: "https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.css" }));
    await new Promise((ok, bad) => document.head.append(Object.assign(document.createElement("script"),
      { src: "https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.js", onload: ok, onerror: bad })));
  }
  if (!map) {
    map = new maplibregl.Map({ container: "map", style: style(), center: [10, 30], zoom: 0.6, attributionControl: { compact: true }, dragRotate: false });
    self.planMap = map;                 // for the test
    map.touchZoomRotate.disableRotation();
    map.on("style.load", () => { styleReady = true; addLayers(); });
    // the attribution stays behind its (i) button: open, it covers a part of a small map
    map.once("load", () => document.querySelector("#map .maplibregl-ctrl-attrib")?.classList.remove("maplibregl-compact-show"));
  } else if (styleReady) addLayers();
  if (!lastPlan) return;
  // the view: the boundary and all outlines that the map shows
  const b = new maplibregl.LngLatBounds();
  for (const f of [lastPlan.area, ...lastPlan.outlines.features].filter(Boolean)) for (const poly of f.geometry.coordinates) for (const p of poly[0]) b.extend(p);
  if (!b.isEmpty()) map.fitBounds(b, { padding: 24, duration: 0, maxZoom: 15 });
}
const EMPTY = { type: "FeatureCollection", features: [] };
function addLayers() {
  const c = COLORS[dark() ? "dark" : "light"];
  for (const [id, data] of [["outlines", lastPlan?.outlines ?? EMPTY], ["area", lastPlan?.area ?? EMPTY]]) {
    if (map.getSource(id)) map.getSource(id).setData(data);
    else map.addSource(id, { type: "geojson", data });
  }
  const layer = (spec, paint) => {
    if (!map.getLayer(spec.id)) map.addLayer({ ...spec, paint });
    else for (const [k, v] of Object.entries(paint)) map.setPaintProperty(spec.id, k, v);
  };
  layer({ id: "outlines-fill", type: "fill", source: "outlines" }, { "fill-color": c.fill, "fill-opacity": c.fillOpacity });
  // the outlines show when a tile or cell is at least about 6 pixels wide (MapLibre draws 512-pixel tiles: a tile of
  // zoom z is 512 * 2^(m - z) pixels wide at map zoom m); smaller, the fill alone shows the area
  const at = Math.max(0, (lastPlan?.level ?? 0) - 6.5);
  layer({ id: "outlines-line", type: "line", source: "outlines" }, { "line-color": c.line, "line-width": 1.2,
    "line-opacity": ["interpolate", ["linear"], ["zoom"], at, 0, at + 1.5, 1] });
  layer({ id: "area-line", type: "line", source: "area" }, { "line-color": c.area, "line-width": 2.5, "line-dasharray": [2, 1.2] });
}
askPlan();     // the map from the start: the world, and what to choose

// the text of the areas file for the cut: a FeatureCollection or an osmium config as it is; one Feature, Polygon or
// MultiPolygon becomes a FeatureCollection with one area, named after the file
async function areasText(file) {
  const text = await file.text();
  let v;
  try { v = JSON.parse(text); } catch { return text; }     // the cut gives the error of the JSON
  if (v.type === "FeatureCollection" || v.extracts) return text;
  const name = file.name.replace(/\.(geo)?json$/i, "");
  const f = v.type === "Feature" ? v : { type: "Feature", properties: {}, geometry: v };
  f.properties = { name, ...f.properties };
  return JSON.stringify({ type: "FeatureCollection", features: [f] });
}

// ---- 3. the cut and its progress
$("start").addEventListener("click", async () => {
  // the result and the message of the cut before go at once (the lines below wait)
  $("result").hidden = true;
  $("error").hidden = true;
  await cleaned;
  const boundary = (await boundaryOf())?.text ?? null;      // the boundary file, else the outline of the region
  const steps = STEPS[kind()];
  const msg = {
    kind: kind(), areas: kind() === "areas" ? await areasText($("areas").files[0]) : null,
    level: Number($(kind() === "h3" ? "res" : "zoom").value), border: Number($("border").value) || 0, threads: Number($("threads").value) || 1,
    boundary, complete: "multipolygon,boundary", compress: $("compress").checked,
    ...(source() === "file" ? { file: $("file").files[0] } : { region: fileName(region.id) }),
  };
  $("progress").hidden = false;
  $("bar").style.width = "0";
  $("result").hidden = true;
  $("error").hidden = true;
  $("steps").replaceChildren(...steps.map(([id, label]) => {
    const li = document.createElement("li");
    li.dataset.step = id;
    li.innerHTML = `<span>${id}</span><b></b><span class="t"></span>`;
    li.querySelector("b").textContent = label;
    return li;
  }));
  const t0 = performance.now();
  let shared = null, lastT = null, panic = null;
  let stepStart = performance.now();     // the running step: its time counts up in the list (a long step is not frozen)
  let top = 0;     // the most WebAssembly memory of this cut
  const show = (bytes) => { top = Math.max(top, bytes); $("memory").textContent = mib(bytes); };
  timer = setInterval(() => {
    $("time").textContent = `${((performance.now() - t0) / 1000).toFixed(1)} s`;
    if (shared) show(shared.buffer.byteLength);
    const run = $("steps").querySelector("li.run .t");
    if (run) run.textContent = `${((performance.now() - stepStart) / 1000).toFixed(0)} s`;
  }, 200);
  const used = async () => {
    const e = await navigator.storage.estimate();
    $("storage").textContent = mib(e.usage);
    $("storage").title = `${mib(e.usage)} of ${mib(e.quota)}`;
  };
  used();
  const storage = setInterval(used, 1000);
  const end = () => { clearInterval(timer); clearInterval(storage); used(); worker?.terminate(); worker = null; $("stop").disabled = true; ready(); };
  // a step message ends the first step that is not done
  const mark = () => {
    const li = $("steps").querySelector("li:not(.done)");
    for (const x of $("steps").children) x.classList.remove("run");
    li?.nextElementSibling?.classList.add("run");
    return li;
  };
  worker = new Worker("worker.js?v=dev", { type: "module" });
  worker.onmessage = ({ data: m }) => {
    if (m.type === "stage") {
      // the box "Step" shows the code of the step only (its name is in the list of steps); before the cut: "start"
      $("stage").textContent = "start";
      $("stage").title = m.name;
      if (m.name === "cut") { lastT = m.t; stepStart = performance.now(); $("steps").firstElementChild.classList.add("run"); $("stage").textContent = steps[0][0]; $("stage").title = steps[0][1]; }
    } else if (m.type === "memory") {
      shared = m.memory;
      show(m.bytes);
    } else if (m.type === "step") {
      if (m.memory) show(m.memory);
      if (m.name.startsWith("panic:")) { panic = m.name; return; }
      const text = m.name.replace(/^[^:]*: /, "");
      const li = mark();
      if (li) {
        li.classList.add("done");
        $("bar").style.width = `${(100 * $("steps").querySelectorAll(".done").length) / steps.length}%`;
        li.querySelector("b").textContent = text.replace(/ \(.*$/, "");
        li.title = text;
        li.querySelector(".t").textContent = `${((m.t - lastT) / 1000).toFixed(2)} s`;
        lastT = m.t;
        stepStart = performance.now();
        const next = steps[[...$("steps").children].indexOf(li) + 1];
        $("stage").textContent = next ? next[0] : "the end";
        $("stage").title = next ? next[1] : "";
      }
    } else if (m.type === "done") {
      for (const x of $("steps").children) x.classList.remove("run");
      $("stage").textContent = "done";
      show(m.memory);
      end();
      $("time").textContent = `${((performance.now() - t0) / 1000).toFixed(1)} s`;
      result(m);
    } else if (m.type === "error") {
      end();
      $("error").hidden = false;
      // a full WebAssembly memory (4 GiB) stops the module with "unreachable", with no message of its own
      const full = !panic && /unreachable/i.test(m.message) && top > 3.8 * 2 ** 30;
      $("error").textContent = full
        ? "The cut stopped: the WebAssembly memory is full (4 GiB). Use the command line for this file, or a boundary that is smaller."
        : `The cut stopped: ${panic ?? m.message}`;
    }
  };
  worker.postMessage(msg);
  $("start").disabled = true;
  $("stop").disabled = false;
  $("stop").onclick = async () => {
    end();
    $("stage").textContent = "stopped";
    // the files of the stopped cut go at once (the temporary files and the part of the result): the worker closes its
    // files when it stops, so the removal tries again for some seconds
    const root = await navigator.storage.getDirectory();
    for (let i = 0; i < 40; i++) {
      try { await root.removeEntry("cut", { recursive: true }); break; }
      catch (e) { if (e.name === "NotFoundError") break; await new Promise((ok) => setTimeout(ok, 250)); }
    }
    used();
  };
});

// ---- 4. the result: one OPFS file with all tile files; `files` of the report, with `parts` (tile number -> [[start, length]])
let saved = null;
async function result({ report, parts, bytes }) {
  const where = new Map(parts);
  const all = await (await (await (await navigator.storage.getDirectory()).getDirectoryHandle("cut")).getFileHandle("tiles")).getFile();
  const files = report.files.map((f) => ({ ...f, parts: where.get(f.index) ?? [] }));
  files.forEach((f) => { f.size = f.parts.reduce((s, p) => s + p[1], 0); });
  saved = { all, files };
  $("result").hidden = false;
  const n = (k, word) => `${k} ${word}${k === 1 ? "" : "s"}`;
  const [word, of] = { areas: ["area", ""], h3: ["cell", ` at resolution ${$("res").value}`], tiles: ["tile", ` at zoom ${$("zoom").value}`] }[kind()];
  $("summary").textContent = `${n(files.length, word + " file")} (${mib(bytes)}) of ${n(report.tiles, word)}${of}, in ${report.seconds.toFixed(1)} s.`;
  const MAX = 1000;
  $("files").replaceChildren(...files.slice(0, MAX).map((f) => {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td><a href="#"></a></td><td>${f.nodes}</td><td>${f.ways}</td><td>${f.relations}</td><td>${mib(f.size)}</td>`;
    const a = tr.querySelector("a");
    a.textContent = f.file;
    a.onclick = (e) => {
      e.preventDefault();
      const url = URL.createObjectURL(new Blob(f.parts.map(([s, n]) => all.slice(s, s + n))));
      Object.assign(document.createElement("a"), { href: url, download: f.file }).click();
      setTimeout(() => URL.revokeObjectURL(url), 10000);
    };
    return tr;
  }));
  if (files.length > MAX) $("summary").textContent += ` The table shows the first ${MAX}.`;
  // "Save all to a folder" needs showDirectoryPicker (Chrome and Edge on a computer); the .zip works in each browser
  const can = "showDirectoryPicker" in self;
  $("save").hidden = !can;
  $("zip").classList.toggle("secondary", can);
  $("save-text").textContent = "";
}
// one .zip of all files (stored: the blocks are compressed already), for each browser
$("zip").addEventListener("click", async () => {
  const { all, files } = saved;
  $("zip").disabled = true;
  const start = performance.now();
  try {
    const blob = await zipBlob(files.map((f) => ({ name: f.file, blob: new Blob(f.parts.map(([s, n]) => all.slice(s, s + n))) })),
      (i, n) => { $("save-text").textContent = `Making the .zip: ${i} of ${n} files`; }, self.zip64ForTest === true);
    const name = `pbfcut-${{ tiles: `zoom-${$("zoom").value}`, h3: `h3-${$("res").value}`, areas: "areas" }[kind()]}.zip`;
    const url = URL.createObjectURL(blob);
    Object.assign(document.createElement("a"), { href: url, download: name }).click();
    setTimeout(() => URL.revokeObjectURL(url), 60000);
    $("save-text").textContent = `${name}: ${files.length} files, ${mib(blob.size)} (${((performance.now() - start) / 1000).toFixed(1)} s).`;
  } catch (e) {
    $("save-text").textContent = `The .zip stopped: ${e.message}`;
  }
  $("zip").disabled = false;
});
$("save").addEventListener("click", async () => {
  let dir;
  try { dir = await showDirectoryPicker({ mode: "readwrite", id: "pbfcut" }); } catch { return; }
  $("save").disabled = true;
  const { all, files } = saved;
  try {
    for (const [i, f] of files.entries()) {
      const w = await (await dir.getFileHandle(f.file, { create: true })).createWritable();
      for (const [s, n] of f.parts) await w.write(all.slice(s, s + n));
      await w.close();
      $("save-text").textContent = `${i + 1} of ${files.length} files saved`;
    }
    $("save-text").textContent = `${files.length} files saved in the folder "${dir.name}".`;
  } catch (e) {
    $("save-text").textContent = `The save stopped: ${e.message}`;
  }
  $("save").disabled = false;
});
