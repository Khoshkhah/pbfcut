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


def cli_files(out, *args):
    subprocess.run([ROOT / "target/release/pbfcut", *(args or ("tiles", PBF, "--zoom", "14")), "--boundary", AREA, "-o", out],
                   check=True, stdout=subprocess.DEVNULL)
    return {p.name: p.read_bytes() for p in Path(out).glob("*.osm.pbf")}


def cli_count(*args):
    """The number of tiles, cells or areas of a cut of the command line (its JSON), for the map of the plan."""
    import json
    with tempfile.TemporaryDirectory() as out:
        r = subprocess.run([ROOT / "target/release/pbfcut", *args, "-o", out], check=True, capture_output=True, text=True)
    j = json.loads(r.stdout)
    return j.get("tiles", j.get("cells", j.get("areas")))


def check_plan(page, count, word):
    """The map before the cut: its text gives the count of the command line, and the map is drawn."""
    page.wait_for_function(f"document.querySelector('#plan-text').textContent.startsWith('{count:,} {word}')", timeout=60000)
    page.wait_for_selector("#map canvas", timeout=60000)
    return page.text_content("#plan-text")


def save_all(page):
    """ "Save all to a folder", with the folder picker of the page given an OPFS folder; gives {name: bytes}."""
    page.evaluate("""self.showDirectoryPicker = async () => {
        const root = await navigator.storage.getDirectory();
        await root.removeEntry('saved', { recursive: true }).catch(() => {});
        return root.getDirectoryHandle('saved', { create: true });
    }""")
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
    return {k: base64.b64decode(v) for k, v in got.items()}


def open_page(browser, url, **ctx):
    page = browser.new_context(**ctx).new_page()
    page.on("console", lambda m: m.type == "error" and print("console:", m.text))
    page.goto(url)
    # the service worker reloads the page one time; then the page is cross-origin isolated
    page.wait_for_function("self.crossOriginIsolated === true", timeout=20000)
    page.wait_for_function("Number(document.documentElement.dataset.regions) > 100")
    return page


