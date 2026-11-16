"""Serve the online page (web/www) for a local test: python3 web/serve.py [PORT] (default 8800).

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

    def do_GET(self):
        u = urllib.parse.urlsplit(self.path)
        if u.path != "/relay":
            return super().do_GET()
        url = urllib.parse.parse_qs(u.query).get("url", [""])[0]
        if not url.startswith(GEOFABRIK):
            return self.send_error(400, "only files of download.geofabrik.de")
        headers = {"User-Agent": "pbfcut-local-relay"}
        if self.headers.get("Range"):
            headers["Range"] = self.headers["Range"]
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(req) as r:
            self.send_response(r.status)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", r.headers["Content-Length"])
            if r.headers.get("Content-Range"):
                self.send_header("Content-Range", r.headers["Content-Range"])
            self.send_header("Access-Control-Expose-Headers", "Content-Range, Content-Length")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            shutil.copyfileobj(r, self.wfile, 1 << 20)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8800
    http.server.ThreadingHTTPServer(("", port), Handler).serve_forever()
