#!/usr/bin/env python3
"""Generate all fixtures and prompt.md files for the agent kill test.
Local tools only: python3 + Pillow + numpy, sips, textutil, ffmpeg. No downloads.
Deterministic (seeded). Run: python3 gen_fixtures.py [task-number ...]
"""
import io, json, os, random, shutil, subprocess, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageChops

ROOT = os.path.dirname(os.path.abspath(__file__))
TASKS = os.path.join(ROOT, "tasks")
PRIVATE = os.path.join(ROOT, "_private")
FF = "/opt/homebrew/bin/ffmpeg"

NAMES = {
    1: "01-compress-pdf", 2: "02-merge-pdfs", 3: "03-heic-to-jpg-no-gps",
    4: "04-exam-photo", 5: "05-trim-video", 6: "06-compress-video",
    7: "07-extract-mp3", 8: "08-docx-to-pdf", 9: "09-passport-photo",
    10: "10-find-order-screenshot",
}


def tdir(n):
    return os.path.join(TASKS, NAMES[n])


def fresh(n):
    d = tdir(n)
    shutil.rmtree(os.path.join(d, "fixtures"), ignore_errors=True)
    shutil.rmtree(os.path.join(d, "work"), ignore_errors=True)
    os.makedirs(os.path.join(d, "fixtures"))
    os.makedirs(os.path.join(d, "work"))
    return d


def write_prompt(n, text):
    d = tdir(n)
    with open(os.path.join(d, "prompt.md"), "w") as f:
        f.write(text.strip().replace("{T}", d) + "\n")


