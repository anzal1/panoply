#!/usr/bin/env python3
"""Fixtures and checks for the desktop user journeys. Runs on the CI runner only.

  journeys-fixtures.py make DIR      write testpic.png, a.pdf (2 pages), b.pdf (3 pages), scan.pdf (a picture of text)
  journeys-fixtures.py pages FILE    print the page count of a PDF
  journeys-fixtures.py text FILE     print the text a PDF carries (empty when it has none)
  journeys-fixtures.py words FILE W  exit 0 when at least two of the comma separated words are in the PDF text

Needs Pillow (HPND) and pypdf (BSD), both installed with pip on the runner.
"""
import os
import random
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont


def font(size):
    for p in (
        r"C:\Windows\Fonts\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    ):
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    try:
        return ImageFont.load_default(size=size)
    except TypeError:
        return ImageFont.load_default()


def photo(path):
    """A photo-like PNG well over 50 kB: a gradient, soft shapes and grain."""
    rnd = random.Random(7)
    w, h = 1600, 1000
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            px[x, y] = (int(40 + 150 * x / w), int(60 + 120 * y / h), int(180 - 90 * x / w))
    d = ImageDraw.Draw(img)
    for _ in range(40):
        x, y, r = rnd.randrange(w), rnd.randrange(h), rnd.randrange(40, 160)
        d.ellipse([x - r, y - r, x + r, y + r], fill=(rnd.randrange(256), rnd.randrange(256), rnd.randrange(256)))
    img = img.filter(ImageFilter.GaussianBlur(6))
    grain = Image.effect_noise((w, h), 28).convert("RGB")
    img = Image.blend(img, grain, 0.12)
    img.save(path, "PNG")


def pages(path, n, label):
    ims = []
    for i in range(n):
        im = Image.new("RGB", (794, 1123), "white")
        d = ImageDraw.Draw(im)
        d.text((60, 80), f"{label} page {i + 1} of {n}", fill="black", font=font(40))
        ims.append(im)
    ims[0].save(path, "PDF", save_all=True, append_images=ims[1:], resolution=96.0)


def scanned(path):
    """One A4 page at 200 dpi that is only a picture of text: no text layer."""
    im = Image.new("RGB", (1654, 2339), "white")
    d = ImageDraw.Draw(im)
    lines = ["INVOICE NUMBER 4812", "Total amount due 350 dollars", "Thank you for your business"]
    y = 220
    for line in lines:
        d.text((150, y), line, fill="black", font=font(78))
        y += 190
    im.save(path, "PDF", resolution=200.0)


def extract(path):
    from pypdf import PdfReader

    r = PdfReader(path)
    return "\n".join((p.extract_text() or "") for p in r.pages)


def main():
    cmd = sys.argv[1]
    if cmd == "make":
        d = sys.argv[2]
        os.makedirs(d, exist_ok=True)
        photo(os.path.join(d, "testpic.png"))
        pages(os.path.join(d, "a.pdf"), 2, "Alpha")
        pages(os.path.join(d, "b.pdf"), 3, "Bravo")
        scanned(os.path.join(d, "scan.pdf"))
        for n in sorted(os.listdir(d)):
            print(n, os.path.getsize(os.path.join(d, n)))
    elif cmd == "pages":
        from pypdf import PdfReader

        print(len(PdfReader(sys.argv[2]).pages))
    elif cmd == "text":
        print(extract(sys.argv[2]).strip())
    elif cmd == "words":
        t = extract(sys.argv[2]).lower()
        hits = [w for w in sys.argv[3].lower().split(",") if w and w in t]
        print(",".join(hits))
        sys.exit(0 if len(hits) >= 2 else 1)
    else:
        sys.exit("unknown command " + cmd)


if __name__ == "__main__":
    main()
