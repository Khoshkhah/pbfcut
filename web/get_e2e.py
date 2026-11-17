"""The end-to-end test of "Get" with Playwright (Chromium).

    python3 web/get_e2e.py

A local relay gives 23 MiB of random bytes with `Range` (the file comes in parts over 8 connections), with `Range` and
a connection that ends in the middle of a part (the part starts again), without `Range` (one stream), and from its
cache (one stream). Download (the save dialog) must give the same bytes. The file in
OPFS must equal the bytes of the relay in the three cases. It serves web/www, so
web/build.sh is not needed.
"""
import hashlib, http.server, os, re, sys, threading, time
from pathlib import Path
from playwright.sync_api import sync_playwright
WWW = Path(__file__).resolve().parent / "www"; DATA = os.urandom(23 * (1 << 20) + 12345)      # not a multiple of 4 MiB
SHA = hashlib.sha256(DATA).hexdigest()
class H(http.server.SimpleHTTPRequestHandler):
    ranges = True       # the relay gives HEAD, Accept-Ranges and Content-Location, as the real one
    flaky = False       # the first part ends in the middle: the page must start it again at the byte where it stopped
    cut = threading.Event()
    hit = False         # the relay has the file in its cache: the page gets it in one connection
    slow = False        # 16 KiB every 50 ms: time to press Cancel

    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(WWW), **k)

    def log_message(self, *a):
        pass

    def heads(self, n, status=200):
        self.send_response(status)
        self.send_header("Content-Length", str(n))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Content-Location, ETag, Accept-Ranges, X-Relay-Cache")
        self.send_header("X-Relay-Cache", "HIT age=1" if H.hit else "MISS-direct")
        if H.ranges:
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Location", "https://download.geofabrik.de/europe/monaco-261115.osm.pbf")
            self.send_header("ETag", '"abc"')

    def do_HEAD(self):
        if not self.path.startswith("/relay"):
            return super().do_HEAD()
        self.heads(len(DATA))
        self.end_headers()

    def do_GET(self):
        if not self.path.startswith("/relay"):
            return super().do_GET()
        m = re.match(r"bytes=(\d+)-(\d+)", self.headers.get("Range") or "")
        if m and H.ranges:
            a, b = int(m[1]), min(int(m[2]), len(DATA) - 1)
            body = DATA[a:b + 1]
            self.heads(len(body), 206)
            self.send_header("Content-Range", f"bytes {a}-{b}/{len(DATA)}")
        else:
            body = DATA
            self.heads(len(body))
        self.end_headers()
        try:
            if H.flaky and m and a > 0 and not H.cut.is_set():
                H.cut.set()
                self.wfile.write(body[: len(body) // 2])      # then the connection ends
                self.wfile.flush()
                self.close_connection = True
                return
            if H.slow:
                for i in range(0, len(body), 1 << 14):
                    self.wfile.write(body[i:i + (1 << 14)])
                    self.wfile.flush()
                    time.sleep(0.05)
            else:
                self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


srv = http.server.ThreadingHTTPServer(("", 0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
HASH = """async () => { const d = await navigator.storage.getDirectory(); const r = await d.getDirectoryHandle('regions'); for await (const [n, h] of r.entries()) { const f = await h.getFile(); const b = await crypto.subtle.digest('SHA-256', await f.arrayBuffer()); return f.size + ' ' + [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join(''); } }"""
with sync_playwright() as p:
    b = p.chromium.launch()
    for ranges, flaky, hit in ((True, False, False), (True, True, False), (False, False, False), (True, False, True)):
        H.ranges, H.flaky, H.hit = ranges, flaky, hit
        H.cut.clear()
        ctx = b.new_context(); pg = ctx.new_page()
        pg.goto(f"http://localhost:{port}/index.html?relay=/relay"); pg.wait_for_timeout(2000); pg.reload(); pg.wait_for_timeout(2000)
        pg.fill("#region", "Europe / Monaco"); pg.dispatch_event("#region", "input"); pg.click("#get")
        pg.on("console", lambda m: print("console:", m.text))
        pg.on("pageerror", lambda e: print("pageerror:", e))
        try:
            pg.wait_for_function("/in the storage|stopped/.test(document.querySelector('#get-text').textContent)", timeout=60000)
        except Exception:
            print("stuck at:", pg.text_content("#get-text"))
            raise
        got = pg.evaluate(HASH)
        print("ranges" if ranges else "no ranges", "flaky" if flaky else "", "cache hit" if hit else "", "|", pg.text_content("#get-text"), "|", "OK" if got == f"{len(DATA)} {SHA}" else f"WRONG {got}")
        assert got == f"{len(DATA)} {SHA}"
        if ranges and not flaky:
            # Download: the save dialog gives a file of the storage here (a test cannot answer a real dialog)
            pg.evaluate("""() => { window.showSaveFilePicker = async () => (await navigator.storage.getDirectory()).getFileHandle('saved.pbf', { create: true }); }""")
            pg.click("#link")
            pg.wait_for_function("/is saved as|stopped/.test(document.querySelector('#get-text').textContent)", timeout=60000)
            saved = pg.evaluate("""async () => { const f = await (await (await navigator.storage.getDirectory()).getFileHandle('saved.pbf')).getFile(); const b = await crypto.subtle.digest('SHA-256', await f.arrayBuffer()); return f.size + ' ' + [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join(''); }""")
            print("  download |", pg.text_content("#get-text"), "|", "OK" if saved == f"{len(DATA)} {SHA}" else f"WRONG {saved}")
            assert saved == f"{len(DATA)} {SHA}"
            # the saved file is the input of the cut at once, with its name and size
            assert pg.is_checked("#src-f") and pg.evaluate("document.querySelector('#file').files[0]?.size") == len(DATA)
            assert pg.text_content("#input-text").startswith(f"Input: saved.pbf, "), pg.text_content("#input-text")
            print("  download | the input of the cut:", pg.text_content("#input-text"))
        ctx.close()
    # no save dialog (Brave, Firefox): the browser downloads the file by itself, and the page says so
    ctx = b.new_context(); pg = ctx.new_page()
    pg.route("https://download.geofabrik.de/**", lambda r: r.fulfill(status=200, body=b"x", headers={"Content-Type": "application/octet-stream", "Content-Disposition": "attachment"}))
    pg.goto(f"http://localhost:{port}/index.html?relay=/relay"); pg.wait_for_timeout(2000); pg.reload(); pg.wait_for_timeout(2000)
    pg.evaluate("delete self.showSaveFilePicker; delete Window.prototype.showSaveFilePicker")
    pg.fill("#region", "Europe / Monaco"); pg.dispatch_event("#region", "input"); pg.click("#link")
    pg.wait_for_function("/list of downloads/.test(document.querySelector('#get-text').textContent)", timeout=10000)
    print("no save dialog |", pg.text_content("#get-text"))
    ctx.close()
    # Cancel: the download stops, and no part of the file stays in the storage
    H.ranges, H.flaky, H.hit, H.slow = True, False, False, True
    ctx = b.new_context(); pg = ctx.new_page()
    pg.goto(f"http://localhost:{port}/index.html?relay=/relay"); pg.wait_for_timeout(2000); pg.reload(); pg.wait_for_timeout(2000)
    pg.fill("#region", "Europe / Monaco"); pg.dispatch_event("#region", "input"); pg.click("#get")
    pg.wait_for_selector("#get-cancel:not([hidden])", timeout=30000)
    pg.wait_for_timeout(500)
    pg.click("#get-cancel")
    try:
        pg.wait_for_function("document.querySelector('#get-text').textContent === 'The download is canceled.'", timeout=30000)
    except Exception:
        print("cancel stuck at:", pg.text_content("#get-text"), "| cancel hidden:", pg.is_hidden("#get-cancel"))
        raise
    left = pg.evaluate("""async () => { const r = await (await navigator.storage.getDirectory()).getDirectoryHandle('regions'); const n = []; for await (const k of r.keys()) n.push(k); return n; }""")
    assert left == [] and pg.is_hidden("#get-cancel") and pg.is_enabled("#get"), left
    print("cancel |", pg.text_content("#get-text"), "| no file in the storage")
    ctx.close()
    H.slow = False
    b.close()
