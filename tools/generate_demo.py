#!/usr/bin/env python3
"""Generate Portfolio's original editable poster from deterministic vector geometry.

Requires Pillow. No reference images, downloaded assets, or font files are bundled.
The output consists of two transparent RLE raster layers and a review PNG.
"""

from __future__ import annotations

import math
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets"
W, H, SCALE = 1200, 840, 3
PAPER = "#EEE8D8"
INK = "#283126"
RED = "#D9573B"
OLIVE = "#35422F"
SAGE = "#7E865C"
OCHRE = "#CBB16F"
BLUE = "#2C53BC"


def rgba(hex_color: str, alpha: int = 255) -> tuple[int, int, int, int]:
    value = hex_color.lstrip("#")
    return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4)) + (alpha,)


def font(kind: str, size: float) -> ImageFont.FreeTypeFont:
    names = {
        "serif": [
            "/usr/share/fonts/opentype/urw-base35/NimbusRoman-Regular.otf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            "C:/Windows/Fonts/georgia.ttf",
        ],
        "italic": [
            "/usr/share/fonts/opentype/urw-base35/NimbusRoman-Italic.otf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Italic.ttf",
            "C:/Windows/Fonts/georgiai.ttf",
        ],
        "sans": [
            "/usr/share/fonts/opentype/urw-base35/NimbusSans-Regular.otf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "C:/Windows/Fonts/arial.ttf",
        ],
        "bold": [
            "/usr/share/fonts/opentype/urw-base35/NimbusSans-Bold.otf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "C:/Windows/Fonts/arialbd.ttf",
        ],
    }
    for name in names[kind]:
        if Path(name).exists():
            return ImageFont.truetype(name, round(size * SCALE))
    raise RuntimeError(f"Install Nimbus Roman/Sans or DejaVu fonts to render {kind}.")


def path_points(commands: list[tuple]) -> list[tuple[float, float]]:
    result: list[tuple[float, float]] = []
    current = (0.0, 0.0)
    start = current
    for command in commands:
        if command[0] == "M":
            current = (command[1], command[2])
            start = current
            result.append(current)
        elif command[0] == "L":
            current = (command[1], command[2])
            result.append(current)
        elif command[0] == "C":
            p0 = current
            p1, p2, p3 = command[1:3], command[3:5], command[5:7]
            for step in range(1, 65):
                t = step / 64
                u = 1 - t
                result.append((
                    u**3 * p0[0] + 3*u*u*t*p1[0] + 3*u*t*t*p2[0] + t**3*p3[0],
                    u**3 * p0[1] + 3*u*u*t*p1[1] + 3*u*t*t*p2[1] + t**3*p3[1],
                ))
            current = (p3[0], p3[1])
        elif command[0] == "Z":
            result.append(start)
            current = start
        else:
            raise ValueError(command[0])
    return result


def polygon(draw: ImageDraw.ImageDraw, commands: list[tuple], color: str) -> None:
    draw.polygon([(round(x*SCALE), round(y*SCALE)) for x, y in path_points(commands)], fill=rgba(color))


def line(draw: ImageDraw.ImageDraw, points, color: str, width: float = 1.0) -> None:
    draw.line([(round(x*SCALE), round(y*SCALE)) for x, y in points], fill=rgba(color), width=max(1, round(width*SCALE)), joint="curve")


def curve(draw: ImageDraw.ImageDraw, commands: list[tuple], color: str, width: float = 1.0) -> None:
    line(draw, path_points(commands), color, width)


def ellipse(draw: ImageDraw.ImageDraw, bounds, color: str) -> None:
    draw.ellipse(tuple(round(v*SCALE) for v in bounds), fill=rgba(color))


def orbit(draw: ImageDraw.ImageDraw, cx: float, cy: float, rx: float, ry: float, angle: float,
          color: str, width: float = 1.0, start: float = 0, end: float = 2*math.pi) -> None:
    ca, sa = math.cos(angle), math.sin(angle)
    points = []
    for i in range(361):
        t = start + (end - start) * i / 360
        x, y = rx * math.cos(t), ry * math.sin(t)
        points.append((cx + x*ca - y*sa, cy + x*sa + y*ca))
    line(draw, points, color, width)


