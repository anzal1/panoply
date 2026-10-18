# Known limitations

This is the v0.1.0 beta. It does 31 tools well and leaves others out on purpose. This page says which, so
you do not have to find out by trying. Items move off this page as they ship (see the
[changelog](../CHANGELOG.md)).

## Opening an unsigned build

Panoply is not code signed in the beta. That does not change what the app does. It means macOS and
Windows cannot tell who built it, so each asks you once to confirm. Check your download against
`SHA256SUMS` on the releases page first. Every release file also has a GitHub build provenance attestation:
`gh attestation verify FILE --repo anzal1/panoply`.

### macOS

macOS 15 and later:

1. Drag Panoply to Applications and double-click it. macOS says it cannot verify the app. Click Done.
2. Open System Settings, then Privacy and Security.
3. Scroll to the Security section. Next to "Panoply was blocked to protect your Mac", click Open Anyway.
4. Enter your password or use Touch ID, then click Open Anyway in the confirmation.

On macOS 14 and earlier you can instead right-click (or Control-click) Panoply, choose Open, then Open.

If you prefer the terminal, this removes the quarantine flag and has the same effect:

```bash
xattr -dr com.apple.quarantine /Applications/Panoply.app
```

You do this once per download. The `panoply` binary from the separate `panoply-cli` archive is quarantined
too. Run `xattr -d com.apple.quarantine ./panoply` on it, or use the same Privacy and Security steps.

### Windows

Windows SmartScreen may show "Windows protected your PC" for the installer or for `panoply-desktop.exe`.
Click More info, then Run anyway. If you downloaded the zip, you can also right-click the zip, choose
Properties, tick Unblock, and unzip again.

### Linux

No signature check applies. Make the AppImage executable (`chmod +x`) and run it.

## Not in the beta yet

- **Most of the plan.** About 240 of the 270 specced modules are not built: the clipboard, snippets, the
  calculators, developer tools, whole-disk search, screen capture, dictation, translation, the office
  converters and the rest. [docs/MODULES.md](MODULES.md) is the full list.
- **The universal converter runs only what the tools do.** The conversion graph knows 214 formats and
  9,214 routes. The beta runs the conversions that its image, audio, video and PDF tools cover. Other
  routes are planned, not executed.
- **The MCP server.** Every tool has an MCP definition, but the transport that serves them to an AI app
  is not built. The command line works for agents today.
- **Sharing in the desktop app.** The Share page sends to and receives from Panoply and LocalSend apps.
  Receive is off every time Panoply starts and is not remembered, on purpose. It works only while the
  Panoply window is open: closing the window stops receiving. The four word check code
  appears only between two Panoply apps; a LocalSend app does not show one.
- **Image output.** No animated GIF, WebP or AVIF output. Animated inputs use the first frame and say so.
  No SVG, no JPEG XL decode, no multi-page TIFF export.
- **Video.** No software HEVC or HEIC encoder, by design (x265 is GPL and HEVC carries patents). Video
  encoding uses your hardware encoder, or SVT-AV1 and VP9 in software. HDR to SDR is a static mapping and
  refuses Dolby Vision profile 5. Repair cannot rebuild sound from a file with no index.
- **PDF.** Encrypted files are refused, not decrypted. Compress leaves CCITT and JBIG2 scans alone, and it
  cannot shrink a scan that is already tightly coded: it says so and keeps the original when the saving
  is under 2%.
- **OCR.** Page rotation is not detected, so a sideways page reads as it is drawn. Arabic, Cyrillic, Thai
  and CJK read through Apple Vision on macOS only. There is no table detection and no handwriting mode.
- **Text to speech.** macOS and Windows only, on the voices the system has installed (no download, nothing
  played: it writes a file). The first version writes WAV; M4A and MP3 come next, through the shared media
  engine. Linux hides the tool: no permissively licensed speech engine ships with Linux, and the open ones
  (Piper, Kokoro) turn text into sounds with espeak-ng, which is GPL-3.0, so neither can be in Panoply. A
  voice or engine with a clean licence and its own phonemes would bring it back. Pauses follow the
  punctuation the system voice already follows, and numbers, dates and abbreviations are read the way the
  system voice reads them. Speed changes through the system voice's own rate, so the result is close to the
  number you pick, not exact. A PDF is read from its own text layer (run PDF OCR first on a scan). Markdown loses its markup, HTML tags are dropped and table rows are read as comma separated cells, but code blocks are skipped and nothing else is interpreted.
