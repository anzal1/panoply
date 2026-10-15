# Continuous integration

Every push to `main` and every pull request runs `.github/workflows/ci.yml`. The repository is public, so GitHub-hosted runners cost nothing. This page says what runs, where, and why the jobs are split the way they are.

## The jobs

| Job | Runs on | What it checks | Runs when |
| --- | --- | --- | --- |
| `changes` | Linux | Lists the changed files and decides which of the jobs below are needed (`tools/ci/changes.py`) | always |
| `docs` | Linux | Every relative link in the Markdown files resolves (`tools/ci/check_docs.py`) | always, takes seconds |
| `fmt` | Linux | `cargo fmt --all --check` | Rust code changed |
| `clippy` | Linux, macOS, Windows | `cargo clippy --workspace --all-targets -D warnings` with the cfg code of each OS | Rust code changed |
| `test` | Linux, macOS, Windows | The tests, see the table below | Rust code changed |
| `licenses`, `advisories` | Linux | `cargo deny`: licenses, bans, sources; advisories | Rust code or a manifest changed, and every Monday |
| `notices` | Linux | `THIRD-PARTY-NOTICES.md` matches `Cargo.lock` | code, `Cargo.lock`, `tools/notices` or the file itself changed |
| `conversions` | Linux | The conversion graph is regenerated and unchanged | code or `conversions/` or `docs/CONVERSIONS.md` changed |
| `ci` | Linux | One name that is green when every job above passed or was skipped | always |

A push that only changes documentation (Markdown outside `src/` and `tests/` folders, anything under `docs/`, licenses, issue templates) skips every Rust job and finishes in well under a minute. A change to the workflow itself, to `Cargo.toml` or to anything that is not clearly documentation runs everything. When the changed-file list cannot be read, everything runs. A scheduled run (Mondays) and a manual run always run everything, which is also how a new advisory against an untouched dependency shows up.

A push to `main` is never cancelled by a later push, so every commit on `main` gets its result. A pull request or branch run is cancelled when a newer push arrives.

## Which tests run where