def sh(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(" ".join(cmd) + "\n" + r.stderr[-600:])


# ---------- tiny PDF writer (text via Helvetica, optional JPEG image) ----------
def esc(s):
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def build_pdf(pages):
    objs = []  # list of bytes bodies, object number = index+1

    def add(b):
        objs.append(b)
        return len(objs)

    cat = add(b"")      # 1 placeholder
    pgs = add(b"")      # 2 placeholder
    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    kids = []
    for p in pages:
        stream = []
        y = 800
        for i, line in enumerate(p["lines"]):
            size = 20 if i == 0 else 12
            stream.append(f"BT /F1 {size} Tf 50 {y} Td ({esc(line)}) Tj ET")
            y -= 30 if i == 0 else 20
        img_ref = ""
        if p.get("img"):
            jpeg, w, h = p["img"]
            img = add(b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceRGB "
                      b"/BitsPerComponent 8 /Filter /DCTDecode /Length %d >>\nstream\n" % (w, h, len(jpeg))
                      + jpeg + b"\nendstream")
            img_ref = f"/XObject << /Im0 {img} 0 R >>"
            iw = 495
            ih = int(iw * h / w)
            stream.append(f"q {iw} 0 0 {ih} 50 {y - ih - 10} cm /Im0 Do Q")
        data = "\n".join(stream).encode()
        cont = add(b"<< /Length %d >>\nstream\n" % len(data) + data + b"\nendstream")
        page = add(f"<< /Type /Page /Parent {pgs} 0 R /MediaBox [0 0 595 842] /Contents {cont} 0 R "
                   f"/Resources << /Font << /F1 {font} 0 R >> {img_ref} >> >>".encode())
        kids.append(page)
    objs[cat - 1] = f"<< /Type /Catalog /Pages {pgs} 0 R >>".encode()
    objs[pgs - 1] = f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>".encode()
    out = bytearray(b"%PDF-1.4\n")
    offs = []
    for i, b in enumerate(objs):
        offs.append(len(out))
        out += f"{i + 1} 0 obj\n".encode() + b + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for o in offs:
        out += f"{o:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs) + 1} /Root {cat} 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


def photo(w, h, seed, noise=14.0):
    """Photo-like image: smooth blobs plus sensor noise (compresses poorly, like a real scan)."""
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    base = np.zeros((h, w, 3), np.float32)
    for c in range(3):
        base[..., c] = 110 + 60 * np.sin(x / (90 + 40 * c) + seed) + 50 * np.cos(y / (70 + 30 * c) - seed)
    small = rng.random((h // 40 + 2, w // 40 + 2, 3)).astype(np.float32) * 90
    big = np.array(Image.fromarray(small.astype(np.uint8)).resize((w, h), Image.BICUBIC), np.float32)
    img = base + big - 45 + rng.normal(0, noise, (h, w, 3)).astype(np.float32)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))


def jpeg_bytes(im, q):
    b = io.BytesIO()
    im.save(b, "JPEG", quality=q)
    return b.getvalue()


# ---------- tasks ----------
def t1():
    d = fresh(1)
    pages = []
    for i in range(1, 5):
        im = photo(1600, 1100, seed=i, noise=15)
        pages.append({"lines": [f"Site inspection report, page {i}",
                                f"Selectable text marker ALPHA-{i}-7731 on this page.",
                                "Photographs attached below were taken during the visit."],
                      "img": (jpeg_bytes(im, 93), 1600, 1100)})
    data = build_pdf(pages)
    open(os.path.join(d, "fixtures", "scan_report.pdf"), "wb").write(data)
    print("t1 size", len(data))
    write_prompt(1, """
My college upload portal keeps rejecting this file because it is too big. Can you compress
{T}/fixtures/scan_report.pdf so it is under 1 MB? I still need to be able to select and
copy the text in it afterwards. Save the result as {T}/work/compressed.pdf
""")


def t2():
    d = fresh(2)
    spec = {"invoice.pdf": ("INV", 2), "id_proof.pdf": ("IDP", 3), "contract.pdf": ("CON", 4)}
    for name, (tag, n) in spec.items():
        pages = [{"lines": [f"{tag} document page {i}", f"Marker {tag}-P{i}-5520"]} for i in range(1, n + 1)]
        open(os.path.join(d, "fixtures", name), "wb").write(build_pdf(pages))
    write_prompt(2, """
I need to send one PDF to the bank. Please merge these three PDFs into one file, in this order:
contract.pdf first, then id_proof.pdf, then invoice.pdf. They are in {T}/fixtures/.
Save the merged file as {T}/work/merged.pdf
""")


def t3():
    d = fresh(3)
    out = os.path.join(d, "fixtures", "trip_photos")
    os.makedirs(out)
    tmp = os.path.join(PRIVATE, "t3tmp")
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    ref = {}
    for i in range(1, 6):
        im = photo(1600, 1200, seed=30 + i, noise=6)
        ex = Image.Exif()
        ex[0x010F] = "Apple"
        ex[0x0110] = "iPhone 15"
        ex[0x8825] = {1: "N", 2: (12.0, 58.0 + i, 30.0), 3: "E", 4: (77.0, 35.0, 10.0 + i)}
        j = os.path.join(tmp, f"IMG_{4000 + i}.jpg")
        im.save(j, "JPEG", quality=92, exif=ex)
        h = os.path.join(out, f"IMG_{4000 + i}.HEIC")
        sh(["sips", "-s", "format", "heic", j, "--out", h])
        # verify GPS survived the HEIC round trip
        back = os.path.join(tmp, f"back{i}.jpg")
        sh(["sips", "-s", "format", "jpeg", h, "--out", back])
        gps = Image.open(back).getexif().get_ifd(0x8825)
        assert gps, "GPS lost in HEIC fixture " + h
        ref[f"IMG_{4000 + i}"] = list(Image.open(back).size)
    shutil.rmtree(tmp)
    json.dump(ref, open(os.path.join(PRIVATE, "t3.json"), "w"))
    write_prompt(3, """
I exported my trip photos from my iPhone as HEIC and they are in {T}/fixtures/trip_photos/.
I want to post them online, so please convert all of them to JPG and make sure my location
is not embedded in the files. Put the JPGs in {T}/work/out/ (keep the same base names).
""")


def t4():
    d = fresh(4)
    im = photo(2400, 3200, seed=44, noise=18)
    # add detail that survives downscaling (like hair, fabric, background texture)
    rng = np.random.default_rng(44)
    tex = rng.random((3200 // 12, 2400 // 12, 3)).astype(np.float32) * 110
    tex = np.array(Image.fromarray(tex.astype(np.uint8)).resize((2400, 3200), Image.BILINEAR), np.float32)
    im = Image.fromarray(np.clip(np.asarray(im, np.float32) + tex - 55, 0, 255).astype(np.uint8))
    dr = ImageDraw.Draw(im)
    dr.ellipse([700, 700, 1700, 2000], fill=(222, 170, 140))
    dr.rectangle([300, 2300, 2100, 3200], fill=(40, 40, 90))
    im.save(os.path.join(d, "fixtures", "photo.jpg"), "JPEG", quality=92)
    write_prompt(4, """
I need to upload my photo to a government exam portal. They want a JPG that is exactly
200 pixels wide by 230 pixels tall, and the file size must be between 20 KB and 50 KB.
My photo is {T}/fixtures/photo.jpg. Save the result as {T}/work/exam_photo.jpg
""")


def run_ff(args):
    sh([FF, "-y", "-v", "error", "-nostdin"] + args)


def t5():
    d = fresh(5)
    expr = ("128+127*sin(T*1.1)+20*sin(X/5+T*4)", "128+127*sin(T*1.7+1)+20*sin(Y/4+T*3)",
            "128+127*sin(T*0.6+2)+20*sin((X+Y)/6-T*5)")
    vf = (f"color=c=black:s=64x36:r=30:d=20,geq=r='{expr[0]}':g='{expr[1]}':b='{expr[2]}',"
          "scale=640:360:flags=neighbor,format=yuv420p")
    run_ff(["-f", "lavfi", "-i", vf,
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=20",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-g", "300", "-keyint_min", "300",
            "-x264-params", "scenecut=0", "-c:a", "aac", "-b:a", "96k", "-shortest",
            os.path.join(d, "fixtures", "lecture.mp4")])
    write_prompt(5, """
Please cut the part from 00:05 to 00:12 out of {T}/fixtures/lecture.mp4 and save it as
{T}/work/clip.mp4. The clip should start exactly at the 5 second mark and I do not want any
glitchy or frozen frames at the start of it.
""")


def t6():
    d = fresh(6)
    vf = "testsrc2=s=1280x720:r=30:d=12,noise=alls=14:allf=t+u,format=yuv420p"
    run_ff(["-f", "lavfi", "-i", vf,
            "-f", "lavfi", "-i", "sine=frequency=330:sample_rate=48000:duration=12",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "19", "-c:a", "aac", "-b:a", "128k",
            "-shortest", os.path.join(d, "fixtures", "holiday.mp4")])
    print("t6 size", os.path.getsize(os.path.join(d, "fixtures", "holiday.mp4")))
    write_prompt(6, """
I want to send {T}/fixtures/holiday.mp4 in a chat app that only allows files up to 8 MB.
Please shrink it to under 8 MB but keep it at 720p. Save it as {T}/work/holiday_small.mp4
""")


def t7():
    d = fresh(7)
    aud = "aevalsrc=0.5*sin(2*PI*t*(440+110*floor(t/5)))|0.5*sin(2*PI*t*(440+110*floor(t/5))):s=44100:d=15"
    run_ff(["-f", "lavfi", "-i", "color=c=0x224466:s=640x360:r=25:d=15", "-f", "lavfi", "-i", aud,
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "28", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "192k", "-shortest", os.path.join(d, "fixtures", "talk.mp4")])
    write_prompt(7, """
Please pull the audio out of {T}/fixtures/talk.mp4 and give it to me as an MP3 at 128 kbps.
Save it as {T}/work/talk.mp3
""")


def docx_bytes(lorem):
    import zipfile
    W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'

    def p(text, style=None, bold_parts=None, num=False):
        ppr = ""
        if style:
            ppr += f'<w:pStyle w:val="{style}"/>'
        if num:
            ppr += '<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>'
        ppr = f"<w:pPr>{ppr}</w:pPr>" if ppr else ""
        runs = ""
        for t, b in (bold_parts or [(text, False)]):
            runs += f'<w:r>{"<w:rPr><w:b/></w:rPr>" if b else ""}<w:t xml:space="preserve">{t}</w:t></w:r>'
        return f"<w:p>{ppr}{runs}</w:p>"

    def cell(t, b=False):
        return f'<w:tc><w:tcPr><w:tcW w:w="2800" w:type="dxa"/></w:tcPr>{p(t, bold_parts=[(t, b)])}</w:tc>'

    def row(vals, b=False):
        return "<w:tr>" + "".join(cell(v, b) for v in vals) + "</w:tr>"

    border = "".join(f'<w:{e} w:val="single" w:sz="4" w:space="0" w:color="444444"/>'
                     for e in ("top", "left", "bottom", "right", "insideH", "insideV"))
    table = (f'<w:tbl><w:tblPr><w:tblW w:w="8400" w:type="dxa"/><w:tblBorders>{border}</w:tblBorders></w:tblPr>'
             '<w:tblGrid><w:gridCol w:w="2800"/><w:gridCol w:w="2800"/><w:gridCol w:w="2800"/></w:tblGrid>'
             + row(["Metric", "Q2", "Q3"], True) + row(["Q3 revenue", "38,900", "42,500"])
             + row(["Operating cost", "21,300", "22,100"]) + row(["Headcount", "41", "47"]) + "</w:tbl>")
    body = (p("Quarterly Operations Report", "Heading1")
            + p("", bold_parts=[("Prepared for the leadership team. This report covers ", False), ("revenue", True),
                                (", ", False), ("costs", True), (" and the ", False), ("hiring plan", True),
                                (" for the third quarter.", False)])
            + p("1. Summary", "Heading2") + p(lorem) + p("2. Key numbers", "Heading2") + table
            + p("3. Priorities", "Heading2")
            + p("Launch the Pune warehouse", num=True) + p("Close the vendor audit", num=True)
            + p("Hire two support leads", num=True)
            + p("4. Details", "Heading2") + p(lorem) + p(lorem) + p(lorem)
            + p("", bold_parts=[("Sign-off reference: ZX-REP-9047", True)])
            + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>')
    doc = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}</w:body></w:document>'
    styles = (f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {W}>'
              '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>'
              '<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>'
              '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
              '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style>'
              '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="80"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>'
              '</w:styles>')
    numbering = (f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering {W}>'
                 '<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/>'
                 '<w:lvlText w:val="\u2022"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>'
                 '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>')
    ct = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
          '<Default Extension="xml" ContentType="application/xml"/>'
          '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
          '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
          '<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>')
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
    drels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
             '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
             '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>')
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", rels)
        z.writestr("word/document.xml", doc)
        z.writestr("word/styles.xml", styles)
        z.writestr("word/numbering.xml", numbering)
        z.writestr("word/_rels/document.xml.rels", drels)
    return buf.getvalue()


def t8():
    d = fresh(8)
    lorem = ("Operations ran steadily through the quarter, with fulfilment times improving each month "
             "and customer complaints falling by a third. The finance team reconciled all vendor "
             "invoices ahead of schedule, while the logistics group renegotiated two freight contracts "
             "that will reduce unit costs next year. Staffing remained tight in support, which is why "
             "hiring is a priority. ") * 3
    # textutil's DOCX writer flattens tables, so the DOCX is assembled by hand (real table,
    # real bold runs, Heading styles, a real bullet list), as Word would produce.
    open(os.path.join(d, "fixtures", "report.docx"), "wb").write(docx_bytes(lorem))
    textutil_ok = subprocess.run(["textutil", "-convert", "txt", "-stdout", os.path.join(d, "fixtures", "report.docx")],
                                 capture_output=True, text=True)
    assert "Q3 revenue" in textutil_ok.stdout, "fixture docx not readable"
    write_prompt(8, """
Please convert {T}/fixtures/report.docx to a PDF and keep the formatting (headings, bold text,
the table and the bullet list). Save it as {T}/work/report.pdf
""")


def t9():
    d = fresh(9)
    W, H, S = 1200, 1600, 1
    rng = np.random.default_rng(9)
    y, x = np.mgrid[0:H, 0:W].astype(np.float32)
    bg = np.zeros((H, W, 3), np.float32)
    bg[..., 0] = 40 + 25 * (y / H)
    bg[..., 1] = 95 + 30 * (y / H)
    bg[..., 2] = 200 + 30 * (x / W)
    bg += rng.normal(0, 5, bg.shape)
    im = Image.fromarray(np.clip(bg, 0, 255).astype(np.uint8))
    dr = ImageDraw.Draw(im)
    dr.ellipse([100, 1180, 1100, 2000], fill=(150, 30, 40))          # shirt / shoulders
    dr.rectangle([520, 840, 680, 1250], fill=(205, 155, 125))         # neck
    dr.ellipse([370, 330, 830, 910], fill=(224, 172, 140))            # head
    dr.pieslice([355, 300, 845, 780], 180, 360, fill=(60, 35, 25))     # hair
    dr.ellipse([480, 580, 520, 610], fill=(40, 30, 30))
    dr.ellipse([680, 580, 720, 610], fill=(40, 30, 30))
    dr.arc([520, 700, 680, 790], 20, 160, fill=(150, 70, 70), width=6)
    im = im.filter(ImageFilter.GaussianBlur(1.2))
    im.save(os.path.join(d, "fixtures", "portrait.jpg"), "JPEG", quality=93)
    write_prompt(9, """
I need a passport-style photo made from {T}/fixtures/portrait.jpg. It should be a 600x600
pixel JPG with a plain white background instead of the blue one behind me.
Save it as {T}/work/passport.jpg
""")


def t10():
    d = fresh(10)
    out = os.path.join(d, "fixtures", "screenshots")
    os.makedirs(out)
    rnd = random.Random(10)
    fonts = ["/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/Menlo.ttc",
             "/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Supplemental/Verdana.ttf"]
    target_i = 19
    decoys = ["ORDER-4417", "ORDER-4741", "ORDER-1447", "TRACK-4471", "INVOICE-4471", "ORDER-4472", "ORDER-4407"]
    filler = ["Payment received, thank you", "Out for delivery today", "Delivered to front desk",
              "Return window closes soon", "Rate your purchase", "Your cart has 2 items",
              "Refund initiated to original method", "Address updated successfully",
              "New message from seller", "Support will reply in 24 hours"]
    names = []
    for i in range(30):
        w, h = 900, 1600
        dark = rnd.random() < 0.4
        bgc = (24, 26, 32) if dark else (246, 246, 248)
        fg = (230, 230, 235) if dark else (25, 25, 30)
        im = Image.new("RGB", (w, h), bgc)
        dr = ImageDraw.Draw(im)
        dr.rectangle([0, 0, w, 150], fill=(0, 100, 200) if rnd.random() < .5 else (200, 60, 40))
        fpath = rnd.choice(fonts)
        title = ImageFont.truetype(fpath, 46)
        body = ImageFont.truetype(fpath, rnd.choice([26, 28, 30]))
        dr.text((40, 50), rnd.choice(["My Orders", "Messages", "Notifications", "Activity"]), font=title, fill=(255, 255, 255))
        y = 200
        rows = []
        for _ in range(rnd.randint(8, 12)):
            if rnd.random() < 0.45:
                rows.append(f"{rnd.choice(decoys)}  {rnd.choice(filler)}")
            else:
                rows.append(f"ORDER-{rnd.randint(1000, 9999) if True else 0}  {rnd.choice(filler)}")
        rows = [r.replace("ORDER-4471", "ORDER-4470") for r in rows]
        if i == target_i:
            rows[rnd.randint(2, len(rows) - 2)] = "ORDER-4471  Payment received, thank you"
        for r in rows:
            dr.rounded_rectangle([30, y - 12, w - 30, y + 62], 14, outline=(120, 120, 130), width=2)
            dr.text((50, y + 6), r, font=body, fill=fg)
            y += 100
        if rnd.random() < 0.5:
            im = im.filter(ImageFilter.GaussianBlur(0.4))
        name = f"Screenshot 2026-09-{rnd.randint(1, 28):02d} at {rnd.randint(8, 22)}.{rnd.randint(0, 59):02d}.{rnd.randint(0, 59):02d}"
        while name in names:
            name += "_"
        names.append(name)
        ext = "png" if rnd.random() < 0.7 else "jpg"
        fname = f"{name}.{ext}"
        names[-1] = fname
        im.save(os.path.join(out, fname), "PNG" if ext == "png" else "JPEG", **({} if ext == "png" else {"quality": 90}))
    json.dump({"answer": names[target_i]}, open(os.path.join(PRIVATE, "t10.json"), "w"))
    write_prompt(10, """
I have 30 screenshots in {T}/fixtures/screenshots/. One of them shows the order number
ORDER-4471. Which file is it? Write just the file name into {T}/work/answer.txt
""")


if __name__ == "__main__":
    os.makedirs(PRIVATE, exist_ok=True)
    which = [int(a) for a in sys.argv[1:]] or list(range(1, 11))
    for n in which:
        globals()[f"t{n}"]()
        print("generated", NAMES[n])
