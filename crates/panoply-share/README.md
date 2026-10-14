# panoply-share

Device sharing for Panoply: send and receive files between devices with the open LocalSend v2
protocol, and a QR browser page for devices that do not run Panoply. Specs: `docs/specs/docs-find-files-share-capture.md`
(the share stack block and the Share modules), `docs/ARCHITECTURE.md` section 8, `docs/adr/0006`.
The wire format is written from the public protocol specification (github.com/localsend/protocol), not from
any LocalSend app code. The command line is `panoply share <list|send|receive|qr>`.

## What is in it

| module | job |
|---|---|
| `proto` | The wire types (`DeviceInfo`, `FileMeta`, prepare-upload request and response, prepare-download response), constants (port 53317, group 224.0.0.167, `/api/localsend/v2`), ISO 8601 helpers. |
| `identity` | Self-signed ECDSA P-256 certificate, kept in `cert.pem` and `key.pem` (key mode 0600), so the fingerprint (SHA-256 of the DER certificate, lowercase hex) is stable across restarts. The TLS client pins a peer to its announced fingerprint. |
| `discovery` | One UDP multicast announcement at start (no periodic beacon), replies to announcements with `POST /register` (UDP fallback when that cannot connect), and the HTTP `register` scan of the local /24 for networks that drop multicast. Virtual adapters (VPN, containers, `utun`, `awdl`) are skipped. |
| `node` | `Node`: the HTTPS server (or plain HTTP), the device list, the event stream, trust, and the send API. Cheap to clone. Call `shutdown()` to stop it. |
| `receive` | The endpoints `info`, `register`, `prepare-upload`, `upload`, `cancel`, the prompt callback (`ReceiveHandler`), PIN, trust, rate limits. |
| `send` | Expands files and folders, `prepare-upload` with PIN retries, parallel uploads over reused connections, progress, cancel. Text messages use the protocol's `text/plain` preview. |
| `qr` | One-time web page: file list, per file download with range support, streamed zip, optional upload, optional PIN, optional HTTPS. `terminal_qr` draws the code. |
| `zip`, `multipart` | A streaming stored-zip writer with exact length and Zip64 when needed, and a streaming `multipart/form-data` parser. |
| `fsx` | Name sanitizing, traversal rejection, symlink-safe directory creation, never-overwrite placement through a hidden `.panoply-part` file. |
| `trust`, `words` | The trust list (JSON file) and the four word check code derived from both fingerprints. |
| `cli` | The `panoply share` commands. |

## Using the library

```rust
use panoply_share::{Node, NodeConfig, ReceiveConfig, SendOptions, handler_fn, Decision};

// Receive into a folder, asking on every request.
let mut cfg = NodeConfig::new("Anzal's MacBook");
cfg.data_dir = Some(panoply_share::node::default_data_dir());
cfg.receive = Some(ReceiveConfig::new("/Users/me/Panoply Received", handler_fn(|req| {
    Box::pin(async move {
        // req.sender, req.files, req.check_code, req.verified ... show a prompt, then:
        Decision::Accept { files: None, trust: true }
    })
})));
let node = Node::start(cfg).await?;
let mut events = node.subscribe();      // DeviceFound, Transfer(progress, done, text)

// Send.
let device = node.find("Anzal's iPhone").await?;
let job = node.send(device, vec!["photo.jpg".into(), "Trip".into()], SendOptions::default());
// job.cancel() at any time; job.wait().await gives the summary.
```

`Node::start` and the jobs run on the caller's tokio runtime. The desktop app creates one runtime and calls these
from it; `cli::run` builds its own for the command line.

## Trust and safety rules (all tested)

- Receiving is off unless `NodeConfig::receive` is set. Files go to the chosen folder and are never overwritten
  (`name (1).ext`). Names are sanitized per path segment, any `..` segment rejects the request, folders arrive
  as trees, a symlink anywhere in the save path is refused.
- The sender claims a fingerprint in `prepare-upload`. The receiver connects back to the sender's announced port
  and checks that the certificate it presents hashes to that fingerprint (`IncomingRequest::verified`). Only
  verified, trusted senders skip the prompt, and only verified senders can be trusted from the prompt. A trusted
  alias reused with another fingerprint sets `alias_conflict`.
- One session at a time (HTTP 409 otherwise). Wrong PINs: three tries, then HTTP 429 for a minute. 30 requests a
  minute per address. A sender whose prompts go unanswered three times is locked out for five minutes.
- A file must be exactly its declared size and, when the sender gave `sha256`, match it (HTTP 422).
- Files that run code (`.exe`, `.sh`, `.app`, ...) are flagged `risky` for the prompt. Nothing is ever run.
- QR page: 128 bit token in the path (a wrong token and a wrong path answer identically), `no-store`, a strict CSP,
  files always served as `attachment` octet-stream, closes on first completion, expiry (10 minutes) or revoke, the
  port closes within a second.

## Decisions and limits

- **TLS.** The client speaks TLS 1.3 only and checks the handshake signature against the certificate's public key,
  so a stock peer's certificate profile (for example `CA:TRUE`, which `openssl req -x509` sets) does not matter. The
  server accepts TLS 1.2 and 1.3. The pin is enforced when the announced fingerprint is 64 hex characters (what
  the spec says an HTTPS device announces); any other value cannot be a certificate hash and is not compared.
