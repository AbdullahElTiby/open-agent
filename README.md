# Open Agent

A personal AI coding agent: a native Rust desktop app (egui, no WebView) that runs on your **Claude Code** and **ChatGPT (Codex)** subscriptions.

It drives both CLIs in headless streaming mode, the same protocols their SDKs wrap:

| Agent  | Process                                 | Auth                                  |
| ------ | --------------------------------------- | ------------------------------------- |
| Claude | `claude -p --output-format stream-json` | your `claude` login (Pro/Max)          |
| Codex  | `codex exec --json`                     | `codex login` → Sign in with ChatGPT   |

## Run

```bash
cargo run --release
```

The binary is `target/release/open-agent.exe` (~14 MB, no installer needed). Sessions are saved to `%APPDATA%\Open Agent\sessions.json`.

## Features

- Threads sidebar, conversation, rounded composer, and a **Changes** panel with per-file diffs.
- Switch agent and model from the composer (`Ctrl J` toggles agent). The other agent receives the conversation so far.
- Approval modes: **Read only**, **Auto** (edits allowed), **Full access** (no prompts, no sandbox).
- Model brand icons (Claude, OpenAI, Gemini, DeepSeek, Grok, Qwen, Mistral, Kimi, Llama and more) are fetched automatically for any model name from the MIT-licensed [LobeHub icon set](https://github.com/lobehub/lobe-icons) and cached in `%APPDATA%\Open Agent\icons`. Unknown models get a lettered badge.
- Reasoning-effort picker. Codex models come from `~/.codex/models_cache.json`.
- Compact tool rows (ran / read / edited with +/- counts), expandable diffs and command output, a live plan checklist, and token counts.
- Threads resume the agent's real session (`--resume` / `codex exec resume`).
- **Import sessions** (sidebar) brings in your existing history: Claude Code and Claude Desktop transcripts (`~/.claude/projects`, plus any `.claude/projects` under Claude Desktop's app data) and Codex rollouts (`~/.codex/sessions`, `~/.codex/archived_sessions`). Imported threads keep their original session id, so you can keep talking where you left off. Re-running skips threads already imported.
- Redraws only on input or agent output: about 0% CPU when idle.

Shortcuts: `Ctrl N` new thread · `Ctrl B` sidebar · `Ctrl .` changes panel · `Ctrl J` switch agent · `Esc` stop.

The earlier Tauri/React version is kept in `legacy/tauri-web` for reference.

## Contributing

Contributions are welcome! Read [CONTRIBUTING.md](CONTRIBUTING.md) to get set up, and check the open issues for something to work on. Please follow the [Code of Conduct](CODE_OF_CONDUCT.md), and report security issues privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
