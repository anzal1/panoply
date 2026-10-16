# panoply-cli

The `panoply` command line. Short, plain output, and one command for every tool the desktop app has.

```sh
panoply modules                              # every built-in tool
panoply run image.compress photo.jpg         # run any tool by id
panoply pdf merge a.pdf b.pdf                # tool groups: pdf, image, video, audio, id, share
panoply route docx jpg                       # how one format converts to another
panoply disk ~/Downloads                     # what is using the space: biggest entries and files
panoply models list                          # the models and engines Panoply may download
panoply --help
```

`panoply disk <path>` scans a folder and lists the biggest entries at the top level (`--top N`, default 15)
and the biggest files anywhere (`--files N`, default 10). `--depth N` lists nested folders, `--cross` goes
into other drives, and `--json` prints the same result for scripts. Sizes are allocated bytes, hard links
and clones are counted once, and folders that could not be read are listed with the reason (on macOS, Full
Disk Access would let it in).

Models and engines are downloaded on first use, after asking. Pass `--yes` to say yes to all of them, for
scripts and CI. Results are new files next to the original and never overwrite anything.

Build it with `cargo build --release -p panoply-cli`. The binary is called `panoply`.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
