"""Panoply conversion graph: every format, every converter, one source of truth.

Run `python3 conversions/formats.py` from the repo root to regenerate
docs/CONVERSIONS.md and crates/panoply-convert/data/graph.json, and print the
pair count. The Rust crate panoply-convert loads graph.json to route any-to-any
conversions through the shortest chain of converters.

Format entry: ext -> (name, mode). mode: "rw" read and write, "r" read only,
"w" write only.
"""
from collections import defaultdict, deque
import os

FAMILIES = {
    "Image": {
        "engine": "native Rust (image, ravif, jxl-oxide, libheif, libraw)",
        "formats": {
            "jpg": ("JPEG", "rw"), "png": ("PNG", "rw"), "webp": ("WebP", "rw"),
            "avif": ("AVIF", "rw"), "jxl": ("JPEG XL", "rw"), "heic": ("HEIC", "rw"),
            "heif": ("HEIF", "rw"), "gif": ("GIF", "rw"), "apng": ("Animated PNG", "rw"),
            "bmp": ("Bitmap", "rw"), "tiff": ("TIFF", "rw"), "ico": ("Windows icon", "rw"),
            "icns": ("macOS icon", "rw"), "tga": ("Targa", "rw"), "qoi": ("QOI", "rw"),
            "ppm": ("Netpbm", "rw"), "pcx": ("PCX", "rw"), "jp2": ("JPEG 2000", "rw"),
            "dds": ("DirectDraw texture", "rw"), "exr": ("OpenEXR", "rw"), "hdr": ("Radiance HDR", "rw"),
            "psd": ("Photoshop", "r"), "xcf": ("GIMP", "r"),
            "dng": ("Adobe DNG raw", "r"), "cr2": ("Canon raw", "r"), "cr3": ("Canon raw 3", "r"),
            "nef": ("Nikon raw", "r"), "arw": ("Sony raw", "r"), "orf": ("Olympus raw", "r"),
            "rw2": ("Panasonic raw", "r"), "raf": ("Fujifilm raw", "r"), "pef": ("Pentax raw", "r"),
            "srw": ("Samsung raw", "r"),
        },
    },
    "Vector": {
        "engine": "resvg, usvg, own tracer, pdfium",
        "formats": {
            "svg": ("SVG", "rw"), "svgz": ("Compressed SVG", "rw"), "emf": ("Windows EMF", "r"),
            "wmf": ("Windows WMF", "r"), "ai": ("Illustrator (PDF based)", "r"),
        },
    },
    "Video": {
        "engine": "ffmpeg (LGPL build) plus OS hardware encoders",
        "formats": {
            "mp4": ("MP4", "rw"), "mov": ("QuickTime", "rw"), "mkv": ("Matroska", "rw"),
            "webm": ("WebM", "rw"), "avi": ("AVI", "rw"), "wmv": ("Windows Media", "rw"),
            "flv": ("Flash video", "rw"), "m4v": ("M4V", "rw"), "mpg": ("MPEG", "rw"),
            "3gp": ("3GP", "rw"), "ts": ("MPEG-TS", "rw"), "m2ts": ("Blu-ray M2TS", "rw"),
            "mts": ("AVCHD", "r"), "ogv": ("Ogg video", "rw"), "vob": ("DVD VOB", "r"),
            "mxf": ("MXF", "rw"), "f4v": ("F4V", "rw"), "asf": ("ASF", "rw"), "rm": ("RealMedia", "r"),
            "mjpeg": ("Motion JPEG", "rw"), "y4m": ("YUV4MPEG", "rw"),
        },
    },
    "Audio": {
        "engine": "ffmpeg (LGPL build), symphonia",
        "formats": {
            "mp3": ("MP3", "rw"), "aac": ("AAC", "rw"), "m4a": ("M4A", "rw"), "m4r": ("iPhone ringtone", "rw"),
            "wav": ("WAV", "rw"), "flac": ("FLAC", "rw"), "alac": ("Apple Lossless", "rw"),
            "ogg": ("Ogg Vorbis", "rw"), "opus": ("Opus", "rw"), "wma": ("Windows Media Audio", "rw"),
            "aiff": ("AIFF", "rw"), "amr": ("AMR voice", "rw"), "ac3": ("Dolby AC-3", "rw"),
            "eac3": ("Dolby E-AC-3", "rw"), "dts": ("DTS", "r"), "ape": ("Monkey's Audio", "r"),
            "wv": ("WavPack", "rw"), "mka": ("Matroska audio", "rw"), "caf": ("Core Audio", "rw"),
            "au": ("Sun AU", "rw"), "mp2": ("MP2", "rw"), "spx": ("Speex", "r"), "mid": ("MIDI", "rw"),
        },
    },
    "Document": {
        "engine": "LibreOffice core (MPL, downloaded on first use), pandoc, native DOCX writer",
        "formats": {
            "docx": ("Word", "rw"), "doc": ("Word 97-2003", "rw"), "odt": ("OpenDocument text", "rw"),
            "rtf": ("Rich text", "rw"), "txt": ("Plain text", "rw"), "md": ("Markdown", "rw"),
            "html": ("HTML", "rw"), "pages": ("Apple Pages", "r"), "wpd": ("WordPerfect", "r"),
            "wps": ("Microsoft Works", "r"), "tex": ("LaTeX", "rw"), "rst": ("reStructuredText", "rw"),
            "org": ("Org mode", "rw"), "adoc": ("AsciiDoc", "rw"), "textile": ("Textile", "rw"),
            "wiki": ("MediaWiki", "rw"), "typ": ("Typst", "rw"), "docbook": ("DocBook", "rw"),
            "opml": ("OPML outline", "rw"), "pub": ("Publisher", "r"), "vsd": ("Visio", "r"),
        },
    },
    "Spreadsheet": {
        "engine": "calamine, rust_xlsxwriter, LibreOffice core",
        "formats": {
            "xlsx": ("Excel", "rw"), "xls": ("Excel 97-2003", "rw"), "ods": ("OpenDocument sheet", "rw"),
            "numbers": ("Apple Numbers", "r"), "csv": ("CSV", "rw"), "tsv": ("TSV", "rw"),
            "dbf": ("dBase", "rw"),
        },
    },
    "Presentation": {
        "engine": "LibreOffice core",
        "formats": {
            "pptx": ("PowerPoint", "rw"), "ppt": ("PowerPoint 97-2003", "rw"),
            "odp": ("OpenDocument slides", "rw"), "key": ("Apple Keynote", "r"), "ppsx": ("PowerPoint show", "rw"),
        },
    },
    "PDF": {
        "engine": "pdfium (BSD), lopdf, own writers",
        "formats": {
            "pdf": ("PDF", "rw"), "pdfa": ("PDF/A archive", "w"), "xps": ("XPS", "r"),
            "oxps": ("OpenXPS", "r"), "djvu": ("DjVu", "r"),
        },
    },
    "Ebook": {
        "engine": "own EPUB writer, pandoc, LibreOffice core",
        "formats": {
            "epub": ("EPUB", "rw"), "mobi": ("Kindle MOBI", "r"), "azw3": ("Kindle KF8", "rw"),
            "fb2": ("FictionBook", "rw"), "cbz": ("Comic ZIP", "rw"), "cbr": ("Comic RAR", "r"),
            "cb7": ("Comic 7z", "rw"), "pdb": ("Palm eBook", "r"), "lit": ("Microsoft Reader", "r"),
        },
    },
    "Data": {
        "engine": "serde family, polars, rusqlite",
        "formats": {
            "json": ("JSON", "rw"), "ndjson": ("JSON lines", "rw"), "yaml": ("YAML", "rw"),
            "toml": ("TOML", "rw"), "xml": ("XML", "rw"), "ini": ("INI", "rw"), "plist": ("Property list", "rw"),
            "parquet": ("Parquet", "rw"), "avro": ("Avro", "rw"), "arrow": ("Arrow IPC", "rw"),
            "sqlite": ("SQLite database", "rw"), "msgpack": ("MessagePack", "rw"), "bson": ("BSON", "rw"),
            "env": ("dotenv", "rw"), "properties": ("Java properties", "rw"),
        },
    },
    "Subtitles": {
        "engine": "own parsers, OCR model for bitmap tracks",
        "formats": {
            "srt": ("SubRip", "rw"), "vtt": ("WebVTT", "rw"), "ass": ("Advanced SubStation", "rw"),
            "ssa": ("SubStation", "rw"), "sub": ("MicroDVD", "rw"), "sbv": ("YouTube SBV", "rw"),
            "ttml": ("TTML / DFXP", "rw"), "lrc": ("Lyrics LRC", "rw"), "sup": ("Blu-ray PGS", "r"),
            "idx": ("VobSub", "r"),
        },
    },
    "Archive": {
        "engine": "libarchive, sevenz-rust, zstd",
        "formats": {
            "zip": ("ZIP", "rw"), "7z": ("7-Zip", "rw"), "rar": ("RAR", "r"), "tar": ("TAR", "rw"),
            "gz": ("Gzip", "rw"), "tgz": ("Tar gzip", "rw"), "bz2": ("Bzip2", "rw"), "xz": ("XZ", "rw"),
            "zst": ("Zstandard", "rw"), "lz4": ("LZ4", "rw"), "iso": ("Disc image", "rw"), "cab": ("Cabinet", "r"),
            "cpio": ("CPIO", "rw"), "lzh": ("LZH", "r"), "dmg": ("macOS disk image", "r"), "jar": ("Java archive", "rw"),
            "apk": ("Android package", "r"), "deb": ("Debian package", "r"),
        },
    },
    "Font": {
        "engine": "own Rust font tools (allsorts, woff2)",
        "formats": {
            "ttf": ("TrueType", "rw"), "otf": ("OpenType", "rw"), "woff": ("WOFF", "rw"),
            "woff2": ("WOFF2", "rw"), "eot": ("Embedded OpenType", "r"), "ttc": ("TrueType collection", "r"),
            "pfb": ("Type 1", "r"),
        },
    },
    "3D": {
        "engine": "assimp-style loaders in Rust, OpenCascade for STEP and IGES (downloaded on first use)",
        "formats": {
            "obj": ("Wavefront OBJ", "rw"), "stl": ("STL", "rw"), "gltf": ("glTF", "rw"), "glb": ("glTF binary", "rw"),
            "ply": ("PLY", "rw"), "3mf": ("3MF", "rw"), "dae": ("Collada", "rw"), "fbx": ("FBX", "r"),
            "usdz": ("USDZ", "rw"), "step": ("STEP", "r"), "iges": ("IGES", "r"), "3ds": ("3DS", "r"),
        },
    },
    "CAD": {
        "engine": "dxf crate, LibreDWG (read, separate process)",
        "formats": {"dxf": ("DXF", "rw"), "dwg": ("DWG", "r")},
    },
    "Geo": {
        "engine": "geo, gpx, kml crates",
        "formats": {
            "gpx": ("GPX", "rw"), "kml": ("KML", "rw"), "kmz": ("KMZ", "rw"), "geojson": ("GeoJSON", "rw"),
            "shp": ("Shapefile", "rw"), "fit": ("Garmin FIT", "r"), "tcx": ("Training Center", "rw"),
        },
    },
    "Mail and contacts": {
        "engine": "mail-parser, libpff (read), ical, vcard",
        "formats": {
            "eml": ("Email", "rw"), "msg": ("Outlook message", "r"), "mbox": ("Mailbox", "rw"),
            "pst": ("Outlook data file", "r"), "vcf": ("vCard contacts", "rw"), "ics": ("Calendar", "rw"),
        },
    },
    "Notebook and code": {
        "engine": "own renderer, syntax highlighting",
        "formats": {"ipynb": ("Jupyter notebook", "rw"), "py": ("Python script", "w"), "code": ("Any source file", "r")},
    },
    "Scientific and medical": {
        "engine": "dicom-rs, fitsio",
        "formats": {"dcm": ("DICOM medical image", "r"), "fits": ("FITS astronomy image", "r")},
    },
    "Motion and color": {
        "engine": "rlottie, own LUT parser",
        "formats": {"lottie": ("Lottie animation", "r"), "cube": ("Cube LUT", "rw"), "3dl": ("3DL LUT", "rw")},
    },
}

