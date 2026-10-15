# QR code maker (`image.qr_code`)

Turn the links or text in a file into QR codes, one per line, as PNG or SVG. Pattern A,
instant transform. Built with [qrcode](https://crates.io/crates/qrcode) (MIT or Apache-2.0) and
[png](https://crates.io/crates/png) (MIT or Apache-2.0).

```
panoply run modules/examples-wasm/qr-code/module.toml links.txt
panoply run modules/examples-wasm/qr-code/module.toml contact.txt --opt format=svg --opt lines=whole_file
```

A file with three lines makes `links (qr 1).png`, `links (qr 2).png` and `links (qr 3).png`.
A one-line file makes `links (qr).png`. Anything works that fits in a code: a link, plain text,
a Wi-Fi sign-in (`WIFI:T:WPA;S:name;P:password;;`), a contact card (a whole file, with
`lines=whole_file`).

## Options

| Option | Values | Default |
|---|---|---|
| `format` | `png`, `svg` | `png` (use `svg` for print) |
| `lines` | `each_line`, `whole_file` | `each_line` |
| `error_correction` | `L`, `M`, `Q`, `H` | `M` |
| `scale` | 2 to 40 | 10 (PNG pixels per square) |
| `border` | 0 to 16 | 4 (squares of quiet border) |
| `dark`, `light` | hex colours | `#000000`, `#ffffff` |

## What to know

- A colour pair a phone could not read (light squares on dark, or too little contrast) is refused
  with a plain message before anything is written.
- Too much text for one code says so, with the line number when the file has several.
- A run makes at most 500 codes.
- The PNG is indexed (two colours), so a code is about 1 KB.
- Generators with no input file (pattern E) do not run in the sandbox yet, so the input is a text
  file. That fits batches well: one link per line.

## How it was made

1. `mkdir modules/examples-wasm/qr-code`, copy `Cargo.toml`, `src` and `wit` from the template.
2. `module.toml`: `accepts = ["text/*"]`, outputs PNG and SVG, seven options.
3. `src/make.rs` does the encoding, the SVG, the PNG and the colour checks (7 unit tests, one of
   them decodes its own PNG). `src/lib.rs` is the loop over files and lines.
4. `cargo build --release --target wasm32-wasip2` (about 20 s cold, 193 KB).
5. `panoply run` on `tests/fixtures/links.txt`, then decoded the PNGs and SVG with the macOS
   barcode reader (Vision) to prove a phone can read them.

Time: all three community plugins (this one, `markdown-to-html`, `qr-code`), from an empty folder to
a commit with READMEs, tests and a CI job, took about 10 minutes of wall time for an AI agent
working only from `modules/template-rust/README.md`. That includes downloading the crates and
two rounds of fixes found by running the real thing. A person new to Rust should plan for an
afternoon.

## Tests

```
cargo test
cargo test -p panoply-cli --test cli qr_code
```
