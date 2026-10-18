# Use Panoply from an AI agent (MCP)

`panoply mcp` turns every installed Panoply tool into an [MCP](https://modelcontextprotocol.io) tool, so an
agent such as Claude Code, Claude Desktop or Cursor can merge PDFs, compress images, trim video and the rest
on your own computer. It runs locally over stdio. Nothing is uploaded, and the agent only touches the files
it names.

You need the `panoply` command line on your PATH. Check with `panoply version`.

## Quick install

| Where | Do this |
| --- | --- |
| Claude Desktop (macOS and Windows) | Download the `panoply-mcp-<version>-<system>.mcpb` that matches your computer from the [latest release](https://github.com/anzal1/panoply/releases/latest) (`arm64` = Apple Silicon, `x64` = Intel or AMD), open it with Claude Desktop, and confirm the install dialog. It carries the program and its modules, so you need nothing else installed. |
| Cursor | [Add to Cursor](https://cursor.com/en/install-mcp?name=panoply&config=eyJjb21tYW5kIjoicGFub3BseSIsImFyZ3MiOlsibWNwIl19) |
| VS Code | [Add to VS Code](https://insiders.vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522name%2522%253A%2522panoply%2522%252C%2522command%2522%253A%2522panoply%2522%252C%2522args%2522%253A%255B%2522mcp%2522%255D%257D) or [VS Code Insiders](https://insiders.vscode.dev/redirect?url=vscode-insiders%3Amcp%2Finstall%3F%257B%2522name%2522%253A%2522panoply%2522%252C%2522command%2522%253A%2522panoply%2522%252C%2522args%2522%253A%255B%2522mcp%2522%255D%257D) |
| Claude Code | `claude mcp add panoply -- panoply mcp` |

The Cursor and VS Code links add a server that runs `panoply mcp`, so they need the `panoply` command line on your
PATH (check with `panoply version`). Both editors show what will be added and ask before they add it.

Direct links, for a browser address bar or a terminal (`xdg-open`, `open`, `start`):

```text
cursor://anysphere.cursor-deeplink/mcp/install?name=panoply&config=eyJjb21tYW5kIjoicGFub3BseSIsImFyZ3MiOlsibWNwIl19
vscode:mcp/install?%7B%22name%22%3A%22panoply%22%2C%22command%22%3A%22panoply%22%2C%22args%22%3A%5B%22mcp%22%5D%7D
vscode-insiders:mcp/install?%7B%22name%22%3A%22panoply%22%2C%22command%22%3A%22panoply%22%2C%22args%22%3A%5B%22mcp%22%5D%7D
```

They are made by `python3 tools/mcp_links.py`, and `python3 tools/mcp_links.py --check` decodes each one and fails
unless it installs `panoply mcp`. Formats: [Cursor](https://cursor.com/docs/context/mcp/install-links) (a base64
JSON config next to a name) and [VS Code](https://code.visualstudio.com/docs/copilot/guides/mcp-developer-guide)
(`vscode:mcp/install?` and the URL encoded JSON). GitHub does not open `vscode:` links, so the table goes through
the `insiders.vscode.dev/redirect` page, the way Microsoft's own server READMEs do.

The bundle has one setting, **Allow downloads**, off by default. It sets `PANOPLY_YES` for the server, which is the
same as `--yes` (see Downloads below).

Claude Desktop unpacks a bundle's files without the execute permission, so a bundle that ran the program directly
would fail to start ([mcpb issue 294](https://github.com/modelcontextprotocol/mcpb/issues/294) and
[282](https://github.com/modelcontextprotocol/mcpb/issues/282)). The bundle is therefore a `node` server: a small
launcher (`server/index.js`, run by the Node that ships inside Claude Desktop) sets the permission on the program,
runs `panoply mcp` and passes the exit code and stop signals on. That is a file mode matter. The program itself is
unsigned like the rest of the beta, see [KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md). The bundle has been checked
with the official `mcpb` tool and by running its own server with every file at mode 0600, but it has not yet been
tried inside a real Claude Desktop.

## Add it by hand

Every client below runs the same thing: the command `panoply` with the argument `mcp`. If the client cannot find
`panoply`, use the full path as the command (`which panoply` on macOS and Linux, `where panoply` on Windows).

**Claude Code.** Add `--scope user` to have it in every project, or `--scope project` to share it in `.mcp.json`.
Check with `claude mcp list`, or type `/mcp` in a session. ([docs](https://code.claude.com/docs/en/mcp))

```sh
claude mcp add panoply -- panoply mcp
```

**Claude Desktop.** Open Settings, then Developer, then Edit Config, and add Panoply to
`claude_desktop_config.json`, then restart. (The `.mcpb` bundle above does this for you.)

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

**Cursor.** The same `mcpServers` entry as above, in `~/.cursor/mcp.json` (every project) or `.cursor/mcp.json`
(one project).

**VS Code.** VS Code calls the top level key `servers`. Put this in `.vscode/mcp.json`, or run
**MCP: Open User Configuration** for every workspace, or use `code --add-mcp` from a terminal.
([docs](https://code.visualstudio.com/docs/copilot/customization/mcp-servers))

```json
{
  "servers": {
    "panoply": {
      "type": "stdio",
      "command": "panoply",
      "args": ["mcp"]
    }
  }
}
```

```sh
code --add-mcp '{"name":"panoply","command":"panoply","args":["mcp"]}'
```

**Windsurf.** Open the Cascade MCP settings and choose to edit the raw config (`mcp_config.json`), then add the
same `mcpServers` entry as for Claude Desktop. Windsurf's docs have moved to
[docs.devin.ai](https://docs.devin.ai/desktop/cascade/mcp), which gives `~/.config/devin/mcp_config.json`
(`%APPDATA%\devin\mcp_config.json` on Windows); older installs used `~/.codeium/windsurf/mcp_config.json`.

**Zed.** Zed calls them context servers. Run `zed: open settings file` and add this to `settings.json`, or use
Settings, then AI, then MCP Servers. ([docs](https://zed.dev/docs/ai/mcp))

```json
{
  "context_servers": {
    "panoply": {
      "command": "panoply",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

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
- Progress is sent as `notifications/progress` when the client sends a progress token.
  `notifications/cancelled` asks the running job to stop; a job stops at its next check, so a long step can
  finish first. A call that is still queued is skipped and answered as cancelled.
- A long job does not hold anything up. `ping` and `tools/list` are answered at once, and up to four calls run
  at the same time. If a tool crashes, that call comes back as an error and the server keeps running.

## Downloads

A few tools need a model or an engine that is not in the installer (see [PRIVACY.md](PRIVACY.md)). Panoply asks
before any download, and a server has no way to ask, because stdin carries the protocol. So the tool fails
with a message that names the item. Allow it ahead of time and call again:

```sh
panoply models allow isnet pdfium
```

or start the server with `panoply --yes mcp`, or with the environment variable `PANOPLY_YES=1`, to say yes to every
download. `panoply models` lists the items. In the Claude Desktop bundle the setting **Allow downloads** sets that
variable.

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
2025-11-25 or 2025-06-18, and 2025-11-25 otherwise. Older versions allow JSON-RPC batches, which this server
does not read. Methods: `initialize`, `ping`,
`tools/list` and `tools/call`. The server is hand written in `crates/panoply-mcp/src/server.rs`: the official
Rust SDK needs an async runtime, which the start-time and memory budgets in
[FLOWS.md](FLOWS.md#budgets-every-module-must-meet) do not leave room for.

## Check it

```sh
cargo nextest run -p panoply-cli --test mcp --no-capture
```

spawns the real `panoply mcp` and runs initialize, tools/list (every built-in, each schema checked), a merge of
two PDFs with a page count check, the refusals above, and a community plugin.

## The Claude Desktop bundle and the MCP registry (for maintainers)

Every release builds one `.mcpb` per system (macOS, Windows and Linux, each for x64 and arm64) in the
`package-mcpb` job of `.github/workflows/release.yml`. A bundle is the `panoply` program, its `modules` folder, an
icon, a small `server/index.js` launcher and a `manifest.json` of type `node` (`packaging/mcpb/`; the reason is in
Quick install above). The tool list in the manifest is read from the
module manifests when the bundle is built. Each bundle is checked with `npx @anthropic-ai/mcpb validate` and
packed with `mcpb pack` ([format](https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md)). The files are
named `panoply-mcp-<version>-<system>.mcpb`: the official registry requires "mcp" in the download URL.
Claude Desktop runs on macOS and Windows only. The two Linux bundles exist for other MCPB capable clients and for
the registry listing, and cost one more job leg. CI also builds a Linux bundle from the test build and runs
`packaging/mcpb/check_served.py` on it, which starts the bundle's own server and fails unless it serves exactly the
tools in the manifest.

The `publish` job then writes `server.json` next to them with the version, the download URLs and the SHA-256 of
each bundle (`packaging/mcp-registry/`). It follows the
[registry schema](https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json) and the
[MCPB package rules](https://github.com/modelcontextprotocol/registry/blob/main/docs/reference/server-json/official-registry-requirements.md)
(GitHub release URL, `fileSha256`). It lists six `mcpb` packages, one per system and architecture, and the schema
has no platform field, so a client can tell them apart only by the file name. To list Panoply in the official registry, after the release is public:

```sh
gh release download vX.Y.Z -R anzal1/panoply -p server.json
mcp-publisher login github      # as anzal1; opens a device code page
mcp-publisher publish           # run in the folder that holds server.json
```

Install `mcp-publisher` from the [registry releases](https://github.com/modelcontextprotocol/registry/releases)
(`brew install mcp-publisher` on macOS). The name `io.github.anzal1/panoply` is proved by the GitHub login. Nothing
is published by the release workflow itself.
