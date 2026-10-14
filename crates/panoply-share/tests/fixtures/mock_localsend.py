#!/usr/bin/env python3
"""A small LocalSend v2 peer written from the public protocol specification
(github.com/localsend/protocol), used by the interop test as an independent implementation:
a different language, a different HTTP stack and a different TLS library from panoply-share.

  mock_localsend.py receive PORT CERT KEY OUTDIR [PIN]   serve as a receiver
  mock_localsend.py send HOST PORT FILE CERT KEY         act as a sender (also serves /info)
  mock_localsend.py announce MCAST_PORT SERVE_PORT CERT KEY LAN_IP
                                                         multicast an announcement, then serve

Everything it does is printed as one line so the test can read it.
"""
import hashlib
import http.client
import http.server
import json
import os
import socket
import ssl
import sys
import threading
import urllib.parse

API = "/api/localsend/v2"


def fingerprint(certfile):
    with open(certfile) as f:
        der = ssl.PEM_cert_to_DER_cert(f.read())
    return hashlib.sha256(der).hexdigest()


def make_info(alias, fp, port):
    return {
        "alias": alias,
        "version": "2.0",
        "deviceModel": "MockPhone",
        "deviceType": "mobile",
        "fingerprint": fp,
        "port": port,
        "protocol": "https",
        "download": False,
    }


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def server_bind(self):
        # http.server resolves the host name here with a reverse lookup that can hang for
        # many seconds on 0.0.0.0. Nothing needs the name.
        import socketserver
        socketserver.TCPServer.server_bind(self)
        self.server_name = "mock"
        self.server_port = self.server_address[1]


def serve(port, certfile, keyfile, handler_cls):
    srv = Server(("127.0.0.1" if os.environ.get("MOCK_LOOPBACK") else "0.0.0.0", port), handler_cls)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(certfile, keyfile)
    srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
    return srv


def say(*a):
    print(*a, flush=True)


class Receiver(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    info = None
    outdir = None
    pin = None
    sessions = {}

    def log_message(self, *a):
        pass

    def reply(self, code, obj=None):
        body = json.dumps(obj).encode() if obj is not None else b""
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        n = int(self.headers.get("Content-Length", "0"))
        return self.rfile.read(n)

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        if u.path == API + "/info":
            self.reply(200, {k: v for k, v in self.info.items() if k not in ("port", "protocol")})
        else:
            self.reply(404, {"message": "not found"})

    def do_POST(self):
        u = urllib.parse.urlparse(self.path)
        q = dict(urllib.parse.parse_qsl(u.query))
        if u.path == API + "/register":
            peer = json.loads(self.body())
            say("REGISTER", peer["alias"], peer["fingerprint"], peer["version"])
            self.reply(200, {k: v for k, v in self.info.items() if k not in ("port", "protocol")})
        elif u.path == API + "/prepare-upload":
            req = json.loads(self.body())
            if self.pin and q.get("pin") != self.pin:
                return self.reply(401, {"message": "PIN required"})
            sid = "mock-session-1"
            files = req["files"]
            tokens = {fid: "tok-" + fid for fid in files}
            Receiver.sessions[sid] = {"files": files, "tokens": tokens}
            say("PREPARE", req["info"]["alias"], req["info"]["fingerprint"], ",".join(sorted(m["fileName"] for m in files.values())))
            self.reply(200, {"sessionId": sid, "files": tokens})
        elif u.path == API + "/upload":
            s = Receiver.sessions.get(q.get("sessionId"))
            if not s or s["tokens"].get(q.get("fileId")) != q.get("token"):
                self.body()
                return self.reply(403, {"message": "bad token"})
            meta = s["files"][q["fileId"]]
            data = self.body()
            if meta.get("sha256") and hashlib.sha256(data).hexdigest() != meta["sha256"]:
                return self.reply(422, {"message": "hash"})
            dest = os.path.join(self.outdir, *[p for p in meta["fileName"].split("/") if p not in ("", ".", "..")])
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "wb") as f:
                f.write(data)
            say("UPLOADED", meta["fileName"], len(data), hashlib.sha256(data).hexdigest())
            self.reply(200)
        elif u.path == API + "/cancel":
            say("CANCEL", q.get("sessionId"))
            self.reply(200)
        else:
            self.reply(404, {"message": "not found"})


def run_receive(port, cert, key, outdir, pin=None):
    Receiver.info = make_info("Mock Receiver", fingerprint(cert), port)
    Receiver.outdir = outdir
    Receiver.pin = pin
    srv = serve(port, cert, key, Receiver)
    say("READY", Receiver.info["fingerprint"])
    srv.serve_forever()


class InfoOnly(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    info = None

    def log_message(self, *a):
        pass

    def do_GET(self):
        body = json.dumps(self.info).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def run_send(host, port, path, cert, key):
    fp = fingerprint(cert)
    srv = serve(0, cert, key, InfoOnly)
    my_port = srv.server_address[1]
    InfoOnly.info = make_info("Mock Sender", fp, my_port)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    c = http.client.HTTPSConnection(host, port, context=ctx)
    data = open(path, "rb").read()
    meta = {
        "id": "file-1",
        "fileName": os.path.basename(path),
        "size": len(data),
        "fileType": "application/octet-stream",
        "sha256": hashlib.sha256(data).hexdigest(),
    }
    body = json.dumps({"info": make_info("Mock Sender", fp, my_port), "files": {"file-1": meta}})
    c.request("POST", API + "/prepare-upload", body, {"Content-Type": "application/json"})
    r = c.getresponse()
    resp = r.read()
    say("PREPARE_STATUS", r.status)
    if r.status != 200:
        return
    j = json.loads(resp)
    q = urllib.parse.urlencode({"sessionId": j["sessionId"], "fileId": "file-1", "token": j["files"]["file-1"]})
    c.request("POST", API + "/upload?" + q, data, {"Content-Type": "application/octet-stream"})
    r = c.getresponse()
    r.read()
    say("UPLOAD_STATUS", r.status)


def run_announce(mport, sport, cert, key, lan_ip):
    fp = fingerprint(cert)
    Receiver.info = make_info("Mock Announcer", fp, sport)
    Receiver.outdir = "/dev/null"
    srv = serve(sport, cert, key, Receiver)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    msg = dict(Receiver.info, announce=True)
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(lan_ip))
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
    s.sendto(json.dumps(msg).encode(), ("224.0.0.167", mport))
    say("ANNOUNCED", fp)
    threading.Event().wait()


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "receive":
        run_receive(int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6] if len(sys.argv) > 6 else None)
    elif mode == "send":
        run_send(sys.argv[2], int(sys.argv[3]), sys.argv[4], sys.argv[5], sys.argv[6])
    elif mode == "announce":
        run_announce(int(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5], sys.argv[6])
