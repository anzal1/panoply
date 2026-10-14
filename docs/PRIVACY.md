# Privacy

Panoply works on your device. Your files are read and written on your own machine, by code you can read.
This page says what that means in practice for the v0.1.0 beta.

## What Panoply does not do

- It has no account, no sign-in and no cloud. There is nothing to sign up for.
- It sends no telemetry, usage statistics or crash reports. The code has no such feature.
- It does not upload your files, their names or their contents anywhere to do a job.
- It does not run in the background when you are not using it. There is no resident process after a job
  finishes, apart from the app and its hotkey listener while the app is open.

## What talks to the network, and when

Three things can, and nothing else does. Each is off until you do something that needs it.

### 1. Downloads of models and engines, only after you say yes

Some tools need an engine or a model that is not in the installer (it keeps the installer small). Panoply
downloads each one once, the first time a tool needs it, and only after you have allowed that item. The
question names the item, its size, where it comes from and its license:

- **Desktop app.** A sheet in the tool's panel: "Download the IS-Net background model?" with what it is for,
  the size, the host and the license. Return downloads, Esc says not now. Nothing is fetched and nothing is
  written until you press Download. Not now goes back to the options with nothing changed.
- **Command line.** A prompt on the terminal: `Download it? [y/N]`. `--yes` says yes to every download
  (`PANOPLY_YES=1` does the same for scripts and agents). Without `--yes` and without a terminal, the command
  stops with a plain error that says what it needs and how to allow it, and downloads nothing.
- **Ahead of time.** `panoply models` lists every item with its size, license and state. `panoply models allow
  isnet` records a yes without downloading. `panoply models fetch isnet` downloads now.

Your yes is remembered per item in `consent.json` in the app data folder, so you are asked once for each item
and never for one you already allowed. A no is not remembered; the next job asks again. The Modules page in
the app, and `panoply models`, list what is on this computer with sizes. Remove (or `panoply models remove
isnet`) deletes the files and forgets the yes, so the next use asks again.

Tools that can download say so before they run: the manifest declares network access with a reason, the app
shows it on the tool's panel, and the command line prints it. A tool that did not declare network access is
refused any download, even one you allowed for another tool.

Each download is pinned to a release or commit and checked against a SHA-256 before it is used. A file that
does not match is deleted. The system `curl` does the transfer, over HTTPS only.

| Item (id) | What it is for | Size | From | License |
|---|---|---|---|---|
| PDF engine (`pdfium`) | Drawing PDF pages, JPEG 2000 and JBIG2 pictures | 3.5 MB | github.com (bblanchon/pdfium-binaries) | BSD-3-Clause, Apache-2.0 |
| ONNX Runtime (`onnxruntime`) | Runs the small models below | 8 to 33 MB, 79 MB on Windows | github.com (microsoft/onnxruntime) | MIT |
| IS-Net (`isnet`) | Background removal where the system has no subject finder | 44 MB | huggingface.co | MIT |
| MI-GAN (`migan`) | Erase objects, remove watermarks | 28 MB | huggingface.co | MIT |
| Real-ESRGAN (`esrgan`) | Upscale | 4.5 MB | qaihub-public-assets.s3.us-west-2.amazonaws.com (Qualcomm AI Hub, on Amazon S3) | BSD-3-Clause |
| YuNet (`yunet`) | Faces for passport and ID photos where the system has no finder | 233 KB | github.com and media.githubusercontent.com (opencv/opencv_zoo) | MIT |
| PP-OCR models (`ppocr-det`, `ppocr-rec-en`, `ppocr-rec-latin`, `ppocr-rec-devanagari`) | Text recognition | 5 to 8 MB each | huggingface.co | Apache-2.0 |
| Tesseract language data (`tessdata-ben`, `tessdata-eng`, `tessdata-hin`) | Bengali and Hindi text | 1 to 4 MB each | github.com (tesseract-ocr/tessdata_fast) | Apache-2.0 |
| Video and audio helper (`ffmpeg`, Windows and Linux) | Reading and writing video and audio | 117 to 172 MB | github.com (BtbN/FFmpeg-Builds) | LGPL-3.0 |

