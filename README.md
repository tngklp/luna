# Luna

AI agent harness in Rust with a **multi-agent orchestrator** at its core,
exposed through two frontends: a **terminal UI** (ratatui) and a **desktop
GUI** (Tauri + web frontend with full markdown and syntax-highlighted code
rendering).

## Features

- **Agentic loop** — streaming responses, tool calling, cancellation,
  interactive approvals, max-iteration guards
- **Multi-agent orchestrator** — single, supervised (delegation + synthesis),
  and fan-out (parallel + join) plans with per-agent failure isolation
- **Inter-agent messaging** — typed mailboxes, broadcast, shared scratchpad
- **Providers** — Anthropic, OpenAI, native Ollama, any OpenAI-compatible
  endpoint; all streaming over SSE/NDJSON
- **Tools** — filesystem (read/write/list/glob), shell, web fetch (SSRF
  guarded), and MCP servers (stdio) bridged into the same registry
- **Permission gate** — deny-by-default rules, command/path prefixes,
  per-tool approval flags, interactive y/n prompts in both UIs
- **Persistence** — every event written through to SQLite; sessions listed
  and replayable
- **Two UIs, one core** — identical behavior in terminal and desktop

## Installing

**TUI — one command:**

```sh
# macOS / Linux
curl -fsSL https://luna.tngklp.com/install | sh

# Windows (PowerShell)
irm https://luna.tngklp.com/install.ps1 | iex
```

Then run `luna` from any terminal.

**GUI + TUI — setup installer:** download the platform installer from
[GitHub releases](https://github.com/tngklp/luna/releases) (`luna-setup-x64.msi`
on Windows, `.dmg` on macOS). The setup installs both the desktop app and the
`luna` CLI, and adds the CLI to your PATH.

## Quick start

1. Enable a provider and export its key:

   ```toml
   # %APPDATA%\luna\Luna\config.toml (Windows)
   # ~/.config/luna/Luna/config.toml (Linux/macOS)
   [providers.anthropic]
   enabled = true
   model = "claude-sonnet-4-5"
   ```

   ```sh
   export ANTHROPIC_API_KEY=sk-ant-...   # or OPENAI_API_KEY, or enable ollama
   ```

2. Run `luna` (TUI) or open the Luna app (GUI).

3. Multi-agent runs from the GUI input box:

   ```
   /fanout researcher: find Rust async best practices | writer: draft a summary
   /supervised boss: researcher: gather data | coder: write the example
   ```

4. Optional MCP servers:

   ```toml
   [mcp_servers.filesystem]
   command = "npx"
   args = ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\you\\Documents"]
   ```

## Layout

```
crates/
  luna-core   # event bus, agent loop, orchestrator, providers, tools, storage
  luna-tui    # terminal UI (ratatui) — binary: luna
  luna-gui    # Tauri desktop app — binary: luna-gui
    ui/       # web frontend (vite, marked + highlight.js + DOMPurify)
```

## Building

Requires Rust 1.90+ and Node 20+ (for the GUI frontend).

```sh
cargo build --workspace                            # core + TUI
cd crates/luna-gui/ui && npm i && npm run build    # GUI frontend
cargo build -p luna-gui                            # GUI backend (expects ui/dist)
```

## Testing

```sh
cargo test --workspace
```

The `MockProvider` enables deterministic, network-free tests of the agent
loop, orchestrator, and messaging (63 tests).

## License

MIT
