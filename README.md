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

Luna keeps everything in one directory, `~/.luna`:

| Path | Contents |
|---|---|
| `~/.luna/config.toml` | Providers, models, roles, approvals |
| `~/.luna/sessions/` | Session transcripts (JSONL) |
| `~/.luna/luna.db` | Session index and change log |

The config is written on first run. Nothing is created in your working
directory, so launching Luna in a fresh folder leaves it untouched.

Most settings are also editable in the app: **Settings → Models** for providers
and models, **Settings → Tools** for the approval policy, **Settings → General**
for behaviour, and **Settings → Customization** for the theme.

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
preset = "anthropic"

[providers.openai]
preset = "openai"

[providers.gemini]
preset = "gemini"

[providers.ollama]
preset = "ollama"
```

Keys are read from the environment by default, so nothing secret has to live in
the file. Set `api_key` instead if you prefer.

### Providers

Every provider below is built in. Enable one by adding a table with its preset:

```toml
[providers.groq]
preset = "groq"
```

| Preset | Provider | Key variable |
|---|---|---|
| `anthropic` | Anthropic | `ANTHROPIC_API_KEY` |
| `openai` | OpenAI | `OPENAI_API_KEY` |
| `gemini` | Google Gemini | `GEMINI_API_KEY` |
| `openrouter` | OpenRouter | `OPENROUTER_API_KEY` |
| `groq` | Groq | `GROQ_API_KEY` |
| `deepseek` | DeepSeek | `DEEPSEEK_API_KEY` |
| `mistral` | Mistral | `MISTRAL_API_KEY` |
| `xai` | xAI | `XAI_API_KEY` |
| `together` | Together AI | `TOGETHER_API_KEY` |
| `fireworks` | Fireworks AI | `FIREWORKS_API_KEY` |
| `cerebras` | Cerebras | `CEREBRAS_API_KEY` |
| `perplexity` | Perplexity | `PERPLEXITY_API_KEY` |
| `zai` | Z.ai (GLM) | `ZAI_API_KEY` |
| `moonshot` | Moonshot (Kimi) | `MOONSHOT_API_KEY` |
| `ollama` | Ollama (local) | — |
| `llamacpp` | llama.cpp (local) | — |
| `lmstudio` | LM Studio (local) | — |
| `vllm` | vLLM (local) | — |

A preset only fills in defaults. Anything you set explicitly wins, so this points
Groq's key and models at a proxy:

```toml
[providers.groq]
preset = "groq"
base_url = "https://proxy.internal/v1"
```

### Custom providers

Anything that speaks the OpenAI Chat Completions dialect works without a preset —
just give it a name, a URL and a key variable:

```toml
[providers.my-gateway]
kind = "openai"                       # or "anthropic" / "gemini"
base_url = "https://gateway.internal/v1"
api_key_env = "GATEWAY_KEY"
models = ["house-model"]
discover_models = false               # true asks the endpoint for its model list
headers = { "X-Team" = "luna" }       # extra request headers
```

Then select it like any other provider:

```sh
luna --model my-gateway/house-model
```

`kind` picks the wire protocol, not the vendor: an Anthropic-compatible gateway
uses `kind = "anthropic"`, and everything else uses `kind = "openai"`.

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
| `Esc` | Abort the running turn, or close the palette |
| `Ctrl+C` | Quit |
| `Ctrl+N` | New session |
| `Ctrl+S` | Toggle the session list |
| `Ctrl+L` | Clear the transcript |
| `PgUp` / `PgDn` | Scroll |
| `y` / `a` / `n` | Answer an approval prompt |

Typing `/` opens the command palette: a filtered list with descriptions, driven
by the arrow keys, `Tab` to complete and `Enter` to run.

| Command | Purpose |
|---|---|
| `/help` | Keys and commands |
| `/new` | Start a new session |
| `/model <ref>` | Switch model, e.g. `/model local/llama3.2` |
| `/thinking <level>` | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max` |
| `/sessions` | Browse saved sessions |
| `/compact` | Summarise the conversation |
| `/copy` | Copy the transcript |
| `/clear` | Clear the transcript |
| `/quit` | Exit |

The header shows the model and where the config came from; the status bar shows
the run state, thinking level, context usage, tokens and cost.

### Desktop

