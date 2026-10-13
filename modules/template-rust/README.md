# Panoply module template (Rust)

A Panoply module is a small program that turns the files a user chose into a result.
You write one Rust function. Panoply gives you the screen, the CLI command, the MCP tool,
the right-click item, progress, cancel and a sandbox. Your module runs as WebAssembly: it
can read only the files the user handed it, write only new files in the output folder, and
cannot reach the network.

This folder is a working module (it uppercases a text file). Copy it, change two files,
build, run.

## Steps

You need Rust from https://rustup.rs. One time only:

```
rustup target add wasm32-wasip2
```

1. **Copy the template** into a new folder anywhere. Inside the Panoply clone, put it
   under `modules/mine/` (create the folder first, `cp` will not). Copy these five things
   only, not this README and not any `target` folder:

   ```
   mkdir -p modules/mine/my-module
   cp -r modules/template-rust/{Cargo.toml,module.toml,src,wit,.gitignore} modules/mine/my-module/
   cd modules/mine/my-module
   ```

   The package name in `Cargo.toml` can stay as it is: it does not matter.

2. **Describe it in `module.toml`.** Change `id` (lowercase words joined by dots, letters,
   digits and underscores only, like `text.uppercase`), `name`, `pattern`, `summary`,
   `accepts` and `outputs`.

   - `accepts` is what it takes, and `outputs` is what it produces, both as MIME types
     (`text/*`, `application/json`) or extensions (`*.csv`). `outputs = []` for a module
     that writes no files. A file is accepted if any one entry matches it: its type, which
     Panoply works out from the file's bytes and falls back to its extension, or an `*.ext`
     entry. One entry is enough (`accepts = ["*.csv"]` takes any `.csv`). The type of a
     `.csv` is `text/csv`.
   - `pattern` is one letter (all nine are in `docs/FLOWS.md` in the Panoply clone). `A` is
     an instant transform: file in, new file out, runs right away. `I` is an inspector: file
     in, a report out, nothing written. Others, for later: `B` tuned transform (a few dials
     and a preview), `E` generator (no file in), `F` instant answer in the bar. `A` and `I`
     cover most modules.
   - Delete the `[options.separator]` block if you have no options.

3. **Write the module in `src/lib.rs`.** Replace the body of `run`. The whole contract is
   below.

4. **Build it.**

   ```
   cargo build --release --target wasm32-wasip2
   ```

   This writes `target/wasm32-wasip2/release/module.wasm`. Panoply finds it there by
   itself, whatever you named the package.

5. **Run it.** `panoply run <module.toml> <files...>`. Every path is relative to the
   folder you run the command from.

   ```
   panoply run module.toml notes.txt
   panoply run module.toml notes.txt --opt separator=";"
   panoply run module.toml *.txt --out ~/Desktop/results
   ```

   If you have no `panoply` command, go to the root of the Panoply clone and use
   `cargo run -p panoply-cli --` in its place (the first time, cargo builds Panoply, which
   takes a few minutes). Now both the module and the file are relative to the clone's root:

   ```
   cd /path/to/panoply
   cargo run -p panoply-cli -- run modules/mine/my-module/module.toml /path/to/notes.txt
   ```

   The new file appears next to the original (or in `--out`) as `notes (uppercase).txt`.
   It never overwrites: a second run makes `notes (uppercase 2).txt`.

That is the whole loop. Edit, build, run.

## What you write

```rust
wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> { ... }
}

export!(Module);
```

`run` gets the files the user chose and the options as a JSON text, with every default from
`module.toml` already filled in. A module with no options just ignores it: name it `_options`. It returns the list of results, or an error message that is
shown to the user as written, so say what went wrong and what to do next.
`InputFile` and `Output` come from the `generate!` line, you do not import them.

An `InputFile` has `handle` (what you read with), `name`, `mime` (found from the file's
bytes) and `size`. You never get a path.

An `Output` is `Output::File(name)` for a file you wrote, or `Output::Note(line)` for one
line of a report (pattern I modules answer with notes).

## What you can call (`host::...`)

| Call | What it does |
|---|---|
| `read_all(handle) -> Result<Vec<u8>, String>` | The whole file. Fails over 64 MB: use `read` for big files. |
| `read(handle, offset, len) -> Result<Vec<u8>, String>` | Up to `len` bytes from `offset` (at most 8 MB a call). Empty at the end. |
| `save(Some(handle), "label", "ext", &bytes) -> Result<String, String>` | Write one output file. Named `<input name> (<label>).<ext>`, so `save(.., "as json", "json", ..)` on `people.csv` makes `people (as json).json`: pick a label that says what changed. Never overwrites. Returns the name, which you put in `Output::File`. Pass `None` instead of `Some(handle)` if there is no input file. |
| `begin_output(Some(handle), "label", "ext")`, `append_output(h, &bytes)`, `finish_output(h)` | The same, in pieces, for big outputs. Memory stays flat. |
| `progress(fraction, "label")` | Show progress, 0.0 to 1.0. Shown for jobs over 300 ms. |
| `cancelled() -> bool` | True once the user cancelled. Check it between chunks and return early. |

Nothing else exists. There is no file system, no network and no environment. A call that
fails returns a plain-words `Err(String)`, and you can pass it straight up with `?`.

If a job fails or is cancelled, Panoply deletes every file the job wrote, so you never leave
half a result behind.

## Use crates, keep it short

A module is usually 10 to 40 lines, because a crate does the hard part. Any pure Rust crate
that builds for `wasm32-wasip2` works: add it with `cargo add`, like `cargo add csv` for CSV
files, `regex`, `pulldown-cmark` for Markdown, `unicode-normalization`. Before you write a
parser, look for a crate.

A few small tips:

- `serde_json` writes object keys in alphabetical order unless you turn on its
  `preserve_order` feature. The template already depends on `serde_json`, so change that
  line with `cargo add serde_json -F preserve_order` (do not paste a second `serde_json`
  line into `Cargo.toml`).
- `serde_json::to_string_pretty` gives readable JSON.
- Text from Excel can begin with an invisible byte order mark. Strip it with
  `text.trim_start_matches('\u{feff}')`.

## Limits

- 256 MB of memory per run. Stream big files with `read` and `begin_output`.
- A module that never checks `cancelled()` is still stopped within about 10 ms of cancel.
- Nothing stays loaded between runs.
- Heavy work (PDF, images, video, OCR) is done by Panoply engines, not inside your module.
  Keep your module to logic and glue.

## If something goes wrong

| You see | Do this |
|---|---|
| `can't find crate for std` | Run `rustup target add wasm32-wasip2`. |
| `the compiled module is missing at ...` | Run the build step, from the folder that holds `Cargo.toml`. |
| `The module id "..." is not valid` | Use lowercase letters, digits, underscores and dots, like `text.my_tool`. |
| `... is a image/png file, and ... works on text/*` | The file is not in `accepts`. Add its type, or run it on a different file. |
| `"x" is not an option` | Declare it in `module.toml` under `[options.x]`. |
| `the module crashed and said: ...` | That is your panic message. Fix it and rebuild. |
| `it asks for the network` | Your code or a dependency opens a socket. Panoply modules cannot. |

## Examples

`modules/examples-wasm/` has two finished modules built from this template:
`slugify-lines` (pattern A) and `word-count` (pattern I).
