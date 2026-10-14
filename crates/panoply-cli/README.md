# panoply-cli

The `panoply` command line. Short, plain output, and one command for every tool the desktop app has.

```sh
panoply modules                              # every built-in tool
panoply run image.compress photo.jpg         # run any tool by id
panoply pdf merge a.pdf b.pdf                # tool groups: pdf, image, video, audio, id, share
panoply route docx jpg                       # how one format converts to another
panoply models list                          # the models and engines Panoply may download
panoply --help
```

Models and engines are downloaded on first use, after asking. Pass `--yes` to say yes to all of them, for
scripts and CI. Results are new files next to the original and never overwrite anything.

Build it with `cargo build --release -p panoply-cli`. The binary is called `panoply`.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
