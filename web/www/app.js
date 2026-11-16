// The page: it chooses the input and the settings, runs the cut in a worker (worker.js), shows the progress, and saves
// the tile files. The tile files stay in one OPFS file ("cut/tiles"); `parts` says where each file is in it.

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
  ready();
});
$("boundary").addEventListener("change", ready);
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
});

// ---- the zoom list: each zoom with the size of its tiles (a square) at the equator
for (let z = 2; z <= 16; z++) {
  const km = 40075 / 2 ** z, w = km >= 10 ? Math.round(km) : km.toFixed(1);
  // a figure space before a zoom of one digit, and two em spaces after it: the numbers line up (option text has no CSS)
  $("zoom").append(new Option(`${String(z).padStart(2, "\u2007")}\u2003\u2003${w} × ${w} km`, z, false, z === 8));
}

// ---- the H3 resolutions: each with the mean area of its cells (hexagons), as in the H3 library
const H3_KM2 = [4357449, 609788, 86802, 12393, 1770, 252.9, 36.13, 5.161, 0.7373, 0.1053, 0.01505, 0.00215, 0.000307];
for (let r = 2; r <= 12; r++) {
  const a = H3_KM2[r], area = a >= 100 ? Math.round(a).toLocaleString("en") : a >= 1 ? a.toFixed(1) : a.toPrecision(2);
  $("res").append(new Option(`${String(r).padStart(2, "\u2007")}\u2003\u2003${area} km²`, r, false, r === 7));
}

// ---- the help popups (the buttons "?")
// each popup: a title and its items [term, text]
const HELP = {
  input: ["Input", [
    ["Geofabrik region", "Geofabrik (download.geofabrik.de) is a website that gives free OpenStreetMap files of the countries and regions of the world. Search a region, then click Get or Download."],
    ["Get", "Puts the file into the storage of this site, through a relay (Geofabrik does not let web pages read its files)."],
    ["Download", "Saves the file on your computer. Then select it as a local file."],
    ["Local file", "A .osm.pbf file on this computer. The page does not upload it."],
  ]],
  settings: ["Settings", [
    ["Tiles", "The square tiles of web maps. One file for each tile: z_x_y.osm.pbf."],
    ["H3 cells", "The hexagons of the H3 grid. One file for each cell, named with its H3 index."],
    ["Areas", "No grid: one file for each feature of a GeoJSON FeatureCollection, named by its property \"name\". The areas must not overlap. A file with one Polygon gives one area, named after the file."],
    ["Zoom", "The size of a tile, at the equator. At latitude 60° a tile is half as wide and half as high."],
    ["Resolution", "The mean area of an H3 cell."],
    ["Border", "Each file also gets the data this many metres around its tile or cell."],
    ["Threads", "The number of threads of the cut. Default: the number of cores."],
    ["Boundary", "Optional: a GeoJSON, .poly (osmosis) or WKT file; the cut keeps only the data inside it. Without it, tiles and cells cover the bounding box of the file."],
    ["GeoJSON", "In longitude and latitude (WGS 84): a Polygon, a MultiPolygon, a Feature or a FeatureCollection."],
  ]],
  progress: ["Progress", [
    ["Memory", "The WebAssembly memory of the cut."],
    ["Storage", "The files of this site: the input from Get, the temporary files and the result."],
    ["Steps", "Each step gets its time when it ends. Move the pointer on a step to see its details."],
  ]],
  files: ["Files", [
    ["Save all to a folder", "Writes all files into a folder that you select (Chrome and Edge on a computer)."],
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
  regions = list.map(([id, name, parent, pbf]) => ({ id, label: path([id, name, parent]), pbf })).sort((a, b) => a.label.localeCompare(b.label));
  $("regions").replaceChildren(...regions.map((x) => Object.assign(document.createElement("option"), { value: x.label })));
});
$("region").addEventListener("input", async () => {
  region = regions.find((x) => x.label === $("region").value) ?? null;
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
    if (kept) {
      regionReady = true;
      $("get-text").textContent = `The file is in the storage of this site (${mib(kept.size)}).`;
      $("get").textContent = "Get it again";
    } else $("get").textContent = "Get";
  }
  ready();
});
// the download through the relay into OPFS. One connection to Geofabrik is slow (Luxembourg, 48 MB: 30 s), so the file
// comes in 8 parts at the same time with `Range`, each part at its place in the file (11 s). "<region>-latest.osm.pbf" is a
// redirect that Geofabrik answers after about 30 s, each time. A dated file that is there answers in 0.5 s. So the page asks
// for the files of the last 5 days (UTC) at the same time, waits 1.5 s, and takes the newest that has answered: it is
// what "latest" is. If none has answered, the request for "latest" is the fallback. All parts are of the dated file
// (`Content-Location`), and each answer must have the same ETag. A part that fails starts again at the byte where it
// stopped. A server that gives no ranges gets the old single stream. A written file is complete (createWritable
// writes to a temporary file and replaces the file at close).
const PARTS = 8, TRIES = 4, MIN_PART = 4 << 20;
async function download(x, onProgress, signal) {
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
  const parts = head.headers.get("Accept-Ranges") === "bytes" && url ? Math.min(PARTS, Math.floor(total / MIN_PART)) : 0;
  const out = await (await (await regionsDir()).getFileHandle(fileName(x.id), { create: true })).createWritable();
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
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          await out.write({ type: "write", position: pos, data: value });
          pos += value.length;
          got += value.length;
          onProgress(got, total);
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
      const r = await get(x.pbf);
      if (!r.ok) throw await bad(r);
      const reader = r.body.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        await out.write(value);
        got += value.length;
        onProgress(got, total);
      }
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
$("file").addEventListener("change", ready);

// ---- 2. the settings
const isolated = self.crossOriginIsolated;
$("threads").value = isolated ? navigator.hardwareConcurrency || 4 : 1;
$("threads").disabled = !isolated;
$("threads-hint").textContent = isolated ? "" : "This page is not cross-origin isolated, so the cut uses one thread.";

function ready() {
  const ok = (source() === "file" ? $("file").files.length > 0 : regionReady)
    && (kind() !== "areas" || $("areas").files.length > 0);     // the areas need their file
  $("start").disabled = !ok || !!worker;
}

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
  const boundary = $("boundary").files[0] ? await $("boundary").files[0].text() : null;
  const steps = STEPS[kind()];
  const msg = {
    kind: kind(), areas: kind() === "areas" ? await areasText($("areas").files[0]) : null,
    level: Number($(kind() === "h3" ? "res" : "zoom").value), border: Number($("border").value) || 0, threads: Number($("threads").value) || 1,
    boundary, complete: "multipolygon,boundary",
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
  let top = 0;     // the most WebAssembly memory of this cut
  const show = (bytes) => { top = Math.max(top, bytes); $("memory").textContent = mib(bytes); };
  timer = setInterval(() => {
    $("time").textContent = `${((performance.now() - t0) / 1000).toFixed(1)} s`;
    if (shared) show(shared.buffer.byteLength);
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
      $("stage").textContent = m.name;
      if (m.name === "cut") { lastT = m.t; $("steps").firstElementChild.classList.add("run"); $("stage").textContent = steps[0][1]; }
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
        const next = steps[[...$("steps").children].indexOf(li) + 1];
        $("stage").textContent = next ? next[1] : "the end";
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
  $("stop").onclick = () => {
    end();
    $("stage").textContent = "stopped";
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
  const can = "showDirectoryPicker" in self;
  $("save").disabled = !can;
  $("save-text").textContent = can ? "" : "Chrome and Edge only. Click a file to download it.";
}
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
