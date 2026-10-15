#!/usr/bin/env python3
"""Fixtures and checks for the desktop user journeys. Runs on the CI runner only.

  journeys-fixtures.py make DIR      write testpic.png (a screenshot-like graphic), photo.png (a grainy photo), a.pdf (2 pages), b.pdf (3 pages), scan.pdf (a picture of text)
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


def graphic(path):
    """A screenshot-like PNG well over 50 kB: a window with a gradient header, cards and many lines of text.

    What people really shrink under a size limit: flat areas, anti-aliased text, a few soft gradients. A
    grainy photo cannot stay a PNG under 50 kB, and the app says so (see `photo`)."""
    w, h = 1200, 640
    img = Image.new("RGB", (w, h), (246, 247, 249))
    px = img.load()
    for y in range(120):
        for x in range(w):
            px[x, y] = (int(30 + 60 * x / w), int(70 + 90 * y / 120), int(160 + 70 * x / w))
    d = ImageDraw.Draw(img)
    f_big, f_mid, f_small = font(44), font(26), font(18)
    d.text((40, 36), "Quarterly report - regional summary", fill="white", font=f_big)
    rnd = random.Random(11)
    colours = [(231, 76, 60), (52, 152, 219), (46, 204, 113), (155, 89, 182), (241, 196, 15), (26, 188, 156)]
    for i in range(6):
        x0 = 40 + i * 190
        d.rounded_rectangle([x0, 150, x0 + 175, 290], radius=14, fill=colours[i], outline=(255, 255, 255), width=3)
        d.text((x0 + 18, 170), f"Region {i + 1}", fill="white", font=f_mid)
        d.text((x0 + 18, 220), f"{rnd.randrange(100, 999)}.{rnd.randrange(10, 99)}", fill="white", font=f_big)
    for r in range(10):
        y = 330 + r * 31
        d.rectangle([40, y - 4, w - 40, y + 24], fill=(255, 255, 255) if r % 2 else (236, 240, 244))
        d.text((54, y), f"Order {10000 + rnd.randrange(9999)}  customer {rnd.randrange(1000, 9999)}  "
               f"warehouse {rnd.choice(['North', 'South', 'East', 'West'])}  total {rnd.randrange(100, 9999)}.{rnd.randrange(10, 99)}",
               fill=(40, 44, 52), font=f_small)
        d.rectangle([w - 300, y + 6, w - 300 + rnd.randrange(40, 240), y + 16], fill=colours[r % 6])
    img.save(path, "PNG")


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
        graphic(os.path.join(d, "testpic.png"))
        photo(os.path.join(d, "photo.png"))
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
