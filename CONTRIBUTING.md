# Contributing to Open Agent

Thanks for your interest in improving Open Agent! Bug reports, feature ideas, docs fixes and pull requests are all welcome.

## Getting started

1. Install [Rust](https://rustup.rs) (stable).
2. Install and log in to at least one agent CLI:
   - [Claude Code](https://docs.claude.com/claude-code): `claude` (Pro/Max login)
   - [Codex](https://github.com/openai/codex): `codex login`
3. Fork the repo, then:

```bash
git clone https://github.com/<you>/open-agent.git
cd open-agent
cargo run
```

## Project layout

| Path            | What lives there                                        |
| --------------- | ------------------------------------------------------- |
| `src/`          | The native egui app (UI, agent process driver, models)  |
| `assets/`       | Icons and logos                                         |
| `build.rs`      | Embeds the Windows icon/metadata into the executable    |
| `legacy/`       | The old Tauri/React version, kept for reference only    |

New work should target `src/`. Please don't send changes to `legacy/` unless they're fixes for reference purposes.

## Making a change

1. Open an issue first for anything bigger than a small fix, so we can agree on the approach.
2. Create a branch from `main`: `git checkout -b fix/short-description`.
3. Keep PRs focused: one feature or fix per PR.
4. Before pushing, run:

```bash
cargo fmt
cargo clippy --all-targets
cargo build --release
```

5. Test your change by actually running the app with the agent(s) it touches.
6. Open a pull request using the template, describing **what** changed and **why**. Screenshots or a short GIF help a lot for UI changes.

## Guidelines

- Keep the app lightweight: it should stay near 0% CPU when idle (redraw only on input or agent output).
- Avoid adding heavy dependencies; explain why in the PR if you need one.
- Never commit secrets, tokens, or personal session files.
- Write clear commit messages (e.g. `Fix diff panel scroll on long files`).

## Reporting bugs

Use the **Bug report** issue template and include your OS, Open Agent version, which agent (Claude/Codex) and CLI version, and steps to reproduce.

## Security issues

Please do **not** open a public issue for vulnerabilities. See [SECURITY.md](SECURITY.md).

## Code of Conduct

By participating you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
