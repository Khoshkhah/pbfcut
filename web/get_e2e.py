"""The end-to-end test of "Get" with Playwright (Chromium).

    python3 web/get_e2e.py

A local relay gives 23 MiB of random bytes with `Range` (the file comes in parts over 8 connections), with `Range` and
a connection that ends in the middle of a part (the part starts again), and without `Range` (one stream). The file in
OPFS must equal the bytes of the relay in the three cases. It serves web/www, so
web/build.sh is not needed.
"""
import hashlib, http.server, os, re, sys, threading
from pathlib import Path
from playwright.sync_api import sync_playwright
WWW = Path(__file__).resolve().parent / "www"; DATA = os.urandom(23 * (1 << 20) + 12345)      # not a multiple of 4 MiB
SHA = hashlib.sha256(DATA).hexdigest()
class H(http.server.SimpleHTTPRequestHandler):
    ranges = True       # the relay gives HEAD, Accept-Ranges and Content-Location, as the real one
    flaky = False       # the first part ends in the middle: the page must start it again at the byte where it stopped
    cut = threading.Event()

    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(WWW), **k)

    def log_message(self, *a):
        pass

    def heads(self, n, status=200):
        self.send_response(status)
        self.send_header("Content-Length", str(n))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Content-Location, ETag, Accept-Ranges")
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
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


srv = http.server.ThreadingHTTPServer(("", 0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
HASH = """async () => { const d = await navigator.storage.getDirectory(); const r = await d.getDirectoryHandle('regions'); for await (const [n, h] of r.entries()) { const f = await h.getFile(); const b = await crypto.subtle.digest('SHA-256', await f.arrayBuffer()); return f.size + ' ' + [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join(''); } }"""
with sync_playwright() as p:
    b = p.chromium.launch()
    for ranges, flaky in ((True, False), (True, True), (False, False)):
        H.ranges, H.flaky = ranges, flaky
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
        print("ranges" if ranges else "no ranges", "flaky" if flaky else "", "|", pg.text_content("#get-text"), "|", "OK" if got == f"{len(DATA)} {SHA}" else f"WRONG {got}")
        ctx.close()
    b.close()
