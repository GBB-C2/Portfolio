#!/usr/bin/env python3
"""Render exported Frame/TextRuns for design review; this is not a screenshot.

Usage: python tools/render_preview.py <preview-directory> [--output image.png]
Requires Pillow. Native text metrics and Windows UI behavior must be tested on Windows.
"""

from __future__ import annotations

import argparse
import json
import os
from functools import lru_cache
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


FONT_LABELS = {
    "bahnschrift": "Bahnschrift",
    "segoe_ui": "Segoe UI",
    "tahoma": "Tahoma",
    "verdana": "Verdana",
    "segoe_script": "Segoe Script",
    "gabriola": "Gabriola",
    "proggy_forever": "Proggy Forever",
    "cozette_vector": "Cozette Vector",
    "proggy_clean": "Proggy Clean",
    "proggy_square": "Proggy Square",
}


def font_key(value) -> str:
    compact = str(value or "bahnschrift").lower().replace("_", "").replace(" ", "")
    return {"bahnschrift": "bahnschrift", "segoeui": "segoe_ui",
            "tahoma": "tahoma", "verdana": "verdana",
            "segoescript": "segoe_script", "gabriola": "gabriola",
            "proggyforever": "proggy_forever", "cozettevector": "cozette_vector",
            "proggyclean": "proggy_clean", "proggysquare": "proggy_square"}.get(compact, "bahnschrift")


@lru_cache(maxsize=256)
def load_font(size: int, bold: bool, serif: bool, family: str = "bahnschrift") -> ImageFont.FreeTypeFont:
    family = font_key(family)
    windows_fonts = Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts"
    weight = "Bold" if bold else "Regular"
    # Windows fonts are used only when already installed. Nothing is downloaded
    # or redistributed. Linux substitutes are deliberately reported below and
    # are not described as metrically identical to the Windows families.
    bundled = {
        "proggy_forever": "ProggyForever.ttf",
        "cozette_vector": "CozetteVectorBold.ttf" if bold else "CozetteVector.ttf",
        "proggy_clean": "ProggyClean.ttf",
        "proggy_square": "ProggySquare.ttf",
    }
    if not serif and family in bundled:
        path = Path(__file__).resolve().parents[1] / "assets" / "fonts" / bundled[family]
        if path.is_file():
            return ImageFont.truetype(str(path), max(1, size))
        raise RuntimeError(f"Missing bundled font: {path}")
    if serif:
        candidates = [windows_fonts / ("georgiab.ttf" if bold else "georgia.ttf"),
                      Path(f"/usr/share/fonts/opentype/urw-base35/NimbusRoman-{weight}.otf"),
                      Path(f"/usr/share/fonts/truetype/dejavu/DejaVuSerif{'-Bold' if bold else ''}.ttf")]
    else:
        windows_names = {
            "bahnschrift": ["bahnschrift.ttf"],
            "segoe_ui": ["seguisb.ttf", "segoeuib.ttf"] if bold else ["segoeui.ttf"],
            "tahoma": ["tahomabd.ttf" if bold else "tahoma.ttf"],
            "verdana": ["verdanab.ttf" if bold else "verdana.ttf"],
            "segoe_script": ["segoescb.ttf" if bold else "segoesc.ttf"],
            "gabriola": ["gabriola.ttf"],
        }
        substitutes = {
            "bahnschrift": f"/usr/share/fonts/truetype/dejavu/DejaVuSansCondensed{'-Bold' if bold else ''}.ttf",
            "segoe_ui": f"/usr/share/fonts/truetype/noto/NotoSans-{weight}.ttf",
            "tahoma": f"/usr/share/fonts/truetype/liberation/LiberationSans-{weight}.ttf",
            "verdana": f"/usr/share/fonts/truetype/dejavu/DejaVuSans{'-Bold' if bold else ''}.ttf",
            # Z003 is an installed calligraphic face with Cyrillic coverage.
            # Both requested families share this one preview substitute; it is
            # not an exact visual or metrics reproduction of either Windows font.
            "segoe_script": "/usr/share/fonts/opentype/urw-base35/Z003-MediumItalic.otf",
            "gabriola": "/usr/share/fonts/opentype/urw-base35/Z003-MediumItalic.otf",
        }
        candidates = [windows_fonts / name for name in windows_names[family]]
        # Match the native choice when the selected Windows face is absent.
        candidates += [windows_fonts / name for name in windows_names["segoe_ui"]]
        candidates += [Path(substitutes[family]),
                       Path(f"/usr/share/fonts/truetype/noto/NotoSans-{weight}.ttf"),
                       Path(f"/usr/share/fonts/truetype/dejavu/DejaVuSans{'-Bold' if bold else ''}.ttf")]
    failures = []
    for candidate in candidates:
        if not candidate.is_file():
            continue
        try:
            result = ImageFont.truetype(str(candidate), max(1, size))
            if candidate.name.lower() == "bahnschrift.ttf":
                # Bahnschrift is a variable font. Keep its default width and
                # choose the same regular/semibold weights as the GDI path.
                try:
                    axes = result.get_variation_axes()
                    values = [axis["default"] for axis in axes]
                    for index, axis in enumerate(axes):
                        name = axis["name"].decode("utf-8", errors="replace") if isinstance(axis["name"], bytes) else str(axis["name"])
                        if "weight" in name.lower():
                            values[index] = min(axis["maximum"], max(axis["minimum"], 600 if bold else 400))
                    result.set_variation_by_axes(values)
                except (AttributeError, OSError, ValueError):
                    pass
            return result
        except (OSError, ValueError) as error:
            failures.append(f"{candidate}: {error}")
    details = "; ".join(failures)
    raise RuntimeError(f"No usable UI font. Install the selected Windows font, or Noto Sans/DejaVu for design previews. {details}")


