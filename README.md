# Luna

An AI coding agent with a terminal interface and a desktop app, built around a
multi-agent orchestrator.

Luna is one Rust engine with two front-ends. The TUI and the GUI drive the same
agent loop and consume the same event stream, so behaviour cannot drift between
them.

```
┌──────────────┐   ┌──────────────┐
│  luna (TUI)  │   │  Luna (GUI)  │
│   ratatui    │   │    Tauri     │
└──────┬───────┘   └──────┬───────┘
       │                  │
       └────────┬─────────┘
                │  AgentEvent stream
        ┌───────▼────────┐
        │   luna-core    │
        │ providers      │
        │ tools          │
        │ agent loop     │
        │ orchestrator   │
        │ sessions       │
        └────────────────┘
```

## Install

### Terminal only

```powershell
irm https://raw.githubusercontent.com/tngklp/luna/main/install.ps1 | iex
```

```sh
curl -fsSL https://raw.githubusercontent.com/tngklp/luna/main/install.sh | sh
```

Both scripts are interactive: they detect your platform, download the matching
release, verify its SHA-256, install to a user directory, add it to `PATH`, and
walk you through provider setup. No Rust toolchain is needed.

### Desktop app

Download the installer for your platform from
[Releases](https://github.com/tngklp/luna/releases/latest):

| Platform | Installer |
|---|---|
| Windows | `Luna_<version>_x64-setup.exe` (NSIS) or `.msi` |
| Linux | `.deb` or `.AppImage` |
| macOS | `.dmg` |

The desktop installer also installs the `luna` terminal command.

### From source

```sh
cargo build --release -p luna-tui     # target/release/luna
cd gui && npm install && npm run tauri:build
```

## Configure

Luna writes a commented default config on first run:

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\luna\config\config.toml` |
| Linux | `~/.config/luna/config.toml` |
| macOS | `~/Library/Application Support/luna/config.toml` |

```toml
[general]
default_model = "anthropic/claude-sonnet-4-5"
max_concurrent_agents = 4   # how many subagents may run at once
max_cost_usd = 0.0          # 0 disables the budget guard
max_steps = 60
compact_at = 0.8            # compact when the context is 80% full
bash_timeout_secs = 120

[approvals]
read = "allow"
write = "ask"
bash = "ask"
network = "allow"

[providers.anthropic]
kind = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[providers.openai]
kind = "openai"
api_key_env = "OPENAI_API_KEY"

[providers.gemini]
kind = "gemini"
api_key_env = "GEMINI_API_KEY"

# Ollama, llama.cpp, vLLM, LM Studio, OpenRouter and any other
# OpenAI-compatible endpoint.
[providers.local]
kind = "openai"
base_url = "http://localhost:11434/v1"
```

Keys are read from the environment by default, so nothing secret has to live in
the file. Set `api_key` instead if you prefer.

### Roles

Each subagent runs with a role, which decides its model, tools and step budget:

```toml
[roles.reviewer]
model = "anthropic/claude-haiku-4-5"
tools = ["read", "grep", "glob"]
max_steps = 20
```

Built-in roles: `lead`, `planner`, `worker`, `reviewer`, `researcher`.

## Use

### Terminal

```
luna
```

| Key | Action |
|---|---|
| `Enter` | Send |
| `Shift+Enter` | New line |
| `Esc` | Abort the running turn |
| `Ctrl+C` | Quit |
| `Ctrl+N` | New session |
| `Ctrl+S` | Toggle the session list |
| `Ctrl+L` | Clear the transcript |
| `y` / `a` / `n` | Answer an approval prompt |

Commands: `/help`, `/new`, `/model`, `/thinking`, `/sessions`, `/copy`, `/quit`.

### Desktop

The GUI is the chat surface from [PiUI](https://github.com/tngklp/piui), ported
to Tauri. `Ctrl+N` starts a session, `Ctrl+K` searches, and the composer switches
to steering while a turn is running.

## How it works

### The agent loop

One turn is: assemble context → stream the model → run the tools it asked for →
append the results → repeat until it stops asking. Everything is cancellable, and
every step is observable through the event stream.

- **Steering** injects a message into the *current* turn, so you can correct
  course mid-flight.
- **Follow-ups** wait for the turn to settle, so they behave like the next
  message rather than an interruption.
- **Compaction** summarises old turns when the context fills, keeping the tail
  verbatim so recent work is never lossy.

### Multi-agent orchestration

A supervisor owns a tree of agents. Each node is a full session with its own
conversation, model and tool allowlist, so a subagent cannot corrupt its parent's
context.

Three properties make it reliable rather than merely parallel:

1. **Bounded fan-out.** A semaphore caps concurrent subagents, so a model that
   spawns twenty workers cannot exhaust the machine or the budget.
2. **Dependency-ordered work.** The task board decides what runs next, not the
   model's prose. A worker starts only when its prerequisites are done.
3. **Propagating cancellation.** Aborting the root cancels every descendant, and
   a subagent that dies reports back instead of hanging its parent.

The model reaches this through four tools:

| Tool | Purpose |
|---|---|
| `spawn_agent` | Run one subagent on a self-contained brief |
| `spawn_agents` | Run several in parallel |
| `plan` | Get the instructions for writing a plan |
| `run_plan` | Execute a dependency-ordered plan |

Every subagent returns a structured report — status, summary, steps, tool calls,
cost — which the parent sees as a message. No free-form parsing.

### Tools

| Tool | Class | Purpose |
|---|---|---|
| `read` | read | Read a file with line numbers |
| `write` | write | Create or replace a file |
| `edit` | write | Replace one exact occurrence |
| `multi_edit` | write | Several replacements, all or nothing |
| `ls` | read | List a directory |
| `glob` | read | Find files by pattern |
| `grep` | read | Search contents with a regex |
| `bash` | bash | Run a shell command |
| `todo_write` / `todo_read` | write / read | The task board |

Approval is per class, so `read` can be allowed while `bash` always asks.

### Sessions

Two stores, each doing what it is good at:

- **JSONL** — the transcript, one message per line. Crash-safe, greppable, cheap
  to resume or fork. A truncated final line is skipped rather than failing the
  session.
- **SQLite** — an index over those files for fast listing, full-text search and
  the change log.

### Performance

- `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`.
- `mimalloc` as the global allocator.
- Zero-copy SSE decoding; tool-call arguments accumulate in place.
- UI updates are coalesced to one redraw per frame, not one per token.
- Large tool outputs are capped before they reach the model.
- The GUI links the engine in-process, so a streamed token costs one event hop.
- Stable prompt prefixes and provider cache hints maximise prompt-cache hits.

## Development

```sh
cargo test --workspace          # 114 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

cd gui
npm install
npm run dev                     # the UI in a browser, backed by a mock
npm run tauri:dev               # the real app
npm run typecheck
```

`npm run dev` serves the renderer with a mock core, which is how the chat surface
is developed without rebuilding Rust.

### Layout

```
crates/luna-core/     the engine
  provider/           OpenAI-compatible, Anthropic, Gemini
  tool/               fs, bash, todo, delegation
  agent/              the loop, context, queues
  orchestrator/       supervisor, task board
  session/            JSONL transcripts, SQLite index
crates/luna-tui/      the terminal interface
gui/                  the Tauri app
  src/                the ported PiUI chat surface
  src-tauri/          commands, DTOs, state
scripts/              icon generation, sidecar staging
```

## Credits

The desktop chat surface is ported from [PiUI](https://github.com/tngklp/piui),
which is MIT licensed. The design system, transcript, composer and markdown
rendering are its work; Luna replaces the Electron host with Tauri and the Pi
agent with `luna-core`.

## License

MIT. See [LICENSE](LICENSE).