- **Face-aware crop.** Resize to exact KB crops around the center, or the top for portraits
  (`anchor=top`). It does not look for the face yet.
- **Wayland.** There is no global hotkey on Wayland. Use the app window, or bind a key in your desktop
  settings to start `panoply-desktop`.
- **Package managers.** No Homebrew, Scoop, winget or Flathub package yet. Download from the releases page.

## Differences between systems

| | macOS | Windows | Linux |
|---|---|---|---|
| HEIC, HEIF, JPEG XL, JPEG 2000, OpenEXR, camera RAW in | Yes, through the system | Not yet. Panoply says so in a plain sentence | Not yet |
| HEIC out | Yes, through the system | No | No |
| Video and audio engine | An `ffmpeg` and `ffprobe` you install (`brew install ffmpeg`) | Downloaded once, about 170 MB (x64) | Downloaded once |
| Hardware video encoding | VideoToolbox | Media Foundation, NVENC, QSV, AMF | VA-API, NVENC, QSV, AMF |
| Remove background | Apple Vision first, IS-Net as a fallback | IS-Net | IS-Net |
| Faces for passport photos | Apple Vision | YuNet model, downloaded once | YuNet model, downloaded once |
| OCR | Apple Vision, then PP-OCRv5 and Tesseract | PP-OCRv5 and Tesseract | PP-OCRv5 and Tesseract |
| Text to speech | System voices (AVSpeechSynthesizer) | System voices (Windows.Media.SpeechSynthesis) | Hidden (no clean engine) |
| Undo | New file to the Trash | New file to the Recycle Bin | New file to the Trash |
| Global hotkey | Option+Space | Ctrl+Alt+Space | Ctrl+Alt+Space on X11 |

More about the gaps:

- **HEIC off macOS.** Windows and Linux builds answer a HEIC file with "HEIC needs the HEIC helper on this
  system" and what to do meanwhile (convert it on your phone, or use a Mac). Windows will read HEIC through
  the HEIF Image Extensions from the Microsoft Store once that route is built. Where the system has no
  decoder, a separate helper program will do it. That helper is a download you are asked about first.
- **Windows and Linux testing.** The test suite runs on all three systems in CI. The system-specific
  engines (Windows OCR, DirectML, the hardware video encoder argument sets, VA-API) have had much less
  hands-on use than the macOS ones. Expect rough edges and please report them.
- **Intel Macs.** There is no ONNX Runtime download for macOS on Intel. Image AI that needs a model, and
  the OCR fallbacks, need `PANOPLY_ORT_LIB` pointed at an ONNX Runtime 1.28.3 library. Apple Vision
  covers Latin text and background removal without it.
- **Tesseract.** Bengali OCR needs a `tesseract` program on your PATH (or `PANOPLY_TESSERACT`). Panoply does
  not install it.

## Things that behave in a way you might not expect

- **Watermark removal sometimes needs a box.** Panoply finds text-like watermarks (white or black letters
  over a picture) by itself. It does not find logos, patterned marks or anything that looks like part of
  the picture, and it never fills in a face (a mark across a face is left on the face). A large pale mark
  across a busy picture can be missed; it then says nothing was found. When it finds nothing or the wrong
  thing, name the area yourself with `panoply unmark
  --rect x,y,w,h`, or use Erase object on it. The desktop panel has no box-drawing tool yet. It only runs on your own pictures: it asks you to confirm that you own them. It
  fills the area with a model's guess, which looks right but is not the original.
- **First use downloads something.** The PDF engine, ONNX Runtime, each model and the video helper are
  fetched the first time a tool needs them. In the beta the download starts without a separate approval
  screen, and the size is shown in the progress line. See [PRIVACY.md](PRIVACY.md).
