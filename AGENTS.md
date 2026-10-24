# AGENTS.md

Machine-oriented project summary for AI coding agents. Facts here are verified
against the source; when code and this file disagree, the code wins — update
this file in the same change.

---

## 1. What this is

**Luna** is an AI agent harness written in Rust. It runs LLM agents that call
tools (filesystem, shell, web, MCP), coordinates multiple agents through an
orchestrator, and exposes the same core through two frontends: a terminal UI
and a desktop GUI.

- Repo: `tngklp/luna` · License: MIT · Version: `0.1.1`
- Rust edition 2024, MSRV 1.90
- Not a library for third parties: `luna-core` is an internal crate, but its
  API is stable enough to be the single source of truth for both UIs.

**Design invariant:** every UI is a *view* over `luna-core`. UIs never talk to
providers, tools, or storage directly. If you need new behavior, add it to
`luna-core` and surface it through events/commands.

---

## 2. Repository layout

```
Cargo.toml                     workspace root (shared deps, version, profile)
crates/
  luna-core/                   the harness (no UI code)
    src/lib.rs                 EventBus, LunaEvent, EventKind, Command, AgentHandle
    src/config.rs              TOML config, provider metadata, ~/.luna paths
    src/error.rs               unified Error/Result
    src/event.rs               re-export shim for the bus types
    src/storage.rs             SQLite: sessions + events, write-through bus tap
    src/orchestrator/
      agent_loop.rs            AgentRunner: streaming turn loop, tool dispatch
      registry.rs              AgentRegistry: name -> AgentSpec resolution
      runner.rs                Orchestrator: single / fan-out / supervised plans
      messaging.rs             MessageHub: mailboxes, broadcast, scratchpad
    src/provider/
      mod.rs                   Provider trait
      types.rs                 Message, ContentBlock, CompletionRequest, StreamChunk
      anthropic.rs             Anthropic Messages API (SSE)
      openai_compat.rs         OpenAI-compatible (SSE) — OpenAI, Gemini, xAI, …
      ollama.rs                native Ollama (/api/chat, NDJSON)
      resolver.rs              config -> constructed provider (shared by UIs)
      mock.rs                  scripted provider for deterministic tests
    src/tool/
      registry.rs              Tool trait + ToolRegistry
      builtin.rs               read_file, write_file, list_dir, glob, shell
      web.rs                   web_fetch (SSRF-guarded, HTML -> text)
      mcp.rs                   MCP stdio client (rmcp), tools bridged in
      permissions.rs           PermissionGate: rules + approval decisions
  luna-tui/                    terminal UI (binary: `luna`)
    src/main.rs                CLI entry, runtime setup, --check/--setup
    src/app.rs                 state machine, key handling, event absorption
    src/ui.rs                  ratatui rendering
    src/markdown.rs            markdown -> styled ratatui lines
    src/setup.rs               first-run interactive wizard
  luna-gui/                    desktop UI (binary: `luna-gui`)
    src/main.rs                Tauri backend: commands + event forwarding
    ui/                        vite frontend (vanilla JS, no framework)
      src/main.js              all UI logic
      src/style.css            theme
    tauri.conf.json            window, bundle targets, sidecar
    binaries/                  sidecar `luna` CLI (gitignored, staged by CI)
.github/workflows/
  ci.yml                       fmt + clippy + test on 3 OSes
  release-v2.yml               tag -> TUI binaries + GUI installers
install.sh / install.ps1       one-liner TUI installers
```

---

## 3. Core concepts

### Event bus (the spine)

`EventBus` is a `tokio::sync::broadcast` channel. Everything observable is an
event; UIs subscribe and render. Storage subscribes and persists.

```rust
pub struct LunaEvent {
    pub session_id: uuid::Uuid,
    pub agent_id: Option<String>,   // None = orchestrator/global
    pub timestamp: DateTime<Utc>,
    pub kind: EventKind,
}

pub enum EventKind {
    RunStarted,
    UserMessage(String),
    AssistantMessage(String),
    TokenDelta(String),             // streaming text
    ToolCallRequested { call_id, name, arguments },
    ToolCallFinished  { call_id, name, outcome },
    ApprovalRequested { request_id, agent_id, tool_name, arguments },
    ApprovalResolved  { request_id, approved },
    AgentSpawned      { agent_id, parent, spec_name },
    OrchestratorTransition(String),
    RunFinished       { summary, error },
}
```

