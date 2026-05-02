# Contributing to mail-mcp

Thanks for your interest. This guide is intentionally short — most decisions are documented inline in [`CLAUDE.md`](./CLAUDE.md), [`docs/superpowers/specs/`](./docs/superpowers/specs/), and the per-milestone plans in [`docs/superpowers/plans/`](./docs/superpowers/plans/).

## Before you start

- Read the milestone status table in the [README](./README.md). v0.1 is still pre-release; the API surface and IPC schema may change between letter milestones.
- Check open issues + draft PRs before writing a new feature. The project has a per-milestone plan; feature work usually slots into an existing task.
- For larger changes, open an issue first describing the change so we can sanity-check scope.

## Building from source

The Rust workspace builds with the pinned toolchain in `rust-toolchain.toml`:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Tray apps build outside the cargo workspace; see each tray's README:
- macOS: `tray-app/README.md`
- Windows: `tray-app-win/README.md`
- Linux (planned): `tray-app-linux/README.md`

## Branch + PR conventions

- Branch from `main`. Name `vX.Y<letter>-<topic>` if your work targets a milestone, else describe the topic (`fix-oauth-redirect`, `docs-readme-cleanup`).
- One logical change per PR. We squash on merge.
- CI must pass before merge: cross-compile checks (Linux/macOS/Windows targets), Rust unit tests on all hosts, tray app builds.
- Add tests for new behavior. Bug fixes should include a regression test.

## Style

- Rust: rustfmt + clippy with default settings. The `rust-toolchain.toml` pin matters.
- C# (Windows tray): `TreatWarningsAsErrors=true`. New warnings must be fixed, not suppressed.
- Default to no comments. Add a comment only when the WHY is non-obvious. Don't explain WHAT well-named code already does.

## Commits

- Follow Conventional Commits: `feat:`, `fix:`, `docs:`, `ci:`, `test:`, `refactor:`. Scope is optional but useful: `feat(core):`, `fix(tray-win):`.
- The first line is short (under 70 chars). Use the body for the WHY — what changed, why this approach, what alternatives were considered.

## Reporting bugs

Open an issue using the appropriate template. Include:
- Version (`mail-mcp-daemon --version`), OS, install method
- Steps to reproduce
- Expected vs. actual behavior
- Relevant log output (`~/.local/state/mail-mcp/logs/` or `%LOCALAPPDATA%\mail-mcp\logs\`)

## Security issues

See [`SECURITY.md`](./SECURITY.md). Do **not** open a public issue for vulnerabilities.

## License

By contributing, you agree your work is licensed under both the MIT License and the Apache License 2.0 (the same dual license as the project). See [`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).