@lru_cache(maxsize=128)
def font_coverage(path: str, index: int = 0):
    try:
        from fontTools.ttLib import TTFont
        source = TTFont(path, fontNumber=index, lazy=True)
        coverage = frozenset((source.getBestCmap() or {}).keys())
        source.close()
        return coverage
    except (ImportError, OSError, ValueError):
        return None


def covers(font, text: str) -> bool:
    coverage = font_coverage(str(font.path), int(getattr(font, "index", 0)))
    if coverage is None:
        # Fallback heuristics when optional FontTools is not installed.
        name = Path(font.path).name.lower()
        if "proggy" in name and any(ord(c) > 255 for c in text):
            return False
        if "cjk" not in name and any(ord(c) >= 0x3000 for c in text):
            return False
        return True
    return all(c.isspace() or ord(c) in coverage for c in text)


def text_script(text: str, language: str) -> str:
    if any(0x1100 <= ord(c) <= 0x11ff or 0x3130 <= ord(c) <= 0x318f or 0xac00 <= ord(c) <= 0xd7af for c in text):
        return "ko"
    if any(0x3040 <= ord(c) <= 0x30ff or 0x31f0 <= ord(c) <= 0x31ff for c in text):
        return "ja"
    if any(0x3400 <= ord(c) <= 0x9fff or 0xf900 <= ord(c) <= 0xfaff for c in text):
        return language if language in ("ko", "ja") else "zh"
    return "latin"


@lru_cache(maxsize=128)
def load_cjk(size: int, bold: bool, language: str):
    path = Path(f"/usr/share/fonts/opentype/noto/NotoSansCJK-{'Bold' if bold else 'Regular'}.ttc")
    if path.is_file():
        return ImageFont.truetype(str(path), max(1, size), index={"ja": 0, "ko": 1, "zh": 2}[language])
    windows = Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts"
    for name in {"ja": ["YuGothM.ttc", "meiryo.ttc"], "ko": ["malgun.ttf", "gulim.ttc"], "zh": ["msyh.ttc", "simhei.ttf"]}[language]:
        if (windows / name).is_file():
            return ImageFont.truetype(str(windows / name), max(1, size))
    raise RuntimeError(f"CJK glyphs require Noto Sans CJK or a suitable Windows {language} font.")


def font_for_text(size: int, bold: bool, serif: bool, family: str, text: str, language: str):
    font = load_font(size, bold, serif, family)
    if covers(font, text):
        return font
    script = text_script(text, language)
    if script != "latin":
        return load_cjk(size, bold, script)
    return load_font(size, bold, False, "segoe_ui")