Commands flow the other way (`Command::UserMessage | Approval | Cancel`).

### Agent loop

`AgentRunner::run(ctx, &mut history, cancel)` loops:
stream completion → append assistant message → if tool calls, gate each one →
execute → append tool results → repeat. Bounded by `max_iterations`.
Cancellation is a `CancellationToken` checked between chunks and before tools.

### Orchestrator

`Orchestrator::execute(plan, ctx, approvals, cancel)` with three plans:

| Plan | Behavior |
|---|---|
| `Single { agent, task }` | one agent, end to end |
| `FanOut { agents, join_prompt }` | all agents in parallel, results joined |
| `Supervised { supervisor, workers }` | workers in parallel, then supervisor synthesizes |

Per-agent failures are captured in `RunResult.errors` and never abort siblings.
`RunResult.outputs` is keyed by **agent name**. A `MessageHub` is created per
run for inter-agent messaging and returned on the result.

### Providers

`Provider` trait: `stream_completion(request, cancel) -> Stream<StreamChunk>`.
`StreamChunk` is `TextDelta | ToolUse | Usage | Done { stop_reason }`.

`provider::resolver` is the only place that turns config into a live provider:

```rust
resolve_active(&cfg) -> Option<ResolvedProvider>   // default_provider, else first enabled
resolve_provider(&cfg, key) -> Option<ResolvedProvider>
```

