# Markdown to HTML (`doc.markdown_to_html`)

Turn a Markdown file into one clean HTML page that prints straight to PDF. Pattern A, instant
transform. Built with [pulldown-cmark](https://crates.io/crates/pulldown-cmark) (MIT).

```
panoply run modules/examples-wasm/markdown-to-html/module.toml README.md
panoply run modules/examples-wasm/markdown-to-html/module.toml notes.md --opt toc=true --opt paper=Letter
```

Open the page in any browser. To get a PDF, print it and choose "Save as PDF": the page
carries its own print rules (black on white, page size, no split code blocks or tables).

## Options

| Option | Values | Default |
|---|---|---|
| `theme` | `auto`, `light`, `dark` | `auto` (follows the reader's setting) |
| `paper` | `A4`, `Letter` | `A4` |
| `title` | text | empty (the first heading is used) |
| `toc` | on or off | off |
| `page` | on or off | on (off gives only the body, to paste into your own page) |

## What to know

- Tables, footnotes, task lists, strikethrough, smart quotes and `{#custom-id}` headings work.
  Every heading gets an anchor, so links like `#install` work.
- The page is one file with its CSS inside. Images that the Markdown points to by a relative
  path stay as links: a module sees only the file it was handed, so it cannot inline them.
- The page carries a content policy that blocks every script, so a Markdown file from the
  internet cannot run code when you open the result.

## How it was made

1. `mkdir modules/examples-wasm/markdown-to-html`, copy `Cargo.toml`, `src` and `wit` from the template.
2. `module.toml`: `accepts = ["text/markdown", "*.md", "*.markdown"]`, `outputs = ["text/html"]`, five options.
3. `src/render.rs` is the converter (pulldown-cmark, two passes to give headings unique ids, the CSS).
   `src/lib.rs` is 35 lines of glue. 9 unit tests cover a full README, anchors, the contents list,
   escaping and the script policy.
4. `cargo build --release --target wasm32-wasip2` (about 20 s cold, 317 KB).
5. `panoply run` on `tests/fixtures/notes.md`, then printed to PDF from a headless browser to check the print rules.

## Tests

```
cargo test
cargo test -p panoply-cli --test cli markdown
```
