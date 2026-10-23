# Luna

AI agent harness in Rust with a multi-agent orchestrator at its core, exposed
through two frontends: a **terminal UI** (ratatui) and a **desktop GUI**
(Tauri + web frontend with full markdown and syntax-highlighted code rendering).

## Status

Early development. The core (event bus, agent loop, providers, tools,
permission gate, orchestrator) is implemented and tested; UIs are functional
scaffolds.

## Layout

```
crates/
  luna-core   # event bus, agent loop, orchestrator, providers, tools, config
  luna-tui    # terminal UI (ratatui) — binary: luna
  luna-gui    # Tauri desktop app — binary: luna-gui
    ui/       # web frontend (vite, marked + highlight.js + DOMPurify)
```

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

## Building

Requires Rust 1.90+ and Node 20+ (for the GUI frontend).```sh
cargo build --workspace          # core + TUI
cd crates/luna-gui/ui && npm i && npm run build   # GUI frontend
cargo build -p luna-gui          # GUI backend (expects ui/dist)
```

## Running

```sh
# TUI
cargo run -p luna-tui

# GUI (dev)
cd crates/luna-gui/ui && npm run dev   # terminal 1
cargo run -p luna-gui                  # terminal 2
```

Configure providers in the platform config dir
(`%APPDATA%\luna\Luna\config.toml` on Windows) and export API keys:

```toml
[providers.anthropic]
enabled = true
model = "claude-sonnet-4-5"
```

```sh
export ANTHROPIC_API_KEY=sk-ant-...
```

## Testing

```sh
cargo test --workspace
```

The `MockProvider` enables deterministic, network-free tests of the agent loop
and orchestrator.

## License

MIT