- **No mDNS.** LocalSend v2 discovers with UDP multicast only, so that is what is implemented. mDNS would add a
  dependency and reach no stock LocalSend app.
- **IPv4 only.** Discovery and scan are IPv4 (the protocol's multicast group is IPv4).
- **No reverse "download" API** (`prepare-download`, `download`): the QR page serves its own HTML instead.
  `download` is announced as `false`.
- **No resume for LocalSend transfers**: the v2 protocol has no byte ranges. The QR page resumes (HTTP Range).
- **Private networks only** is a policy for the app that owns the receive toggle; the library binds where it is told.
- **Client isolation.** Not detected here. A failed scan or a failed connect-back is visible to the caller
  (`verified: false`, `Error::Io`), and the guest-Wi-Fi ladder belongs in the app.
- Receiving a very large session keeps its file table in memory (about 300 bytes per file).

## Tests

`cargo test -p panoply-share`. Everything runs on loopback or this machine's private LAN address, with a private
UDP port, so no real LocalSend app on the network is ever contacted. Tests that need `openssl`, `python3` with
TLS 1.3, `curl` or `unzip` print `SKIPPED:` and pass when the tool is missing.

- `tests/conformance.rs`: the spec's JSON examples parse and re-serialize identically; the endpoints answer with
  the documented bodies and status codes (200, 204, 400, 401, 403, 409, 422, 429).
- `tests/e2e.rs`: two nodes, files and folders, Unicode names, never-overwrite, decline, partial accept, text,
  PIN, trust, cancel (sender and receiver side), busy, 300 small files, HTTP mode, pinned certificate mismatch.
  `-- --ignored throughput` times 1 GiB over loopback.
- `tests/security.rs`: 33 hostile file names, links in the save folder, spoofed trusted fingerprints, borrowed
  aliases, floods, unanswered prompts, oversized bodies.
- `tests/interop.rs`: independent peers. `openssl s_client` confirms the fingerprint rule; `curl` sends as a
  stock sender; `tests/fixtures/mock_localsend.py` (a LocalSend v2 peer in Python on OpenSSL, written from the
  spec, with an RSA `CA:TRUE` certificate) receives from us and sends to us.
- `tests/discovery.rs`: multicast between two nodes on the LAN interface, the HTTP scan with multicast off, and an
  announcement from the Python peer that we find and answer with `POST /register`.
- `tests/qr.rs`: page, ranges and resume, exact-length zip checked with `unzip -t` and Python `zipfile` (also with
  forced Zip64), expiry, revoke, PIN, uploads, HTTPS, and the terminal QR decoded back to the same modules.
- `crates/panoply-cli/tests/share.rs`: the real `panoply` binary, send and receive between two processes.

## Manual interop test with a phone running LocalSend

No LocalSend app can run in an automated test, so the stock-app check is manual. You need a phone (or another
computer) with the LocalSend app on the same Wi-Fi, and a build of `panoply`. Use the default port 53317 so the app
finds you. If the LocalSend desktop app runs on the same computer it owns port 53317; quit it or pass `--port`
(then the phone must be given the address by hand, since LocalSend only scans its own port).

1. **Discovery, Panoply finds the phone.** Open LocalSend on the phone (Receive tab visible). Run
   `panoply share list`. The phone appears with its alias and `mobile`. If not, run `panoply share list --scan`
   (the HTTP register scan). If the phone shows up only with `--scan`, the network drops multicast.
2. **Panoply sends to the phone.** `panoply share send "<phone alias>" photo.jpg Trip/ --hash`. Accept on the phone.
   Check the files in the LocalSend folder, including the folder tree and a Unicode file name, and compare hashes
   (`panoply hash` here, any hash app there). Try a 1 GB file for the throughput comparison.
3. **Text.** `panoply share send "<phone alias>" --text "hello ünï"`. It should arrive as a message.
4. **PIN.** In LocalSend turn on the receive PIN, send again: Panoply asks `PIN:` on a terminal, or pass `--pin`.
5. **The phone finds Panoply.** Run `panoply share receive --dir ~/Inbox`. On the phone open Send, pick files, and
   the computer appears (announced once at start, so start the receiver before opening the phone's send list or
   tap refresh). Send. The terminal shows the sender, the check code and `identity verified` (the connect-back
   check against the phone's certificate; `identity NOT verified` means the phone's server was not reachable from
   here, for example client isolation), then asks `Accept?`. Answer `y` and check the file.
6. **Both directions with the same files.** Hashes must match in both directions.
7. **Fallback.** `panoply share qr big.zip`. Scan the QR with the phone's camera app, open the link, download. Try
   `--upload-dir ~/Inbox` and upload from the phone page, `--pin 1234`, and `--https` (the browser warns once).

What to report if something fails: the phone's LocalSend version, `panoply share list --scan` output, and whether
step 5 shows `identity verified`. A TLS or fingerprint difference with a particular LocalSend release is the most
likely failure and would show up first in steps 1 and 5.
