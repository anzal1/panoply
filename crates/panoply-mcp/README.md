# panoply-mcp

Generates MCP tool definitions from module manifests, so every module is an MCP tool with no extra work for its author,
and serves them over stdio with a small hand-written JSON-RPC server (`server`). What a call does is a `Backend`;
`panoply mcp` in `panoply-cli` supplies the one that runs modules. See [docs/MCP.md](../../docs/MCP.md).

The tool name is the module id with dots turned into underscores
(`pdf.compress` becomes `pdf_compress`), the description is the manifest summary, and the input schema has a
`files` array, an optional `output_dir` and one property per option.

```sh
cargo test -p panoply-mcp
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