def text_color(value) -> tuple[int,int,int,int]:
    if isinstance(value,str):
        integer = int(value.lstrip("#"),16)
        return ((integer>>16)&255,(integer>>8)&255,integer&255,255)
    if isinstance(value,(list,tuple)):
        return tuple(value[:3])+(255,)
    # Rust exports packed RGB, independent of Win32 COLORREF byte order.
    integer = int(value)
    return ((integer>>16)&255,(integer>>8)&255,integer&255,255)


def end_ellipsis(text: str, font: ImageFont.FreeTypeFont, width: int) -> str:
    if font.getlength(text) <= width:
        return text
    suffix = "…"
    if font.getlength(suffix) > width:
        return ""
    lo, hi = 0, len(text)
    while lo < hi:
        mid = (lo+hi+1)//2
        if font.getlength(text[:mid]+suffix) <= width:
            lo = mid
        else:
            hi = mid-1
    return text[:lo].rstrip()+suffix


def render(directory: Path, output: Path, language: str = "ru") -> None:
    frame = Image.open(directory/"frame.ppm").convert("RGBA")
    runs = json.loads((directory/"text.json").read_text(encoding="utf-8"))
    font_usage = set()
    for run in runs:
        x,y = round(float(run["x"])),round(float(run["y"]))
        width,height = round(float(run.get("w",run.get("width",0)))),round(float(run.get("h",run.get("height",0))))
        if width <= 0 or height <= 0:
            continue
        serif = bool(run.get("serif"))
        # Respect the exported family at every size, including small labels.
        # Only actual font availability can trigger the reported substitution.
        family = font_key(run.get("font"))
        text = str(run.get("text", "")).replace("\r", "").replace("\n", " ")
        font = font_for_text(round(float(run["size"])), bool(run.get("bold")), serif, family, text, language)
        requested = "Georgia" if serif else FONT_LABELS[family]
        font_usage.add((requested,font.getname()[0],Path(font.path).name))
        # The native path is DrawTextW with SINGLELINE, VCENTER, END_ELLIPSIS.
        text = str(run.get("text","")).replace("\r","").replace("\n"," ")
        text = end_ellipsis(text,font,width)
        if not text:
            continue
        layer = Image.new("RGBA",(width,height))
        draw = ImageDraw.Draw(layer)
        advance = font.getlength(text)
        align = run.get("align","left")
        left = (width-advance)/2 if align=="center" else width-advance if align=="right" else 0
        ascent,descent = font.getmetrics()
        baseline = (height-ascent-descent)/2+ascent
        draw.text((round(left),round(baseline)),text,font=font,fill=text_color(run["color"]),anchor="ls")
        # Intersect the text rect with the painter's optional screen-space clip.
        # Keep the original rect for alignment, then crop the rendered tile.
        clip = run.get("clip")
        if isinstance(clip,(list,tuple)) and len(clip)==4:
            cx,cy,cw,ch = (round(float(v)) for v in clip)
        else:
            cx,cy,cw,ch = 0,0,frame.width,frame.height
        left = max(x,cx,0)
        top = max(y,cy,0)
        right = min(x+width,cx+max(0,cw),frame.width)
        bottom = min(y+height,cy+max(0,ch),frame.height)
        if right>left and bottom>top:
            frame.alpha_composite(layer.crop((left-x,top-y,right-x,bottom-y)),(left,top))
    output.parent.mkdir(parents=True,exist_ok=True)
    frame.convert("RGB").save(output,optimize=True)
    print(f"Design render (not a native screenshot): {output}")
    for requested, actual, filename in sorted(font_usage):
        note = "bundled MIT face" if filename.startswith(("Proggy", "Cozette")) else "installed face" if actual.casefold()==requested.casefold() else "preview substitute; metrics differ"
        print(f"Font: {requested} -> {actual} [{filename}] ({note})")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path)
    parser.add_argument("--output",type=Path)
    parser.add_argument("--language", choices=["ru", "en", "de", "ko", "zh", "ja"], default="ru")
    args=parser.parse_args()
    render(args.directory,args.output or args.directory/"preview.png", args.language)


if __name__=="__main__":
    main()