def cut(page, threads=None):
    page.select_option("#zoom", "14")
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
            # the map from the start, before any file
            page.wait_for_selector("#map canvas", timeout=60000)
            assert page.text_content("#plan-text").startswith("Choose a region, a file or a boundary"), page.text_content("#plan-text")
            # the region search: one match is chosen at once; more matches: a list, Enter takes the first, the arrows and
            # Tab take another; Esc closes the list
            region_value = lambda: page.input_value("#region")
            # letter by letter: "esto" is already unique; the region is chosen, and the next letters go on as typed
            page.fill("#region", "")
            page.click("#region")
            page.keyboard.type("estoni", delay=30)
            assert region_value() == "estoni" and page.is_visible("#region-actions"), region_value()
            page.keyboard.press("Tab")
            assert region_value() == "Europe / Estonia", region_value()
            page.fill("#region", "mona")
            page.wait_for_selector("#region-actions:not([hidden])")
            page.press("#region", "Enter")
            assert region_value() == "Europe / Monaco" and page.is_hidden("#region-list"), region_value()
            page.fill("#region", "georgia")
            page.wait_for_selector("#region-list:not([hidden])")
            options = page.locator("#region-list li").all_text_contents()
            assert len(options) >= 2, options
            page.press("#region", "Enter")
            first = region_value()
            assert first.endswith("Georgia") and page.is_hidden("#region-list"), first
            page.fill("#region", "georgia")
            page.press("#region", "ArrowDown")
            page.press("#region", "Tab")
            assert region_value().endswith("Georgia") and region_value() != first, region_value()
            page.fill("#region", "zzqq")
            assert page.locator("#region-list li").all_text_contents() == ["No region has these words."]
            page.press("#region", "Escape")
            assert page.is_hidden("#region-list") and page.is_hidden("#region-actions")
            print("desktop, the region search:", options, "->", first)
            # a region of Geofabrik, before Get: the map shows the tiles in the outline of the region
            page.fill("#region", "Europe / Monaco")
            page.dispatch_event("#region", "input")
            page.select_option("#zoom", "14")
            page.wait_for_function("/tiles? that touch(es)? the boundary of Monaco/.test(document.querySelector('#plan-text').textContent)", timeout=60000)
            print("desktop, a region before Get:", page.text_content("#plan-text"))
            # a level above the limit of the area: the page takes the highest one that it can cut, and says so
            page.click("label[for=kind-h]")
            page.select_option("#res", "8")       # possible for Monaco
            page.fill("#region", "Europe / Estonia")
            page.wait_for_function("/so the page chose/.test(document.querySelector('#plan-text').textContent)", timeout=60000)
            chosen = page.input_value("#res")
            assert int(chosen) < 8 and not page.locator("#plan-text").evaluate("e => e.classList.contains('bad')"), page.text_content("#plan-text")
            print("desktop, a level above the limit:", page.text_content("#plan-text"))
            page.click("label[for=kind-t]")
            page.fill("#region", "Europe / Monaco")
            # a boundary only (no file yet): the plan, and the zooms above 20,000 tiles are off
            europe = Path(tmp) / "europe.txt"
            europe.write_text("-10,35,30,70")
            page.set_input_files("#boundary", str(europe))
            page.select_option("#zoom", "8")
            page.wait_for_function("/^[0-9,]+ tiles that touch the boundary\\./.test(document.querySelector('#plan-text').textContent)", timeout=60000)
            off = page.evaluate("[...document.querySelectorAll('#zoom option')].filter(o => o.disabled).map(o => Number(o.value))")
            # the tiles of a box at zoom z: all tiles from its north-west corner to its south-east corner
            import math
            def tile(z, lon, lat):
                n = 2 ** z
                return int((lon + 180) / 360 * n), int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n)
            def box_tiles(z):
                (x0, y0), (x1, y1) = tile(z, -10, 70), tile(z, 30, 35)
                return (x1 - x0 + 1) * (y1 - y0 + 1)
            want_off = [z for z in range(0, 19) if box_tiles(z) > 20000]
            assert off == want_off, f"the zooms that are off: {off}, want {want_off}"
            print("desktop, a boundary only:", page.text_content("#plan-text"), "zooms off:", off)
            # the theme: the layers of the plan stay, in the colours of the other theme
            colour = lambda: page.evaluate("planMap.getPaintProperty('outlines-line', 'line-color')")
            before = colour()
            page.click("#theme")
            page.wait_for_function("planMap.isStyleLoaded() && !!planMap.getLayer('outlines-line') && planMap.querySourceFeatures('outlines').length > 0", timeout=60000)
            assert colour() != before, "the colour of the outlines did not change with the theme"
            page.click("#theme")
            page.wait_for_function("planMap.isStyleLoaded() && !!planMap.getLayer('outlines-line')", timeout=60000)
            print("desktop, the theme: the layers stay")
            # a local file with the name of a Geofabrik file: the outline of its region is the boundary of the map and of
            # the cut; the files equal the command line with that outline
            page.set_input_files("#boundary", [])
            named = Path(tmp) / "monaco-latest.osm.pbf"
            named.write_bytes(PBF.read_bytes())
            page.click("label[for=src-f]")
            page.set_input_files("#file", str(named))
            page.select_option("#zoom", "14")
            outline = ROOT / "web/www/regions/monaco.json"
            want_o = cli_count("tiles", PBF, "--zoom", "14", "--boundary", outline)
            page.wait_for_function(f"document.querySelector('#plan-text').textContent.startsWith('{want_o:,} tile') && /the boundary of Monaco/.test(document.querySelector('#plan-text').textContent)", timeout=60000)
            print("desktop, a local Geofabrik file:", cut(page, threads=2))
            got = save_all(page)
            with tempfile.TemporaryDirectory() as o:
                subprocess.run([ROOT / "target/release/pbfcut", "tiles", PBF, "--zoom", "14", "--boundary", outline, "-o", o], check=True, stdout=subprocess.DEVNULL)
                want_files = {q.name: q.read_bytes() for q in Path(o).glob("*.osm.pbf")}
            assert got == want_files, f"the cut in the outline: page {sorted(got)}, command line {sorted(want_files)}"
            print(f"desktop, a local Geofabrik file: {len(got)} files equal the command line with the outline")
            page.click("label[for=src-f]")
            page.set_input_files("#file", str(PBF))
            page.set_input_files("#boundary", str(AREA))
            page.select_option("#zoom", "14")
            print("desktop, the map:", check_plan(page, cli_count("tiles", PBF, "--zoom", "14", "--boundary", AREA), "tile"))
            # a boundary that is not a boundary: the plan gives the error before the cut
            bad = Path(tmp) / "bad.geojson"
            bad.write_text('{"type": "Point", "coordinates": [7.4, 43.7]}')
            page.set_input_files("#boundary", str(bad))
            page.wait_for_function("document.querySelector('#plan-text').textContent.startsWith('This cut cannot start')", timeout=60000)
            print("desktop, a bad boundary:", page.text_content("#plan-text"))
            page.set_input_files("#boundary", str(AREA))
            print("desktop:", cut(page, threads=4))
            assert page.text_content("#threads-hint") == ""
            got = save_all(page)
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

            # areas: one file for each feature, equal to the files of `pbfcut areas` (also for two areas with a shared
            # border, and for a plain Polygon file, which the page makes one area named after the file); areas that
            # overlap stop the cut with the error of the library
            import json
            box = lambda w, s_, e, n: [[[w, s_], [e, s_], [e, n], [w, n], [w, s_]]]
            two = Path(tmp) / "two.geojson"
            two.write_text(json.dumps({"type": "FeatureCollection", "features": [
                {"type": "Feature", "properties": {"name": "west"}, "geometry": {"type": "Polygon", "coordinates": box(7.40, 43.72, 7.42, 43.76)}},
                {"type": "Feature", "properties": {"name": "east"}, "geometry": {"type": "Polygon", "coordinates": box(7.42, 43.72, 7.45, 43.76)}}]}))
            overlap = Path(tmp) / "overlap.geojson"
            overlap.write_text(json.dumps({"type": "FeatureCollection", "features": [
                {"type": "Feature", "properties": {"name": "A"}, "geometry": {"type": "Polygon", "coordinates": box(7.40, 43.72, 7.43, 43.76)}},
                {"type": "Feature", "properties": {"name": "B"}, "geometry": {"type": "Polygon", "coordinates": box(7.42, 43.72, 7.45, 43.76)}}]}))
            plain = Path(tmp) / "plain.geojson"
            plain.write_text(json.dumps(json.load(open(AREA))["features"][0]["geometry"]))
            page.click("label[for=src-f]")
            page.set_input_files("#file", str(PBF))
            page.set_input_files("#boundary", str(AREA))
            page.click("label[for=kind-a]")
            assert page.is_disabled("#start"), "the areas need their file"
            for areas_file, want_dir in [(AREA, "a1"), (two, "a2"), (plain, None)]:
                page.set_input_files("#areas", str(areas_file))
                n = 2 if areas_file == two else 1
                check_plan(page, n, "area")
                page.click("#start")
                page.wait_for_selector("#result:not([hidden]), #error:not([hidden])", timeout=120000)
                assert not page.locator("#error").is_visible(), page.text_content("#error")
                got = save_all(page)
                if want_dir:
                    want_a = cli_files(Path(tmp) / want_dir, "areas", PBF, "--areas", str(areas_file))
                else:     # the plain Polygon: the same area as Monaco, named after the file
                    want_a = {"plain.osm.pbf": cli_files(Path(tmp) / "a1", "areas", PBF, "--areas", str(AREA))["Monaco.osm.pbf"]}
                assert sorted(got) == sorted(want_a), f"areas: page {sorted(got)}, command line {sorted(want_a)}"
                assert all(got[n] == want_a[n] for n in want_a), f"an area file of {areas_file.name} differs"
                print(f"desktop, areas {Path(areas_file).name}: {sorted(got)} equal the command line;", page.text_content("#summary"))
            page.set_input_files("#areas", str(overlap))
            page.click("#start")
            page.wait_for_selector("#error:not([hidden])", timeout=120000)
            err = page.text_content("#error")
            assert "overlap" in err and '"A"' in err and '"B"' in err, err
            print("desktop, areas that overlap:", err)

            # H3 cells: the files equal the files of `pbfcut h3`
            want_h3 = cli_files(Path(tmp) / "h3", "h3", PBF, "--res", "9")
            page.click("label[for=kind-h]")
            page.select_option("#res", "9")
            print("desktop, H3, the map:", check_plan(page, cli_count("h3", PBF, "--res", "9", "--boundary", AREA), "cell"))
            page.click("#start")
            page.wait_for_selector("#result:not([hidden]), #error:not([hidden])", timeout=120000)
            assert not page.locator("#error").is_visible(), page.text_content("#error")
            got = save_all(page)
            assert sorted(got) == sorted(want_h3), f"cells: page {sorted(got)}, command line {sorted(want_h3)}"
            assert all(got[n] == want_h3[n] for n in want_h3), "a cell file of the page differs"
            print(f"desktop, H3: {len(got)} files equal the files of the command line;", page.text_content("#summary"))

            # the help popups: each "?" opens its text
            for b in page.locator("button.help").all():
                if b.is_visible():
                    b.click()
                    assert page.locator("#help-box").is_visible() and page.text_content("#help-title")
                    page.click("#help-box button")
            # the phone: the cut, one file downloaded, no horizontal scroll
            phone = open_page(browser, url, viewport={"width": 390, "height": 844}, is_mobile=True, has_touch=True,
                              device_scale_factor=3)
            # a region: the buttons Get (through the default relay) and Download
            phone.fill("#region", "georgia")
            phone.wait_for_selector("#region-list:not([hidden])")
            phone.tap("#region-list li:nth-child(2)")
            assert phone.input_value("#region").endswith("Georgia"), phone.input_value("#region")
            phone.fill("#region", "Europe / Monaco")
            phone.dispatch_event("#region", "input")
            phone.wait_for_function("document.querySelector('#get').title.includes('storage')")
            assert phone.is_enabled("#get") and phone.is_visible("#link")
            assert phone.get_attribute("#link", "href").startswith("https://download.geofabrik.de/europe/monaco")
            if shots:
                phone.screenshot(path=shots / "phone-region.png")
            phone.click("label[for=src-f]")
            phone.set_input_files("#file", str(PBF))
            phone.set_input_files("#boundary", str(AREA))     # this test file has no bounding box in its header
            phone.select_option("#zoom", "14")
            check_plan(phone, cli_count("tiles", PBF, "--zoom", "14", "--boundary", AREA), "tile")
            if shots:
                phone.screenshot(path=shots / "phone-map.png", full_page=True)
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
