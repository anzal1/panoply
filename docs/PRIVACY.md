# Privacy

Panoply works on your device. Your files are read and written on your own machine, by code you can read.
This page says what that means in practice for the v0.1.0 beta.

## What Panoply does not do

- It has no account, no sign-in and no cloud. There is nothing to sign up for.
- It sends no telemetry, usage statistics or crash reports. The code has no such feature.
- It does not upload your files, their names or their contents anywhere to do a job.
- It does not run in the background when you are not using it. There is no resident process after a job
  finishes, apart from the app and its hotkey listener while the app is open.

## What it does on the network

Only two things use the network.

**1. First-use downloads.** A tool that needs an engine or a model fetches it once, with your system's `curl`,
the first time you use that tool. Each download is pinned to a release or commit and checked against a
SHA-256 before it is used. A file that does not match is deleted. The size is shown before the transfer. In
the beta the download starts without a separate approval screen. The files and where they come from:

| What | From |
|---|---|
| PDF engine (PDFium) | github.com (bblanchon/pdfium-binaries) |
| ONNX Runtime | github.com (microsoft/onnxruntime) |
| OCR, background, erase and face models | huggingface.co, github.com and its media.githubusercontent.com host |
| Upscale model | qaihub-public-assets.s3.us-west-2.amazonaws.com (Qualcomm AI Hub, on Amazon S3) |
| Video and audio helper (Windows and Linux) | github.com (BtbN/FFmpeg-Builds) |

These hosts see your IP address and the file you ask for, like any download. They never see your files.
The exact addresses, versions and licenses are in [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md).
After a download, the tool works offline. To avoid the network completely, put the file in the engines
folder yourself, or point Panoply at a copy with the environment variables the tool names
(`PANOPLY_PDFIUM_LIB`, `PANOPLY_ORT_LIB`, `PANOPLY_FFMPEG`, `PANOPLY_MODEL_ISNET`, `PANOPLY_MODEL_MIGAN`,
`PANOPLY_YUNET_MODEL`).

**2. Sharing, only when you start it.** `panoply share` talks to other devices on your own network. It
announces itself once by UDP multicast (group 224.0.0.167, port 53317), serves HTTPS with a self-made
certificate that is pinned to the fingerprint it announces, and asks you before it accepts a file.
Receiving is off unless you turn it on. A QR link serves your files from your computer to one browser on your
network for ten minutes at most. Nothing goes through a server of ours, because there is none.

## What stays on your device

| What | Where |
|---|---|
| Job history (what ran, on which file, the result) | `history.jsonl` in the app data folder |
| Downloaded engines and models | The engines folder (below) |
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