- **Some tools use a lot of memory.** Remove background (IS-Net) peaks near 750 MB, erase near 600 MB and
  up to about 1.2 GB on a large picture, remove watermark up to about 900 MB, OCR on a PDF up to about
  550 MB when the Hindi and Bengali fallback starts, and software AV1 video compression near 1.3 GB at
  1080p. A 33 megapixel HEIF photo or a camera RAW file can take 0.4 to 1.6 GB in any picture tool,
  because the system's decoder holds it. Everything else stays under 300 MB. Pictures over 24
  megapixels are processed at a reduced size for cutout, erase and watermark, and the result card says
  so. On macOS, OCR reads pictures and pages at no more than 6 million pixels (A4 at about 250 dpi).
- **The disk map counts what a file costs on disk.** Sizes are allocated bytes, so a sparse or compressed file
  shows what it occupies, a hard link is counted once, and an APFS clone is counted once. A clone you edited
  after copying it is found exactly only for files of 1 MiB or more; a small edited clone can be counted
  twice for the blocks it still shares, under 1 MiB per file. Other volumes and network drives are skipped
  unless you ask. Without Full Disk Access on macOS some folders in your Library cannot be read: the map
  says which, and why. A scan of a whole home folder holds about 290 MB while it runs.
- **The disk map will not move folders the system needs.** Drive roots, the home folder and what holds it,
  `/System`, `/Library`, `/Applications`, `~/Library` (except cache folders inside it), `C:\Windows`,
  `Program Files`, `AppData` and `/usr`, `/etc`, `~/.config` on Linux have a locked box that says why. The same
  list sits under the Trash call itself. A few cache folders inside a protected one can be moved, with a note.
- **Undo after Move to Trash puts the files back where they were.** It works on macOS and Linux, where the
  system tells Panoply where the Trash put each item. On Windows the Recycle Bin gives no location, so Undo
  says so and you restore from the Recycle Bin yourself. If something new already sits at the old path, that
  file is left in the Trash and listed.
- **Compress video picks a hardware encoder first.** Hardware encoders are fast and less efficient than
  x264 at tight sizes. Pass `--codec av1` when you need the smallest file and have time.
- **A tool that cannot reach a target says so.** "Under 1 MB" on a scan may produce a bigger file than
  asked. Panoply writes the smallest it reached and tells you.
- **Results are new files.** They sit next to the original, such as
  `name (compressed).jpg`, numbered on a clash. Nothing is ever overwritten.

## Instant search (Find)

- **macOS** keeps the file list current with FSEvents, in about 220 ms, at no cost while nothing changes.
- **Linux** uses inotify. The kernel caps watches per user (`fs.inotify.max_user_watches`). On a very large
  tree Panoply stops adding watches at the cap, says so once on stderr, and folders past it are not
  watched until you raise the limit and restart the app. After a kernel queue overflow Panoply scans
  your folders again, so a burst of changes can show up a moment late. It does not poll.
- **Windows** watches with `ReadDirectoryChangesW`, one reader per folder root, and scans again after a
  buffer overflow. Between one read finishing and the next starting there is a short gap, and a change in
  it can be missed until the next scan. Searching is still instant: the list is in memory either way.
- **Windows USN journal is off by default.** It would watch a whole volume with no scan after a restart,
  but it was written and reviewed on a Mac and has not run against a real Windows volume, and it needs an
  elevated process. It is opt in with `PANOPLY_FIND_USN=1`. Any journal problem (a different file system,
  a wrapped journal, a damaged record, missing rights) falls back to `ReadDirectoryChangesW` and a scan,
  never to a stale list. Its position is not saved between runs yet.
- **First start reads your files once.** Until that ends, the Find page shows how many files it has read
  and no match count. The list is saved, so later starts are quick.

## Reporting a problem

Open an issue at https://github.com/anzal1/panoply/issues with your system, the tool and the sentence
Panoply showed. Security problems go through [SECURITY.md](../SECURITY.md).
