"""Draw the diagrams of the manual: each docs/assets/diagrams/NAME.mmd (Mermaid) to NAME.png next to it.

Mermaid measures each box with the font of the browser that shows it. In the browser of a reader, that font can be
another font, and then the text runs out of its box. So the manual shows images, made here with one font that is loaded
before Mermaid measures. Each diagram gives NAME.png and NAME-dark.png (for the dark theme). Run it after a change to a .mmd file (needs Python Playwright and its Chromium):

    python tools/diagrams.py
"""
import html
import pathlib
from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).resolve().parent.parent / "docs" / "assets" / "diagrams"
PAGE = """<!doctype html><meta charset="utf-8">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;700&display=block">
<style>body { margin: 0; padding: 24px; background: BG; font-family: "IBM Plex Sans"; display: inline-block; }</style>
<pre id="src" hidden>SOURCE</pre>
<div id="d"></div>
<script type="module">
import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11.4.1/dist/mermaid.esm.min.mjs";
await document.fonts.load('16px "IBM Plex Sans"'); await document.fonts.load('bold 16px "IBM Plex Sans"');
mermaid.initialize({ startOnLoad: false, theme: "base", fontFamily: '"IBM Plex Sans"',
  themeVariables: { fontFamily: '"IBM Plex Sans"', fontSize: "16px", lineColor: "LINE", primaryTextColor: "TEXT",
    edgeLabelBackground: "BG", textColor: "TEXT" },
  flowchart: { padding: 16, nodeSpacing: 40, rankSpacing: 40, wrappingWidth: 420 } });
const { svg } = await mermaid.render("g", document.getElementById("src").textContent);
document.getElementById("d").innerHTML = svg;
window.DONE = true;
</script>"""

# the colours of the .mmd files and of the dark copy: background, lines, text, the fills of the boxes
LIGHT = ("#fff", "#ffffff", "#444", "#222", "#eef2ff", "#fff7e0", "#8a6d1f")
DARK = ("#1e2129", "#1e2129", "#aaa", "#e6e6e6", "#28304a", "#3a3220", "#c9a227")

with sync_playwright() as p:
    browser = p.chromium.launch()
    for src in sorted(HERE.glob("*.mmd")):
        page = browser.new_page(device_scale_factor=2)
        for name, colors in [(src.stem, LIGHT), (src.stem + "-dark", DARK)]:
            text = src.read_text()
            for light, other in zip(LIGHT[1:], colors[1:]):
                text = text.replace(light, other)
            page.set_content(PAGE.replace("SOURCE", html.escape(text)).replace("BG", colors[0])
                             .replace("LINE", colors[2]).replace("TEXT", colors[3]))
            page.wait_for_function("window.DONE === true", timeout=60000)
            page.locator("body").screenshot(path=str(src.parent / (name + ".png")))
        print(src.stem, "->", src.stem + ".png,", src.stem + "-dark.png")
    browser.close()