# Converters between families. (from, to, how). "*" means every readable or
# writable format of that family. "ai" marks steps that need a local model;
# they are only used as a single direct step, never chained.
CROSS = [
    ("Image:*", "PDF:pdf", "one page per image"),
    ("PDF:*", "Image:png,jpg,webp,tiff,avif", "render pages"),
    ("Vector:*", "Image:*", "rasterize"),
    ("Image:png,jpg,bmp,webp", "Vector:svg", "trace to vector"),
    ("Vector:*", "PDF:pdf", "vector PDF"),
    ("PDF:pdf", "Vector:svg", "page to SVG"),
    ("Image:*", "Video:mp4,webm,mov", "slideshow or animation"),
    ("Video:*", "Image:gif,apng,webp,png,jpg", "animated or frame export"),
    ("Video:*", "Audio:*", "extract audio"),
    ("Audio:*", "Video:mp4,webm,mov", "with cover art or waveform"),
    ("Audio:mid", "Audio:wav", "synthesize"),
    ("Document:*", "PDF:pdf,pdfa", "print to PDF"),
    ("PDF:pdf", "Document:docx,odt,rtf,txt,md,html", "reflow to text document"),
    ("Document:*", "Ebook:epub,azw3,fb2", "book build"),
    ("Ebook:*", "Document:docx,odt,txt,md,html,rtf", "unpack book"),
    ("Ebook:*", "PDF:pdf", "print book"),
    ("PDF:pdf", "Ebook:epub,cbz", "reflow or page images"),
    ("Image:*", "Ebook:cbz,cb7", "comic build"),
    ("Spreadsheet:*", "Data:json,ndjson,xml,parquet,arrow,sqlite,avro", "table export"),
    ("Data:json,ndjson,xml,parquet,arrow,sqlite,avro", "Spreadsheet:xlsx,ods,csv,tsv", "table import"),
    ("Spreadsheet:*", "PDF:pdf", "print sheet"),
    ("Spreadsheet:*", "Document:html,md", "table to markup"),
    ("PDF:pdf", "Spreadsheet:xlsx,csv", "extract tables"),
    ("Presentation:*", "PDF:pdf", "print slides"),
    ("Presentation:*", "Image:png,jpg", "slide images"),
    ("Presentation:*", "Video:mp4", "slides to video"),
    ("PDF:pdf", "Presentation:pptx", "page per slide"),
    ("Document:md,html,txt", "Presentation:pptx,odp", "outline to slides"),
    ("Subtitles:*", "Document:txt", "transcript"),
    ("Subtitles:*", "PDF:pdf", "printable transcript"),
    ("Archive:*", "Archive:*", "repack"),
    ("3D:*", "Image:png", "render preview"),
    ("CAD:*", "Vector:svg", "drawing to SVG"),
    ("CAD:*", "PDF:pdf", "drawing to PDF"),
    ("Vector:svg", "CAD:dxf", "outline to DXF"),
    ("Mail and contacts:eml,msg,mbox,pst", "PDF:pdf", "print mail"),
    ("Mail and contacts:eml,msg", "Document:html,txt", "mail body"),
    ("Mail and contacts:vcf,ics", "Spreadsheet:csv,xlsx", "contacts or events to table"),
    ("Spreadsheet:csv", "Mail and contacts:vcf,ics", "table to contacts or events"),
    ("Notebook and code:ipynb", "Document:html,md", "render notebook"),
    ("Notebook and code:ipynb", "PDF:pdf", "print notebook"),
    ("Notebook and code:code", "PDF:pdf", "highlighted print"),
    ("Notebook and code:code", "Document:html", "highlighted HTML"),
    ("Scientific and medical:*", "Image:png,jpg,tiff", "export image"),
    ("Motion and color:lottie", "Video:mp4,webm", "render animation"),
    ("Motion and color:lottie", "Image:gif,apng,webp", "render animation"),
    ("Document:html", "Image:png,jpg", "page screenshot"),
    ("Geo:*", "Data:json", "features as JSON"),
    ("Geo:*", "Spreadsheet:csv", "points as table"),
    ("Spreadsheet:csv", "Geo:gpx,kml,geojson", "points from table"),
    ("Font:*", "Image:png", "specimen"),
    ("Font:*", "PDF:pdf", "specimen"),
    # On-device model steps, direct only
    ("Audio:*", "Subtitles:srt,vtt", "transcribe", "ai"),
    ("Video:*", "Subtitles:srt,vtt", "transcribe", "ai"),
    ("Audio:*", "Document:txt,md,docx", "transcribe", "ai"),
    ("Video:*", "Document:txt,md,docx", "transcribe", "ai"),
    ("Document:txt,md", "Audio:mp3,wav,m4a", "read aloud", "ai"),
    ("PDF:pdf", "Audio:mp3,m4a", "read aloud", "ai"),
    ("Image:*", "Document:txt,md,docx", "OCR", "ai"),
    ("PDF:pdf", "PDF:pdf", "OCR to searchable", "ai"),
    ("Subtitles:sup,idx", "Subtitles:srt", "OCR bitmap subtitles", "ai"),
    ("Audio:*", "Audio:mid", "notes to MIDI", "ai"),
    ("Image:*", "Spreadsheet:xlsx,csv", "table OCR", "ai"),
]