The GUI is the chat surface from [PiUI](https://github.com/tngklp/piui), ported
to Tauri. It opens straight into the chat. `Ctrl+N` starts a session, `Ctrl+K`
searches, and the composer switches to steering while a turn is running.

**New session** only opens a fresh chat window — the transcript is written when
you send the first prompt, so an abandoned draft leaves nothing behind.

Settings live behind the gear at the bottom of the sidebar:

| Tab | What it controls |
|---|---|
| General | Reasoning and tool-output expansion, auto-keep, send key, workspace |
| Models | Providers, base URLs, key variables, protocols, and model definitions |
| Customization | Theme and icon pack |
| Tools | Approval policy, per tool class |

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
cargo test --workspace          # 118 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

cd gui
npm install
npm run dev                     # the UI in a browser, backed by a mock
npm run tauri:dev               # the real app
npm run typecheck
npm run check                   # type-check the Rust backend
```

`npm run dev` serves the renderer with a mock core, which is how the chat surface
is developed without rebuilding Rust.

### Testing locally

There are three ways to run Luna, in increasing order of realism.

#### 1. The GUI in a browser — no Rust, no API key

```sh
cd gui
npm install
npm run dev            # http://localhost:5273
```

This runs the real renderer against [a mock core](gui/src/lib/mock.ts). The mock
streams a canned reply with a code block and a bash tool card, so the transcript,
markdown, composer and status footer all exercise their real code paths. Use it
for layout and styling work — it never touches the network.

#### 2. The TUI

```sh
cargo run -p luna-tui            # debug build, fastest to iterate
cargo run --release -p luna-tui  # what users get
```

The TUI needs a provider to answer. With no key configured it still starts, and
the first message reports the provider error — enough to check the layout, the
composer, `/help`, and the approval prompt.

#### 3. The real desktop app

```sh
cd gui
npm run tauri:dev
```

This builds the Rust backend, starts Vite, and opens the window with hot reload
for the frontend. Rust changes trigger a rebuild and restart.

### Giving it a model

Any one of these is enough.

**A local model — no key, no cost.** Start [Ollama](https://ollama.com) and pull
something:

```sh
ollama pull llama3.2
```

The default config already points the `local` provider at
`http://localhost:11434/v1`, so switch to it:

```sh
luna --model local/llama3.2
```

or set it in `config.toml`:

```toml
[general]
default_model = "local/llama3.2"
```

**A hosted provider.** Set the key for whichever provider you want, then restart:

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."   # current shell only
[Environment]::SetEnvironmentVariable('ANTHROPIC_API_KEY', 'sk-ant-...', 'User')  # persistent
```

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
```

`OPENAI_API_KEY` and `GEMINI_API_KEY` work the same way. The default model is
`anthropic/claude-sonnet-4-5`; change `default_model` in `config.toml` to use
another.

To confirm what Luna can see:

```sh
luna --help          # prints the config path it reads
```

### Checking the agent without a UI

The engine is a library, so the fastest way to exercise the loop, tools and
orchestrator is the test suite — it drives a scripted provider, so it needs no
network and no key:

```sh
cargo test -p luna-core                    # 105 tests
cargo test -p luna-core --test agent_loop  # the loop and orchestrator end to end
```

### The TUI sidecar

The desktop installer ships the `luna` binary as a Tauri sidecar, so
`gui/src-tauri/binaries/luna-<target-triple>` must exist before **any** Cargo
command runs in `gui/src-tauri` — Tauri's build script validates `externalBin`
paths even for `cargo check`.

```sh
cargo build --release -p luna-tui   # build the TUI first
cd gui && npm run sidecar           # copy it into place
```

`npm run sidecar:optional` writes a placeholder instead of failing, which is what
CI uses for `cargo check`. Release builds use the strict form so a placeholder can
never be bundled.

## Releasing

Releases are **manual only** — nothing is published on a normal push. To cut one:

1. Add the version's section to [CHANGELOG.md](CHANGELOG.md):

   ```markdown
   ## [0.2.0] — 2026-11-02

   ### Added
   - …
   ```

2. Run the **Release** workflow from the Actions tab (or `gh workflow run release.yml -f version=0.2.0`).

The workflow then:

- validates the version and refuses to reuse an existing tag,
- reads that version's section from the changelog and uses it as the release body,
- builds the TUI for six targets and the desktop app for four,
- creates the tag and the release, and attaches every artifact plus `checksums.txt`.

The `version` input accepts `0.2.0` or `v0.2.0`. `prerelease` and `draft` are
optional flags.

```sh
gh workflow run release.yml -f version=0.2.0
gh run watch
```

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
scripts/              icon generation, sidecar staging, changelog extraction
```

## Credits

The desktop chat surface is ported from [PiUI](https://github.com/tngklp/piui),
which is MIT licensed. The design system, transcript, composer and markdown
rendering are its work; Luna replaces the Electron host with Tauri and the Pi
agent with `luna-core`.

## License

MIT. See [LICENSE](LICENSE).
