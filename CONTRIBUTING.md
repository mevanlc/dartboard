# Contributing

## Scope

This repository contains the `dartboard` terminal client, the `dartboardd` headless server, and the supporting crates they share.

Contributions are welcome for bug fixes, tests, ergonomics, protocol improvements, rendering correctness, and documentation.

## Before You Start

- Check existing issues and pull requests to avoid duplicate work.
- For larger changes, open an issue first so the shape of the change can be discussed before implementation.
- Keep changes focused. Small, reviewable pull requests are easier to merge than broad refactors mixed with behavior changes.

## Development Setup

This is a Rust workspace. From the repository root:

```bash
just build
just test
just lint
```

If you do not use `just`, the equivalent commands are:

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Making Changes

- Prefer idiomatic Rust and keep APIs small and explicit.
- Add or update tests when behavior changes.
- Update docs when user-facing behavior, controls, flags, or workflows change.
- Avoid unrelated cleanup in the same pull request unless it is necessary for the change.

## Workspace Notes

- `dartboard-cli` builds the `dartboard` TUI binary.
- `dartboard-server` provides the `dartboardd` server binary.
- Shared behavior lives in the reusable crates under the workspace root.

When changing protocol, editor, or rendering behavior, call out which crates are affected and how compatibility is impacted.

## Pull Request Checklist

Before opening a pull request:

- Run `just test`.
- Run `just lint`.
- Confirm the change is described clearly in the PR body.
- Include screenshots, terminal captures, or reproduction steps when the change affects UX or fixes a bug that is easier to verify visually.

## Reporting Bugs

Useful bug reports include:

- what you expected to happen
- what happened instead
- exact steps to reproduce
- platform details
- terminal emulator details when relevant
- logs, screenshots, or short recordings if they help show the problem

## Release Notes

If your change should be called out in release notes, mention that explicitly in the pull request description.