def fmts(spec, want):
    fam, _, lst = spec.partition(":")
    pool = FAMILIES[fam]["formats"]
    names = pool.keys() if lst == "*" else lst.split(",")
    ok = []
    for n in names:
        mode = pool[n][1]
        if (want == "r" and "r" in mode) or (want == "w" and "w" in mode):
            ok.append(f"{fam}:{n}")
    return ok


def build():
    edges = defaultdict(set)   # plain steps, chainable
    ai = defaultdict(set)      # model steps, direct only
    for fam, d in FAMILIES.items():
        rs = [f"{fam}:{k}" for k, v in d["formats"].items() if "r" in v[1]]
        ws = [f"{fam}:{k}" for k, v in d["formats"].items() if "w" in v[1]]
        for a in rs:
            for b in ws:
                if a != b:
                    edges[a].add(b)
    for c in CROSS:
        src, dst, _how, *flag = c
        target = ai if flag else edges
        for a in fmts(src, "r"):
            for b in fmts(dst, "w"):
                if a != b or flag:
                    target[a].add(b)
    return edges, ai


# Family pairs that make sense through more than one bridge, e.g. a Word file to
# JPG goes Document -> PDF -> Image. Everything else must be same-family or have
# a direct bridge, so chains like 3D -> image -> video -> audio never count.
COMPOSITE_OK = {
    ("Document", "Image"), ("Spreadsheet", "Image"), ("Ebook", "Image"),
    ("Presentation", "Image"), ("CAD", "Image"), ("Mail and contacts", "Image"),
    ("Notebook and code", "Image"), ("Vector", "Video"), ("Document", "Spreadsheet"),
    ("Spreadsheet", "Document"), ("Ebook", "Presentation"), ("Document", "Presentation"),
    ("Subtitles", "Ebook"), ("Mail and contacts", "Ebook"), ("Notebook and code", "Ebook"),
    ("Image", "Document"),  # "JPG to Word": image placed in a document via PDF
}