The tests are built once per OS with the `ci` profile (see below) and run by [cargo-nextest](https://nexte.st), which runs every test as its own process and runs the processes side by side. Doc tests run with `cargo test --doc`. nextest profiles live in `.config/nextest.toml`.

| Set | Linux | macOS | Windows | What is in it |
| --- | --- | --- | --- | --- |
| Core | yes | yes | yes | Every unit test and integration test not listed below: about 1,040 tests, including the WebAssembly sandbox tests, the CLI tests, the sharing tests and all corpus and model tests that finish in under ten seconds |
| Heavy | yes | no | no | The slowest corpus and model tests (ten seconds or more on a Linux runner), 16 tests, listed in the `ci-core` profile: the PDF and image size-target searches, the long OCR file, the 400 and 120 megapixel memory runs, the passport rule over every country, the largest vision model runs |
| macOS only | no | yes | no | `panoply-platform-macos` (ImageIO, Apple Vision memory loops). These are never in the heavy list, because they would run nowhere |
| Timing | yes | yes | yes | `panoply-bench`, `panoply-ocr` idle, `panoply-vision` idle and `panoply-share` discovery and interop: tests that measure how quiet a process is, or start network servers and scan against a short deadline. They run alone, one at a time, after everything else, so the measurement does not share the machine with a thousand other tests |
| Doc tests | yes | yes | yes | `cargo test --doc` (none exist today; this keeps it that way when one is added) |

The heavy list is the `ci-core` profile's filter in `.config/nextest.toml`. To move a test, edit that filter. A test that exists on one OS only must stay out of it.

Corpus tests run on every file that is present. The corpus (pictures and PDFs) is fetched on all three OSes and cached by the hash of the manifests in `tests/corpus`, so a run only downloads when a manifest changes.

## Why it is fast

* **Path filters.** Documentation pushes skip the Rust jobs (above).
* **Split jobs.** `fmt` finishes in seconds and clippy runs beside the test build instead of before it. On Linux, macOS and Windows alike the tests start when the build ends, and clippy is no longer on the way.
* **`ci` profile** in `Cargo.toml`: the dev profile with no debug info, no incremental state and stripped standard library symbols. Smaller objects link faster and make a smaller cache to restore. The optimization levels are repeated per package because Cargo does not inherit them between profiles. Keep the two lists in step.
* **nextest** instead of `cargo test`: test binaries run side by side instead of one at a time, and the log shows per-test timing for the slow ones.
* **Caches.** `Swatinem/rust-cache` with a key per OS and job; the corpus by manifest hash; the engines and models the tests download (pdfium, ONNX Runtime, the small models) by the hash of the files that pin them (the checksum is still verified on every use); the small WebAssembly modules some tests build, by the hash of their sources. On a cold engine cache everything is downloaded once, one after another, before the tests start, so no test sees a download in progress. The Rust caches are written only by runs on `main`, so a pull request starts from what `main` built (the first run after `Cargo.lock` changes is cold, the next ones are not).
* **Fewer packages on Linux.** The system libraries for the desktop app are installed without `apt-get update` unless a package has moved on.

The linker is not changed: Rust has linked with `lld` on Linux since 1.90, so `mold` would not add anything.

## Changing CI

* Run `tools/preflight.sh` before every push: the same fmt, clippy and nextest checks, for the crates you changed and the crates that depend on them.
* Run `tools/preflight-linux.sh` (or `tools/preflight.sh --linux`) when you touch Linux-only code: see "The local Linux check" below.
* Run `python3 -I tools/ci/changes.py --files < list.txt` to see which jobs a list of changed files would need.
* Run `python3 -I tools/ci/check_docs.py` before pushing documentation.
* Actions are pinned to a commit SHA with the version in a comment, like `release.yml`. Dependabot proposes updates monthly.

## The local Linux check

`tools/preflight-linux.sh` runs the Linux half of CI on a Mac, so a Linux-only compile or lint error costs minutes instead of a CI round trip.

* **What runs.** `cargo clippy --workspace --exclude gpui-base --all-targets -- -D warnings` (the `clippy (ubuntu-latest)` job), then nextest with the `ci-core` profile and the `ci-idle` timing tests on the crates changed since `origin/main` and the crates that depend on them (the logic of `tools/preflight.sh`, shared in `tools/ci/changed_crates.py`). `--all` takes the whole workspace, `--heavy` adds the heavy tests, `--plan` prints the scope. Formatting is left to the macOS half.
* **Where.** A colima VM (profile `panoply`, Apple Virtualization, native arm64, 4 CPUs, 4 GB; your own default VM is not touched) runs the image of `tools/linux/Dockerfile`: Ubuntu 24.04, the toolchain of `rust-toolchain.toml`, the apt packages of `.github/actions/linux-libs` (keep the two lists in step), cargo-nextest, and Xvfb, openbox and xdotool for the smoke. The repo is mounted read-only and synced to a work volume with `rsync -a`, which keeps modification times, so cargo rebuilds only what changed. Named volumes keep the cargo registry, the Linux target folder and the downloaded engines warm between runs. The image is rebuilt when the Dockerfile or `rust-toolchain.toml` change, or with `--rebuild` (a newer stable Rust).
* **Cost.** Everything runs at `nice -n 15` with 8 cargo jobs (`PANOPLY_LINUX_JOBS`) on the 4 VM CPUs. The VM is stopped with `colima stop panoply` when the check ends (`--keep` skips that) and your docker context is put back. No window is ever opened on the Mac.
* **One heavy check at a time.** `tools/preflight.sh` and `tools/preflight-linux.sh` take a machine-wide lock (`tools/lib/heavy-lock.sh`, a directory made with `mkdir` that holds the holder's PID, at `~/personal/.panoply-heavy.lock`) before they build. A second one prints "waiting for another check" and starts when the first ends. A lock whose process is gone is taken over.
* **`--smoke`.** Builds `panoply-desktop` with the `ci` profile and runs it under Xvfb and openbox with software Vulkan (`tools/linux/smoke.sh`, after `.github/scripts/desktop-smoke-linux.sh`, which tests the released AppImage). It fails when no window titled Panoply becomes visible, when the 720 by 460 command bar is mapped before the hotkey, when the app panics or does not exit when its window is closed. Screenshots (xwd and png, before and after the hotkey) and `report.json` land in `~/.cache/panoply/linux-smoke`.
* **Automatic.** `tools/preflight.sh` runs it after the macOS checks when a changed `.rs` file contains `target_os = "linux"` or is a `platform/other.rs` (vendored `patches/` do not count); `--linux` forces it, `--no-linux` skips it.
* **What it does not cover.** CI is x86_64; the VM is aarch64, so an error that depends on the instruction set can still slip through. The engines and models the tests download are fetched by the tests themselves instead of once up front as in CI.
