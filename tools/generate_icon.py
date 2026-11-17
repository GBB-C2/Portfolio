#!/usr/bin/env python3
"""Generate the original Portfolio P-and-star app mark. Requires Pillow only."""

from pathlib import Path
from PIL import Image, ImageDraw


OUT = Path(__file__).resolve().parents[1]/"assets"
SCALE = 4


def points(commands):
    path=[]
    current=(0.,0.)
    for command in commands:
        if command[0] in ("M","L"):
            current=command[1:3]
            path.append(current)
        elif command[0]=="C":
            a=current
            b,c,d=command[1:3],command[3:5],command[5:7]
            for step in range(1,65):
                t=step/64
                u=1-t
                path.append((u**3*a[0]+3*u*u*t*b[0]+3*u*t*t*c[0]+t**3*d[0],
                             u**3*a[1]+3*u*u*t*b[1]+3*u*t*t*c[1]+t**3*d[1]))
            current=d
    return [(round(x*SCALE),round(y*SCALE)) for x,y in path]


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    icon=Image.new("RGBA",(256*SCALE,256*SCALE))
    d=ImageDraw.Draw(icon)
    bg="#191919"
    d.rounded_rectangle((0,0,255*SCALE,255*SCALE),radius=24*SCALE,fill=bg,
                        outline="#414141",width=2*SCALE)
    # A custom serif P: these are original paths, not font outlines.
    d.polygon(points([("M",48,43),("L",127,43),("C",176,43,199,63,199,101),
                      ("C",199,139,172,160,127,160),("L",102,160),
                      ("L",102,201),("L",123,208),("L",123,219),
                      ("L",47,219),("L",47,208),("L",68,201),
                      ("L",68,62),("L",48,55)]),fill="#F2F2F2")
    d.polygon(points([("M",102,59),("L",102,144),("L",122,144),
                      ("C",150,144,165,129,165,101),
                      ("C",165,73,150,59,123,59)]),fill=bg)
    x,y,r=182,196,31
    star=[(x,y-r),(x+r*.20,y-r*.20),(x+r,y),(x+r*.20,y+r*.20),
          (x,y+r),(x-r*.20,y+r*.20),(x-r,y),(x-r*.20,y-r*.20)]
    d.polygon([(round(a*SCALE),round(b*SCALE)) for a,b in star],fill="#F2F2F2")
    icon=icon.resize((256,256),Image.Resampling.LANCZOS)
    icon.save(OUT/"portfolio.ico",sizes=[(n,n) for n in (16,20,24,32,40,48,64,128,256)])
    icon.save(OUT/"portfolio_icon.png",optimize=True)
    print(f"Original Portfolio icon: {OUT/'portfolio.ico'}")


if __name__=="__main__":
    main()