`ResolvedProvider { key, provider: Arc<dyn Provider>, model, max_tokens, temperature }`.
A provider resolves only if enabled **and** it has an API key (config value or
the provider's env var). Ollama and custom endpoints may be keyless.
`max_tokens == 0` is treated as unset (8192).

### Tools

`Tool` trait: `name`, `description`, `parameters` (JSON Schema), `execute`,
`requires_approval`. Registered in `ToolRegistry`; `specs()` feeds the model.

Built-ins: `read_file`, `write_file`, `list_dir`, `glob`, `shell`, `web_fetch`.
MCP tools are registered as `mcp_<server>_<tool>` and require approval.

### Permission gate

`PermissionGate::evaluate_with_registry(tool, input, registry)` returns
`Allowed | Denied(reason) | NeedsApproval`. Order: explicit deny → contextual
rules (shell prefixes, path roots) → explicit allow → registry approval flag →
default allow. `NeedsApproval` parks the loop on the `ApprovalSink` until the UI
answers or the run is cancelled.

### Storage

SQLite at `~/.luna/sessions.db`. `attach_store(&bus, store)` spawns a task that
records every event. **It requires a tokio runtime** — call it from inside one.

---

## 4. Configuration

Everything lives in `~/.luna/`:

| Path | Purpose |
|---|---|
| `~/.luna/config.toml` | settings |
| `~/.luna/sessions.db` | session history |
| `~/.luna/logs/` | `luna-tui.log`, `gui-panic.log` |

Helpers: `config::luna_dir()`, `config_path()`, `sessions_db_path()`, `logs_dir()`.

Shape (abridged):

```toml
default_provider = "deepseek"

[providers.anthropic]
enabled = true
model = "claude-sonnet-4-5"
api_key = ""            # empty -> ANTHROPIC_API_KEY
base_url = ""           # empty -> official endpoint

[providers.custom.my_local]
base_url = "http://127.0.0.1:8080/v1"
model = "llama3.2"
api_key = ""            # keyless is allowed

[providers.ollama]
enabled = true
base_url = "http://localhost:11434"
model = "qwen3"

[[agents]]
name = "coder"
provider = "anthropic"
system = "You are a careful coding agent."
tools = ["read_file", "write_file", "glob", "shell"]

[permissions]
approval_tools = ["shell", "write_file"]

[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
```

Built-in provider keys: `anthropic`, `openai`, `google`, `xai`, `deepseek`,
`mistral`, `openrouter`, `groq`, `ollama`, plus `custom.<name>`.
Metadata (default base URL, env var, default model) is `config::PROVIDER_META`.

---

## 5. Frontends

### TUI (`crates/luna-tui`, binary `luna`)

- Entry: `main.rs` → `start_tui` wraps everything in one tokio runtime.
- `app.rs` holds all state (`App`), key handling, and event absorption.
- `ui.rs` renders: transcript (markdown + tool cards), composer with slash
  menu, agent sidebar, session picker, approval modal, status bar.
- `markdown.rs` converts markdown to styled `ratatui::text::Line`s.
- CLI: `--version`, `--help`, `--check` (validate config/providers/paths),
  `--setup` (re-run wizard). No config file → wizard runs automatically.

### GUI (`crates/luna-gui`, binary `luna-gui`)

- Tauri 2 backend in `src/main.rs`; frontend is vanilla JS in `ui/src/main.js`.
- Backend forwards every `LunaEvent` to the webview as a flattened JSON payload
  on the `luna://event` channel; `agent_id` is always present.
- Commands (all `luna_*`): `send_message`, `cancel`, `respond_approval`,
  `run_agent`, `run_plan`, `list_sessions`, `load_session`, `list_providers`,
  `active_provider`, `save_provider`, `save_custom_provider`,
  `remove_custom_provider`, `set_default_provider`.
- **Runtime rule:** inside `setup` and `main`, use `tauri::async_runtime::spawn`,
  never bare `tokio::spawn` — there is no ambient runtime there.
- A panic hook writes to `~/.luna/logs/gui-panic.log` (release builds have no
  console, so silent crashes are otherwise invisible).

---

## 6. Build, run, test

```sh
cargo build --workspace                 # core + TUI
cargo test --workspace                  # 73 tests
cargo clippy --workspace --all-targets  # must be warning-free
cargo fmt --all -- --check              # CI enforces

cargo run -p luna-tui                   # TUI
cargo run -p luna-tui -- --check        # config diagnostics

# GUI needs the frontend built first:
cd crates/luna-gui/ui && npm install && npm run build && cd ../../..
cargo run -p luna-gui
```

The GUI build script requires two things to exist even for `cargo check`:
`crates/luna-gui/ui/dist/` (frontend) and
`crates/luna-gui/binaries/luna-<target-triple>[.exe]` (sidecar). CI stages a
placeholder sidecar; locally, copy `target/debug/luna.exe` there.

---

## 7. Conventions and gotchas

- **Tests are deterministic.** Use `provider::mock::MockProvider` with scripted
  turns; never hit the network in tests.
- **No `tokio::spawn` outside a runtime.** Both UIs previously crashed on this.
  TUI: one runtime in `start_tui`. GUI: `tauri::async_runtime`.
- **`RunResult.outputs` is keyed by agent name**, not by output text.
- **Approval flow:** the loop publishes `ApprovalRequested` *before* parking on
  the sink, so UIs can correlate by `request_id`.
- **Custom providers live under `[providers.custom.<name>]`**, not
  `[providers.<name>]` — the latter is silently ignored by serde.
- **YAML in workflows:** quote any step `name:` containing `: ` (colon+space),
  or GitHub rejects the whole file with a startup failure and no logs.
- **Bundle targets are per-platform.** `targets: ["nsis"]` makes macOS skip
  bundling entirely; the release workflow passes `--bundles` per matrix entry.
- **Windows paths** in tests must be platform-agnostic (`temp_dir()`), not
  hardcoded `C:\...`.
- **`std::env::remove_var` is unsafe** in edition 2024; avoid mutating env in
  parallel tests.
- **Formatting:** `cargo fmt` before committing; CI fails on drift.

---

## 8. Where to make common changes

| Goal | Touch |
|---|---|
| New provider | `provider/<name>.rs` + `config::PROVIDER_META` + `resolver.rs` |
| New tool | `tool/<name>.rs`, register in `builtin::register_all` or a new registrar |
| New event | `EventKind` in `lib.rs`, `UiEvent` in GUI, `absorb` in TUI, `storage::serialize_kind` |
| New orchestrator plan | `TaskPlan` + `Orchestrator::execute` in `runner.rs` |
| New TUI screen | `app.rs` state + `ui.rs` draw fn |
| New GUI panel | `ui/src/main.js` + `ui/src/style.css`; add a Tauri command if it needs core |
| Change config shape | `config.rs` (serde defaults matter — old configs must still load) |

---

## 9. Current state

Working: agent loop, streaming, tool calling, approvals, orchestrator (3 plans),
inter-agent messaging, 9 provider paths + custom, 6 built-in tools, MCP client,
SQLite persistence with session resume, TUI (markdown, tool cards, slash
commands, session picker), GUI (sidebar, transcript, settings, sessions).

Known gaps: no token-accurate usage accounting (estimate is `len/4`); no
compaction/summarization of long histories; MCP is stdio-only; no OAuth
providers; GUI has no diff view for file edits.
