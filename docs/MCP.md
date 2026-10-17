# Use Panoply from an AI agent (MCP)

`panoply mcp` turns every installed Panoply tool into an [MCP](https://modelcontextprotocol.io) tool, so an
agent such as Claude Code, Claude Desktop or Cursor can merge PDFs, compress images, trim video and the rest
on your own computer. It runs locally over stdio. Nothing is uploaded, and the agent only touches the files
it names.

You need the `panoply` command line on your PATH. Check with `panoply version`.

## Add it to Claude Code

```sh
claude mcp add panoply -- panoply mcp
```

Add `--scope user` to have it in every project. Check it with `claude mcp list`, or type `/mcp` in a session.

## Add it to Claude Desktop

Open Settings, then Developer, then Edit Config, and add Panoply to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "panoply": {
      "command": "panoply",
      "args": ["mcp"]
    }
  }
}
```

Restart Claude Desktop. If it cannot find `panoply`, use the full path to the program as the `command`
(`which panoply` prints it).

## Add it to Cursor

In `~/.cursor/mcp.json` (or `.cursor/mcp.json` in a project), use the same `mcpServers` entry as above.

## What the agent sees

- **One tool per module.** The name is the module id with dots turned into underscores: `pdf.merge` is
  `pdf_merge`, `image.resize_to_kb` is `image_resize_to_kb`. The description is the module summary.
- **Built-ins and community plugins.** Every built-in module, plus every community plugin that is compiled and
  sits in a folder Panoply scans (see `panoply modules`). A plugin that is not compiled is skipped, with a
  note on stderr. The list is read when the server starts, so restart it after adding a plugin.
- **Inputs.** `files` is a list of absolute paths. `output_dir` is an optional absolute folder for the
  results (created if missing). Every other property is one of the module's options, with its type, range,
  choices and default taken from `module.toml`.
- **Results.** A short text summary and `structuredContent` with `outputs` (absolute paths of the files that
  were written) and `notes` (the report lines of tools that only read, such as `files_hash` and `media_info`).
- **Annotations.** Tools that write files are marked as not read only and not destructive. Tools that use the
  network are marked open world.

## Rules the server keeps

- A path that is not absolute, does not exist or is a folder is refused with a sentence that says what to
  change. Nothing runs.
- Inputs are never overwritten. A result is a new file named after the original, for example
  `report (merged).pdf`, and a second run makes `report (merged 2).pdf`.
- A tool can read only the files it was given and write only into the output folder, or next to the
  originals when none is given. This is the same capability guard the app and the command line use.
- A tool that fails comes back as an error result the model can read: what happened and what to do next.
- Progress is sent as `notifications/progress` when the client sends a progress token, and
  `notifications/cancelled` stops the running job.

## Downloads

A few tools need a model or an engine that is not in the installer (see [PRIVACY.md](PRIVACY.md)). Panoply asks
before any download, and a server has no way to ask, because stdin carries the protocol. So the tool fails
with a message that names the item. Allow it ahead of time and call again:

```sh
panoply models allow isnet pdfium
```

or start the server with `panoply --yes mcp` to say yes to every download. `panoply models` lists the items.

## Try it by hand

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"hand","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | panoply mcp
```

Messages are one JSON object per line. Only protocol messages go to stdout. Logs go to stderr.

## Protocol

Stdio transport, JSON-RPC 2.0. `initialize` answers with the client's protocol version when it is one of
2025-11-25, 2025-06-18, 2025-03-26 or 2024-11-05, and 2025-11-25 otherwise. Methods: `initialize`, `ping`,
`tools/list` and `tools/call`. The server is hand written in `crates/panoply-mcp/src/server.rs`: the official
Rust SDK needs an async runtime, which the start-time and memory budgets in
[FLOWS.md](FLOWS.md#budgets-every-module-must-meet) do not leave room for.

## Check it

```sh
cargo nextest run -p panoply-cli --test mcp --no-capture
```

spawns the real `panoply mcp` and runs initialize, tools/list (every built-in, each schema checked), a merge of
two PDFs with a page count check, the refusals above, and a community plugin.
