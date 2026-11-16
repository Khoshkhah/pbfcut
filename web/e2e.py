"""The end-to-end test of the online page, with Playwright (Chromium), at desktop size and at phone size (390 x 844).

    python3 web/e2e.py [--relay] [--shots DIR]

First run web/build.sh and `cargo build --release`. The test serves web/www with web/serve.py (no COOP and COEP
headers, as GitHub Pages), so the page must get cross-origin isolation from its service worker. On the desktop it cuts
tests/data/monaco.osm.pbf at zoom 14 inside tests/data/monaco.geojson, saves all files with "Save all to a folder"
(the folder is in OPFS), and compares the bytes of each file with the files of the command line. On the phone it does
the cut, downloads one file, and makes sure that the page has no horizontal scroll. A refresh must remove all
files of the site. --relay: also get Monaco from
download.geofabrik.de through the relay of serve.py, and cut it.
"""

import base64
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
PBF = ROOT / "tests/data/monaco.osm.pbf"
AREA = ROOT / "tests/data/monaco.geojson"


def free_port():
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def cli_files(out):
    subprocess.run([ROOT / "target/release/pbfcut", "tiles", PBF, "--zoom", "14", "--boundary", AREA, "-o", out],
                   check=True, stdout=subprocess.DEVNULL)
    return {p.name: p.read_bytes() for p in Path(out).glob("*.osm.pbf")}


def open_page(browser, url, **ctx):
    page = browser.new_context(**ctx).new_page()
    page.on("console", lambda m: m.type == "error" and print("console:", m.text))
    page.goto(url)
    # the service worker reloads the page one time; then the page is cross-origin isolated
    page.wait_for_function("self.crossOriginIsolated === true", timeout=20000)
    page.wait_for_function("document.querySelectorAll('#regions option').length > 100")
    return page


def cut(page, threads=None):
    page.fill("#zoom", "14")
    if threads:
        page.fill("#threads", str(threads))
    page.click("#start")
    page.wait_for_selector("#result:not([hidden]), #error:not([hidden])", timeout=120000)
    err = page.locator("#error")
    assert not err.is_visible(), err.text_content()
    done = page.locator("#steps li.done").count()
    assert done == 10, f"{done} of 10 steps are done"
    return page.text_content("#summary")


def main():
    shots = Path(sys.argv[sys.argv.index("--shots") + 1]) if "--shots" in sys.argv else None
    port = free_port()
    server = subprocess.Popen([sys.executable, ROOT / "web/serve.py", str(port)], stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    url = f"http://localhost:{port}/"
    try:
        with tempfile.TemporaryDirectory() as tmp, sync_playwright() as p:
            want = cli_files(tmp)
            browser = p.chromium.launch()

            # the desktop: a local file, a boundary, threads; "Save all to a folder" into an OPFS folder
            page = open_page(browser, url, viewport={"width": 1280, "height": 900})
            page.check("input[value=file]")
            page.set_input_files("#file", str(PBF))
            page.set_input_files("#boundary", str(AREA))
            print("desktop:", cut(page, threads=4))
            assert page.text_content("#threads-hint") == ""
            page.evaluate("""self.showDirectoryPicker = async () =>
                (await navigator.storage.getDirectory()).getDirectoryHandle('saved', { create: true })""")
            page.click("#save")
            page.wait_for_function("document.querySelector('#save-text').textContent.includes('saved in the folder')")
            got = page.evaluate("""async () => {
                const dir = await (await navigator.storage.getDirectory()).getDirectoryHandle('saved'), out = {};
                for await (const [name, h] of dir.entries()) {
                    const b = new Uint8Array(await (await h.getFile()).arrayBuffer());
                    let s = ''; for (let i = 0; i < b.length; i += 32768) s += String.fromCharCode(...b.subarray(i, i + 32768));
                    out[name] = btoa(s);
                }
                return out;
            }""")
            got = {k: base64.b64decode(v) for k, v in got.items()}
            assert sorted(got) == sorted(want), f"files: page {sorted(got)}, command line {sorted(want)}"
            for name in want:
                assert got[name] == want[name], f"{name}: the page and the command line differ"
            print(f"desktop: {len(got)} files equal the files of the command line")
            # a refresh removes all files of the site (the tile files, the temporary files, the saved folder)
            page.reload()
            page.wait_for_function("self.crossOriginIsolated === true")
            page.wait_for_timeout(500)
            left = page.evaluate("""async () => {
                const names = [];
                for await (const n of (await navigator.storage.getDirectory()).keys()) names.push(n);
                return names;
            }""")
            assert left == [], f"after a refresh the storage has {left}"
            print("desktop: a refresh removes all files of the site")
            if shots:
                page.screenshot(path=shots / "desktop.png", full_page=True)

            # the phone: the cut, one file downloaded, no horizontal scroll
            phone = open_page(browser, url, viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True,
                              device_scale_factor=3)
            # a region: the buttons Get and Download; with no relay, Get is disabled (its tooltip says why)
            phone.fill("#region", "Europe / Monaco")
            phone.dispatch_event("#region", "input")
            phone.wait_for_function("document.querySelector('#get').title.includes('CORS')")
            assert phone.is_disabled("#get") and phone.is_visible("#link")
            assert phone.get_attribute("#link", "href").startswith("https://download.geofabrik.de/europe/monaco")
            if shots:
                phone.screenshot(path=shots / "phone-region.png")
            phone.check("input[value=file]")
            phone.set_input_files("#file", str(PBF))
            phone.set_input_files("#boundary", str(AREA))     # this test file has no bounding box in its header
            print("phone, one thread:", cut(phone, threads=1))
            with phone.expect_download() as d:
                phone.click("#files tr:first-child a")
            name = d.value.suggested_filename
            assert Path(d.value.path()).read_bytes()[:4] != b"", name
            width = phone.evaluate("document.documentElement.scrollWidth")
            assert width <= 390, f"the phone page is {width} px wide"
            if shots:
                phone.screenshot(path=shots / "phone.png", full_page=True)

            if "--relay" in sys.argv:
                page = open_page(browser, url + "?relay=relay", viewport={"width": 1280, "height": 900})
                page.fill("#region", "Europe / Monaco")
                page.dispatch_event("#region", "input")
                page.click("#get")
                page.wait_for_function("document.querySelector('#get-text').textContent.includes('in the storage')", timeout=120000)
                print("relay:", page.text_content("#get-text"), cut(page))
            browser.close()
    finally:
        server.terminate()
    print("e2e: all checks passed")


if __name__ == "__main__":
    main()
