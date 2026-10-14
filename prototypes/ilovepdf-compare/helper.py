import http.server, os, sys, json
ROOT = "/Users/anzalhussainabidi/personal/panoply/tests/corpus/cache/pdf/"
RES = "/Users/anzalhussainabidi/personal/panoply/prototypes/ilovepdf-compare/results/"
FILES = {
 "public-lands":"commons/ia-scan-public-lands.pdf",
 "mining-claims":"commons/ia-scan-mining-claims.pdf",
 "arabic-primer":"commons/arabic-primer-calligraphy.pdf",
 "arabic-book":"commons/arabic-book-lab-al-arab.pdf",
 "fax-ccitt":"qpdf/fax-ccitt.pdf",
 "scanned-skewed":"generated/scanned-skewed.pdf",
 "image-streams":"qpdf/image-streams.pdf",
 "hebrew-parable":"commons/hebrew-abulafia.pdf",
 "cmyk-jpeg":"pdfjs/cmyk-jpeg.pdf",
 "nist":"gov/nist-sp800-53r5.pdf",
 "irs-p17":"gov/irs-p17-2022.pdf",
 "3000-pages":"generated/3000-pages.pdf",
 "arabic-cid":"pdfjs/arabic-cid-truetype.pdf",
 "annotation":"pdfjs/annotation-highlight.pdf",
 "f1040":"gov/irs-f1040-2022.pdf",
 "form-filled":"qpdf/form-filled-acrobat.pdf",
 "acroform-rot":"pdfbox/acroform-rotation.pdf",
 "digitally-signed":"qpdf/digitally-signed.pdf",
 "signed-twice":"pyhanko/signed-twice.pdf",
}
ORIGIN = "https://www.ilovepdf.com"
class H(http.server.BaseHTTPRequestHandler):
    def cors(self):
        self.send_header("Access-Control-Allow-Origin", ORIGIN)
        self.send_header("Access-Control-Allow-Private-Network", "true")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.send_header("Vary", "Origin")
    def do_OPTIONS(self):
        self.send_response(204); self.cors(); self.end_headers()
    def do_GET(self):
        k = self.path.strip("/").split("/")[-1]
        if k == "list":
            b = json.dumps({n: os.path.getsize(ROOT+p) for n,p in FILES.items()}).encode()
            ct = "application/json"
        elif k in FILES:
            b = open(ROOT+FILES[k],"rb").read(); ct = "application/pdf"
        else:
            self.send_response(404); self.cors(); self.end_headers(); return
        self.send_response(200); self.cors()
        self.send_header("Content-Type", ct); self.send_header("Content-Length", str(len(b)))
        self.end_headers(); self.wfile.write(b)
    def do_POST(self):
        k = self.path.strip("/").split("/")[-1]
        n = int(self.headers.get("Content-Length", 0))
        data = self.rfile.read(n)
        if k.split(".")[0].rsplit("_",1)[0] in FILES and k.endswith(".pdf"):
            open(RES+k,"wb").write(data)
            msg = json.dumps({"saved": k, "bytes": len(data)}).encode()
            self.send_response(200)
        else:
            msg = b"{}"; self.send_response(400)
        self.cors(); self.send_header("Content-Length", str(len(msg))); self.end_headers(); self.wfile.write(msg)
    def log_message(self, fmt, *a):
        sys.stderr.write('%s %s\n' % (self.command, self.path)); sys.stderr.flush()
http.server.ThreadingHTTPServer(("127.0.0.1", 8765), H).serve_forever()
