"""Serve the browser page (web/www) for a local test: python3 web/serve.py [PORT] (default 8800).

It sends no COOP and COEP headers, as GitHub Pages: the page gets them from its service worker. It has a relay for the
files of download.geofabrik.de, which sends no CORS headers: open the page with ?relay=relay to use it.
"""

import http.server
import shutil
import sys
import urllib.parse
import urllib.request
from pathlib import Path

WWW = Path(__file__).resolve().parent / "www"
GEOFABRIK = "https://download.geofabrik.de/"


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(WWW), **k)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def relay(self, head):
        """The relay: a file of download.geofabrik.de with CORS headers, `Range` and the headers of the dated file."""
        url = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query).get("url", [""])[0]
        if not url.startswith(GEOFABRIK):
            return self.send_error(400, "only files of download.geofabrik.de")
        headers = {"User-Agent": "pbfcut-local-relay"}
        if self.headers.get("Range"):
            headers["Range"] = self.headers["Range"]
        req = urllib.request.Request(url, headers=headers, method="HEAD" if head else "GET")
        with urllib.request.urlopen(req) as r:
            self.send_response(r.status)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Content-Location, ETag, Accept-Ranges")
            for h in ("Content-Length", "Content-Range", "Content-Location", "ETag", "Accept-Ranges", "Last-Modified"):
                if r.headers.get(h):
                    self.send_header(h, r.headers[h])
            if not r.headers.get("Content-Location"):
                self.send_header("Content-Location", r.geturl())      # the dated file after the redirect
            self.end_headers()
            if not head:
                shutil.copyfileobj(r, self.wfile, 1 << 20)

    def do_HEAD(self):
        self.relay(True) if urllib.parse.urlsplit(self.path).path == "/relay" else super().do_HEAD()

    def do_GET(self):
        self.relay(False) if urllib.parse.urlsplit(self.path).path == "/relay" else super().do_GET()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8800
    http.server.ThreadingHTTPServer(("", port), Handler).serve_forever()