def allowed_families():
    ok = {(f, f) for f in FAMILIES} | set(COMPOSITE_OK)
    for c in CROSS:
        if len(c) == 3:  # model-only bridges never justify a plain chain
            ok.add((c[0].split(":")[0], c[1].split(":")[0]))
    return ok


def count_pairs(edges, ai, max_hops=4):
    ok = allowed_families()
    pairs = set()
    nodes = {f"{fam}:{k}" for fam, d in FAMILIES.items() for k in d["formats"]}
    for s in nodes:
        seen = {s: 0}
        q = deque([s])
        while q:
            u = q.popleft()
            if seen[u] >= max_hops:
                continue
            for v in edges.get(u, ()):
                if v not in seen:
                    seen[v] = seen[u] + 1
                    q.append(v)
        for v in seen:
            if v != s and (s.split(":")[0], v.split(":")[0]) in ok:
                pairs.add((s, v))
        for v in ai.get(s, ()):
            pairs.add((s, v))
    return pairs, nodes


def write_md(pairs, nodes, edges):
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "docs", "CONVERSIONS.md")
    direct = sum(len(v) for v in edges.values())
    readable = sum(1 for fam, d in FAMILIES.items() for v in d["formats"].values() if "r" in v[1])
    writable = sum(1 for fam, d in FAMILIES.items() for v in d["formats"].values() if "w" in v[1])
    L = []
    L.append("# Panoply conversions\n")
    L.append("Generated by `conversions/formats.py`. Do not edit by hand.\n")
    L.append(f"**{len(nodes)} formats** in {len(FAMILIES)} families. Panoply reads {readable} and writes {writable}. "
             f"{direct:,} direct converter steps chain into **{len(pairs):,} format-to-format conversions**, "
             "each routed through at most four steps on your device. Only sensible pairs count: same family, a direct "
             "bridge, or a known chain such as Word to JPG through PDF. Steps that need an on-device model "
             "(transcription, OCR, read aloud) are only used directly, never chained.\n")
    L.append("Any format converts to any other format in its family. The tables below list the formats, "
             "then the bridges between families.\n")
    for fam, d in FAMILIES.items():
        L.append(f"## {fam}\n")
        L.append(f"Engine: {d['engine']}\n")
        L.append("| Format | Name | Read | Write |")
        L.append("|---|---|---|---|")
        for k, (name, mode) in d["formats"].items():
            L.append(f"| .{k} | {name} | {'yes' if 'r' in mode else ''} | {'yes' if 'w' in mode else ''} |")
        L.append("")
    L.append("## Bridges between families\n")
    L.append("| From | To | How | Needs a local model |")
    L.append("|---|---|---|---|")
    for c in CROSS:
        src, dst, how, *flag = c
        pretty = lambda s: s.replace(":*", ": every format").replace(":", ": ").replace(",", ", ")
        L.append(f"| {pretty(src)} | {pretty(dst)} | {how} | {'yes' if flag else ''} |")
    L.append("")
    open(out, "w").write("\n".join(L))
    return out