def star(draw: ImageDraw.ImageDraw, x: float, y: float, radius: float, color: str) -> None:
    points = [(x,y-radius),(x+radius*.18,y-radius*.18),(x+radius,y),(x+radius*.18,y+radius*.18),
              (x,y+radius),(x-radius*.18,y+radius*.18),(x-radius,y),(x-radius*.18,y-radius*.18)]
    draw.polygon([(round(a*SCALE), round(b*SCALE)) for a, b in points], fill=rgba(color))


def word(draw: ImageDraw.ImageDraw, text: str, x: float, y: float, size: float, color=INK,
         kind="sans", tracking: float = 0.0, align="left") -> None:
    face = font(kind, size)
    advances = [draw.textlength(c, font=face) for c in text]
    width = (sum(advances) + max(0, len(text)-1)*tracking*SCALE) / SCALE
    if align == "right":
        x -= width
    elif align == "center":
        x -= width/2
    cursor = x*SCALE
    # All glyphs share a baseline, including punctuation and descenders.
    baseline = round(y*SCALE - face.getbbox(text, anchor="ls")[1])
    if tracking == 0.0:
        draw.text((round(cursor),baseline),text,font=face,fill=rgba(color),anchor="ls")
        return
    for c, advance in zip(text, advances):
        draw.text((round(cursor), baseline), c, font=face, fill=rgba(color), anchor="ls")
        cursor += advance + tracking*SCALE


def draw_forms() -> Image.Image:
    image = Image.new("RGBA", (W*SCALE, H*SCALE))
    d = ImageDraw.Draw(image)
    # A generous vermilion disc acts as the visual counterweight to the title.
    ellipse(d, (716, 164, 1248, 696), RED)
    # Quiet orbital geometry is interrupted naturally by the foreground silhouette.
    orbit(d, 890, 428, 349, 211, -.69, "#A49674", 1.15, -.34, 3.87)
    orbit(d, 917, 412, 325, 169, -.69, "#E8C69D", 1.2, -.08, 3.16)
    # An asymmetrical folded botanical form, drawn from original cubic paths.
    polygon(d, [
        ("M",410,840),("C",449,777,511,753,520,684),
        ("C",530,618,482,563,518,500),("C",558,428,653,438,715,376),
        ("C",781,311,778,226,854,195),("C",923,168,978,201,966,256),
        ("C",953,319,888,344,878,407),("C",866,474,931,511,932,571),
        ("C",934,640,877,679,922,747),("C",947,785,1027,800,1072,840),("Z",)
    ], OLIVE)
    # The inner face carries a softer hue; this creates a readable fold, not a gradient.
    polygon(d, [
        ("M",486,840),("C",539,780,592,746,589,675),
        ("C",585,623,548,568,580,526),("C",617,479,703,475,756,420),
        ("C",808,368,797,301,837,256),("C",866,223,907,214,938,230),
        ("C",910,281,849,324,837,393),("C",822,481,867,534,869,582),
        ("C",870,641,826,674,850,735),("C",871,785,914,815,944,840),("Z",)
    ], SAGE)
    # A thin warm plane turns in the opposite direction at the lower edge.
    polygon(d, [
        ("M",558,840),("C",632,783,668,735,656,675),
        ("C",644,622,606,589,633,552),("C",663,511,739,504,777,462),
        ("C",752,520,698,537,691,586),("C",683,643,734,673,721,732),
        ("C",709,782,677,818,650,840),("Z",)
    ], OCHRE)
    # Delicate contour strokes describe the same geometry as the solid silhouettes.
    curve(d, [("M",573,810),("C",637,746,622,686,603,631),("C",579,562,615,535,684,505),
              ("C",778,465,796,423,799,365)], "#E5DCC1", 1.1)
    curve(d, [("M",754,784),("C",806,730,795,672,784,621),("C",772,562,797,543,809,506)], "#253426", 1.1)
    curve(d, [("M",870,225),("C",831,270,827,316,809,346)], "#B7BB90", .9)
    # The cobalt gesture has a hand-shaped taper and a clear single direction.
    polygon(d, [
        ("M",646,770),("C",686,732,749,702,790,648),
        ("C",832,593,831,548,799,502),("C",764,452,763,405,791,357),
        ("C",824,301,880,275,930,270),("C",976,265,1005,284,1012,310),
        ("C",1020,340,997,360,975,367),("C",995,344,994,327,979,315),
        ("C",958,298,929,304,905,314),("C",864,331,835,353,822,389),
        ("C",805,431,820,457,844,493),("C",884,554,866,617,825,662),
        ("C",776,717,700,750,646,770),("Z",)
    ], BLUE)
    curve(d, [("M",687,757),("C",761,721,829,680,857,609)], "#EEE8D8", .95)
    # Sparse drafting marks add scale without filling the quiet left side.
    star(d, 482, 414, 18, RED)
    star(d, 1067, 130, 23, INK)
    ellipse(d, (1101, 220, 1106, 225), INK)
    line(d, [(1029,721),(1083,721)], INK, 1)
    line(d, [(1056,694),(1056,748)], INK, 1)
    orbit(d, 1056, 721, 7, 7, 0, INK, 1)
    return image.resize((W,H), Image.Resampling.LANCZOS)


