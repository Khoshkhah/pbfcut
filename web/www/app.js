// The page: it chooses the input and the settings, runs the cut in a worker (worker.js), shows the progress, and saves
// the tile files. The tile files stay in one OPFS file ("cut/tiles"); `parts` says where each file is in it.

const $ = (id) => document.getElementById(id);
const STEPS = [
  ["1", "Read 1: the nodes inside"], ["2", "Read 1: the ways"],
  ["3a", "the members of the relations"], ["3b", "the members in the file"], ["3c", "the tiles of the relations"],
  ["3d", "the members of the multipolygons"], ["4", "the nodes from outside"],
  ["5a", "Read 2: the nodes to the tiles"], ["5d", "Read 2: the ways to the tiles"], ["5e", "Read 2: the relations, and the tile files"],
];
// ?relay=<address>: a relay that sends Geofabrik's files with CORS headers (serve.py has one: ?relay=relay)
const relay = new URLSearchParams(location.search).get("relay");
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
// the download through the relay, as a stream into OPFS; a written file is complete (createWritable writes to a
// temporary file and replaces the file at close)
$("get").addEventListener("click", async () => {
  const x = region;
  $("get").disabled = true;
  await cleaned;
  $("get-bar").hidden = false;
  try {
    const r = await fetch(`${relay}?url=${encodeURIComponent(x.pbf)}`);
    if (!r.ok) throw new Error(`the relay answered ${r.status}`);
    const total = Number(r.headers.get("Content-Length")) || 0, start = performance.now();
    const out = await (await (await regionsDir()).getFileHandle(fileName(x.id), { create: true })).createWritable();
    let got = 0;
    const rd = r.body.getReader();
    for (;;) {
      const { done, value } = await rd.read();
      if (done) break;
      await out.write(value);
      got += value.length;
      $("get-bar").value = total ? got / total : 0;
      $("get-text").textContent = `${mib(got)}${total ? " of " + mib(total) : ""}, ${((performance.now() - start) / 1000).toFixed(1)} s`;
    }
    await out.close();
    regionReady = region === x;
    $("get-text").textContent = `The file is in the storage of this site (${mib(got)}).`;
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
  const ok = source() === "file" ? $("file").files.length > 0 : regionReady;
  $("start").disabled = !ok || !!worker;
}

// ---- 3. the cut and its progress
$("start").addEventListener("click", async () => {
  await cleaned;
  const boundary = $("boundary").files[0] ? await $("boundary").files[0].text() : null;
  const msg = {
    zoom: Number($("zoom").value), border: Number($("border").value) || 0, threads: Number($("threads").value) || 1,
    boundary, complete: "multipolygon,boundary",
    ...(source() === "file" ? { file: $("file").files[0] } : { region: fileName(region.id) }),
  };
  $("progress").hidden = false;
  $("result").hidden = true;
  $("error").hidden = true;
  $("steps").replaceChildren(...STEPS.map(([id, label]) => {
    const li = document.createElement("li");
    li.dataset.step = id;
    li.innerHTML = `<span>${id}</span><b></b><span class="t"></span>`;
    li.querySelector("b").textContent = label;
    return li;
  }));
  const t0 = performance.now();
  let shared = null, lastT = null, panic = null;
  const show = (bytes) => { $("memory").textContent = mib(bytes); };
  timer = setInterval(() => {
    $("time").textContent = `${((performance.now() - t0) / 1000).toFixed(1)} s`;
    if (shared) show(shared.buffer.byteLength);
  }, 200);
  const used = async () => {
    const e = await navigator.storage.estimate();
    $("storage").textContent = `${mib(e.usage)} of ${mib(e.quota)}`;
  };
  used();
  const storage = setInterval(used, 1000);
  const end = () => { clearInterval(timer); clearInterval(storage); used(); worker?.terminate(); worker = null; $("stop").disabled = true; ready(); };
  const mark = (id) => {
    const li = $("steps").querySelector(`[data-step="${id}"]`);
    for (const x of $("steps").children) x.classList.remove("run");
    li?.nextElementSibling?.classList.add("run");
    return li;
  };
  worker = new Worker("worker.js?v=dev", { type: "module" });
  worker.onmessage = ({ data: m }) => {
    if (m.type === "stage") {
      $("stage").textContent = m.name;
      if (m.name === "cut") { lastT = m.t; $("steps").firstElementChild.classList.add("run"); $("stage").textContent = STEPS[0][1]; }
    } else if (m.type === "memory") {
      shared = m.memory;
      show(m.bytes);
    } else if (m.type === "step") {
      if (m.memory) show(m.memory);
      if (m.name.startsWith("panic:")) { panic = m.name; return; }
      const [, id, text] = m.name.match(/^tile cut (\w+): (.*)$/) ?? [];
      const li = id && mark(id);
      if (li) {
        li.classList.add("done");
        li.querySelector("b").textContent = text;
        li.querySelector(".t").textContent = `${((m.t - lastT) / 1000).toFixed(2)} s`;
        lastT = m.t;
        const next = STEPS[STEPS.findIndex((s) => s[0] === id) + 1];
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
      $("error").textContent = `The cut stopped: ${panic ?? m.message}`;
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
  $("summary").textContent = `${files.length} tile files (${mib(bytes)}) of ${report.tiles} tiles at zoom ${$("zoom").value}, in ${report.seconds.toFixed(1)} s.`;
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
  $("save-text").textContent = can ? "" : "This browser cannot save to a folder (Chrome and Edge on a computer can). Click a file name to download one file.";
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
