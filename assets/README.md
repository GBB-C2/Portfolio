# Original demonstration artwork

`demo_forms.rle` and `demo_type.rle` contain the original **FORM 01** poster at 1200 × 840 pixels. The geometry, composition, palette and lettering arrangement were created for Portfolio. They do not contain the user's reference images or any downloaded illustrations.

Each file is a sequence of 8-byte records: a positive little-endian `u32` run length, followed by straight RGBA bytes. Each decoded layer contains exactly 1,008,000 pixels. The paper and empty painting layer are created by Rust at startup.

Run `python tools/generate_demo.py` from an environment with Pillow to reproduce the assets. The script uses deterministic vector paths and installed fonts, then rasterizes them with supersampling. `demo_review.png` is a visual review of the document artwork, not a native application screenshot.

## Typography attribution

The original generated pixels use **Nimbus Roman Regular / Italic** and **Nimbus Sans Regular / Bold** from the installed URW Base35 font collection. Font-program copyright: 2015 URW Software; 2013–2014 (URW)++ Design & Development. Upstream: <https://github.com/ArtifexSoftware/urw-base35-fonts>.

No font software or font data is bundled: the artwork contains ordinary rasterized text. The generator refers to system-installed fonts and offers DejaVu or Windows font fallbacks. Reproducing the precise original typography requires the same installed Nimbus fonts.

The optional `tools/render_preview.py` uses installed Windows font families when available and local substitutions otherwise. Linux previews use DejaVu Sans Condensed, Noto Sans, Liberation Sans and DejaVu Sans for the four standard UI families; Segoe Script and Gabriola both use Z003 Medium Italic from URW Base35. Z003 includes Cyrillic but does not reproduce the distinct Windows calligraphic faces. Georgia / Nimbus Roman approximates serif text. The renderer reports the actual files; its output is a design render with metrics that can differ from Windows GDI. No font programs are bundled.

## Application icon

`portfolio.ico` contains 16, 20, 24, 32, 40, 48, 64, 128 and 256 px versions of the original Portfolio mark: a custom serif P and a four-point star on graphite. `portfolio_icon.png` is its 256 px review image. Reproduce both with `python tools/generate_icon.py`. The mark uses original vector paths and no font outlines or external graphics.