def draw_typography() -> Image.Image:
    image = Image.new("RGBA", (W*SCALE,H*SCALE))
    d = ImageDraw.Draw(image)
    word(d, "PORTFOLIO", 64, 55, 14, kind="bold", tracking=3.4)
    word(d, "FIELD NOTES / 2026", 1136, 57, 11, tracking=2.0, align="right")
    line(d, [(64,95),(1136,95)], "#C2BCA9", .8)
    word(d, "FORM", 53, 153, 271, kind="serif", tracking=-5.4)
    word(d, "A study of shape & feeling.", 70, 378, 29, kind="italic")
    word(d, "01", 66, 470, 222, kind="serif", tracking=-5.0)
    line(d, [(73,659),(338,659)], INK, .8)
    word(d, "COLOUR. GESTURE. POSSIBILITY.", 74, 683, 10.0, tracking=1.7)
    word(d, "A little structure.", 75, 727, 15.5, kind="sans")
    word(d, "A little room to wander.", 75, 750, 15.5, kind="sans")
    word(d, "STUDIO EDITION", 75, 800, 9.5, tracking=2.1)
    word(d, "FIG. 01", 1140, 800, 9.5, tracking=2.1, align="right")
    return image.resize((W,H), Image.Resampling.LANCZOS)


def write_rle(image: Image.Image, path: Path) -> int:
    data = image.convert("RGBA").tobytes()
    encoded = bytearray()
    previous = data[:4]
    count = 1
    for offset in range(4,len(data),4):
        value = data[offset:offset+4]
        # Invisible RGB is irrelevant. Normalizing it improves compression.
        if value[3] == 0:
            value = b"\x00\x00\x00\x00"
        if value == previous and count < 0xFFFFFFFF:
            count += 1
        else:
            encoded.extend(struct.pack("<I", count))
            encoded.extend(previous)
            previous, count = value, 1
    encoded.extend(struct.pack("<I",count))
    encoded.extend(previous)
    path.write_bytes(encoded)
    return len(encoded)


def main() -> None:
    OUT.mkdir(parents=True,exist_ok=True)
    forms, typography = draw_forms(), draw_typography()
    total = 0
    for name, layer in [("demo_forms", forms),("demo_type", typography)]:
        amount = write_rle(layer, OUT/f"{name}.rle")
        total += amount
        print(f"{name}.rle: {amount:,} bytes")
    composite = Image.new("RGBA", (W,H), rgba(PAPER))
    composite = Image.alpha_composite(composite,forms)
    composite = Image.alpha_composite(composite,typography)
    composite.convert("RGB").save(OUT/"demo_review.png", optimize=True)
    print(f"Original {W} x {H} poster: {total:,} embedded bytes; no fonts bundled.")


if __name__ == "__main__":
    main()
