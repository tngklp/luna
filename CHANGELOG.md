# Changelog

All notable changes to Luna are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Luna adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-10-25

First release.

### Added

- **Engine (`luna-core`)**
  - Agentic loop with streaming, parallel tool execution, steering and
    follow-up queues, abort, and automatic context compaction.
  - Providers: OpenAI-compatible (also Ollama, llama.cpp, vLLM, LM Studio and
    custom gateways), Anthropic, and Google Gemini, behind one trait.
  - Tools: `read`, `write`, `edit`, `multi_edit`, `ls`, `glob`, `grep`, `bash`,
    `todo_write`, `todo_read`.
  - Per-class approval policy (read / write / bash / network).
  - Multi-agent orchestrator: agent tree, role presets, semaphore-bounded
    fan-out, dependency-ordered task board, cancellation and budget propagation,
    structured subagent reports.
  - Sessions: append-only JSONL transcripts plus a SQLite index with full-text
    search.
  - Token estimation, cost accounting and a shared budget guard.

- **Terminal interface (`luna`)**
  - ratatui UI with a session list, markdown transcript with syntax
    highlighting, composer with slash commands, status bar and approval prompts.
  - `--version` and `--help`.

- **Desktop app**
  - Tauri v2 shell embedding the engine in-process.
  - Chat surface ported from [PiUI](https://github.com/tngklp/piui): sidebar,
    transcript, composer, markdown rendering, status footer, welcome screen.
  - Change review tab.

- **Distribution**
  - `install.ps1` and `install.sh`: interactive, checksum-verified TUI
    installers.
  - Native installers (NSIS, MSI, deb, AppImage, dmg) that also install the
    `luna` terminal command.
  - CI across Linux, Windows and macOS; tag-driven release workflow.

[0.1.0]: https://github.com/tngklp/luna/releases/tag/v0.1.0