These hosts see your IP address and the file you ask for, like any download. They never see your files.
The exact addresses and versions are in [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md). On macOS the
system's own engines (Apple Vision, and an ffmpeg you already have) are used first, so most of these are never
downloaded there. To avoid the network completely, put the file in the engines folder yourself, or point
Panoply at a copy with the variables the tool names (`PANOPLY_PDFIUM_LIB`, `PANOPLY_ORT_LIB`,
`PANOPLY_FFMPEG`, `PANOPLY_MODEL_ISNET`, `PANOPLY_MODEL_MIGAN`, `PANOPLY_MODEL_ESRGAN`, `PANOPLY_YUNET_MODEL`).

### 2. Sharing with devices on your network, only when you start it

`panoply share` (and Nearby, when it ships in the app) talks to other devices on the same local network.

- It announces itself by UDP multicast (group 224.0.0.167, port 53317) on private networks only, and skips
  VPN and virtual adapters.
- It serves HTTPS on port 53317 with a self-made certificate pinned to the fingerprint it announces, and asks
  you before it accepts a file. Receiving is off unless you turn it on.
- It only connects to, and only accepts connections from, addresses on your own network: loopback, private
  (10.x, 172.16 to 31.x, 192.168.x), link-local, and the IPv6 equivalents. An internet address typed by hand
  is refused, and a connection from the internet is dropped.

### 3. The QR page, only when you ask for one

`panoply share qr` serves your files from your computer, over HTTP on your local network address, to one
browser on the same network. It is protected by a one-time token, closes after the download, and in any case
after ten minutes. The same local-network rule applies.

### Nothing else

There is no update check, no telemetry, no crash reporting, no license check and no cloud. Community modules
run in a sandbox that has no network: a module that imports sockets or HTTP is refused when it loads.

How this was checked (repeat it before a release):

- Search the source for network use: `grep -rnE 'reqwest|ureq|TcpStream|TcpListener|UdpSocket|hyper|std::net|tokio::net|curl' crates apps`.
  Every hit is one of: the `curl` download sites (pdf, media, id, ocr and vision crates, each behind the
  consent gate), `panoply-share` (LocalSend and the QR page), or a test.
- `cargo tree -i hyper` (and `rustls`, `socket2`, `if-addrs`, `rcgen`) shows only `panoply-share` and the
  command line. The desktop app does not depend on `panoply-share`. The only network crate it carries is
  `tokio`, through the WebAssembly runtime, whose sockets and HTTP interfaces are refused at module load.
- A test (`crates/panoply-host/tests/trust.rs`) runs every module with an empty engines folder and no
  consent and checks that nothing is downloaded, and that a module without a network declaration is refused.

## What stays on your device

| What | Where |
|---|---|
| Job history (what ran, on which file, the result) | `history.jsonl` in the app data folder |
| Downloaded engines and models | The engines folder (below) |
| Which downloads you allowed | `consent.json` in the app data folder |
| Your own modules | The `modules` folder in the app data folder |
| Share identity (certificate and key) and the trust list | The share data folder, key file mode 0600 |

Folders: macOS `~/Library/Application Support/Panoply` and `~/Library/Caches/Panoply/engines`. Windows
`%APPDATA%\Panoply` and `%LOCALAPPDATA%\Panoply\cache\engines`. Linux `$XDG_DATA_HOME/panoply` and
`$XDG_CACHE_HOME/panoply/engines`. `PANOPLY_DATA_DIR` and `PANOPLY_ENGINES_DIR` move them. Delete a folder to
erase what is in it. History holds file paths, so clear it if the paths are sensitive.

## On-device models and your files

Image AI and OCR run models on your machine through ONNX Runtime. Your picture is passed to the model in
memory and is not written anywhere else. On macOS Panoply uses Apple Vision first, which is part of your
system and runs on the device.

## Metadata tools

Convert keeps metadata by default and tells you what it kept and dropped. Strip metadata removes GPS, or
private fields, or everything, and zeroes what it removes. Check the result card: it lists GPS whenever
it is kept.

## Questions and reports

Privacy questions and security reports: see [SECURITY.md](../SECURITY.md).