def write_graph_json(edges, ai):
    """Machine-readable graph for the Rust router: formats, plain steps, model steps."""
    import json
    here = os.path.dirname(os.path.abspath(__file__))
    out_dir = os.path.join(here, "..", "crates", "panoply-convert", "data")
    os.makedirs(out_dir, exist_ok=True)
    formats = [{"id": f"{fam}:{k}", "family": fam, "ext": k, "name": name, "read": "r" in mode, "write": "w" in mode}
               for fam, d in FAMILIES.items() for k, (name, mode) in d["formats"].items()]
    bridges = [{"from": c[0], "to": c[1], "how": c[2], "model": bool(c[3:])} for c in CROSS]
    data = {
        "formats": formats,
        "edges": sorted([a, b] for a, bs in edges.items() for b in bs),
        "model_edges": sorted([a, b] for a, bs in ai.items() for b in bs),
        "bridges": bridges,
        "composite_families": sorted(list(p) for p in COMPOSITE_OK),
    }
    path = os.path.join(out_dir, "graph.json")
    with open(path, "w") as f:
        json.dump(data, f, indent=0, separators=(",", ":"))
    return path


if __name__ == "__main__":
    e, a = build()
    pairs, nodes = count_pairs(e, a)
    path = write_md(pairs, nodes, e)
    write_graph_json(e, a)
    print(f"{len(nodes)} formats, {len(pairs):,} conversions -> {os.path.normpath(path)}")
