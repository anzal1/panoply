# Format JSON, YAML and TOML (`data.format`)

Tidy, convert or check a JSON, YAML or TOML file. A mistake comes back with its line and
column. Pattern A, instant transform. Built with [serde_json](https://crates.io/crates/serde_json),
[serde_yaml_ng](https://crates.io/crates/serde_yaml_ng) and [toml](https://crates.io/crates/toml)
(all MIT or Apache-2.0).

```
panoply run modules/examples-wasm/data-format/module.toml package.json
panoply run modules/examples-wasm/data-format/module.toml docker-compose.yml --opt to=toml
panoply run modules/examples-wasm/data-format/module.toml Cargo.toml --opt check_only=true
```

## Options

| Option | Values | Default |
|---|---|---|
| `to` | `same`, `json`, `yaml`, `toml` | `same` (tidy in the file's own format) |
| `indent` | `2`, `4`, `tab`, `compact` | `2` |
| `sort_keys` | on or off | off (key order is kept) |
| `check_only` | on or off | off (on writes nothing and says "valid") |

## What to know

- Key order is kept, across all three formats.
- YAML comments are not kept: the file is read into data and written back out.
- TOML has no `null` and needs a table at the top. Converting a file that breaks either rule
  fails with the key that is in the way (`a.b is null`), and writes nothing.
- TOML dates become text in JSON and YAML.
- A YAML file with several `---` documents is refused with a message, not half converted.

## How it was made

The same five steps as `modules/template-rust/README.md`:

1. `mkdir modules/examples-wasm/data-format` and copy `Cargo.toml`, `src` and `wit` from the template.
2. Write `module.toml`: id, name, summary, `accepts` and `outputs` for the three formats, four options.
3. Write the module. The format logic is in `src/convert.rs`, which never touches the host, so it
   has 15 ordinary `cargo test` tests (real `package.json`, `Cargo.toml` and Compose inputs,
   error positions, null into TOML). `src/lib.rs` is 40 lines of glue.
4. `cargo build --release --target wasm32-wasip2` (about 25 s cold, 444 KB).
5. `panoply run` on the fixtures in `tests/fixtures/`.

## Tests

```
cargo test                                   # the 15 unit tests, in this folder
cargo test -p panoply-cli --test cli data_format    # the real module, through the sandbox
```
