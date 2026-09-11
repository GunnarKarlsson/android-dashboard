# Contributing

Thank you for your interest in contributing to Android Debug Dashboard. This document explains how to contribute to the project.

## Good first contributions

These areas are a solid place to start:

- **Improve how data is shown in an existing panel** — clearer layout, denser or more readable presentation, better empty/error states, additional useful data.
- **Add a new panel with new data** — surface additional device or adb-derived information in the dashboard UI.
- **Better filtering for insights** — refine how error logs are selected, noise-stripped, or presented before and after insight generation.

If you are unsure whether an idea fits, open an issue first and describe the intended user-facing change.

## Discuss internals before large work

Changes that touch core internals — for example threading, process lifetime, shared adb locking, or how pollers and logcat interact — should be discussed with the maintainer before you invest significant time. Open an issue or draft PR with the problem and proposed approach so scope and design can be agreed early.

## Development setup

See the [README](README.md) for prerequisites (Rust toolchain, `adb` on `PATH`) and how to build and run the app:

```bash
cargo build
cargo run -p android-dashboard
```

The workspace uses the toolchain in `rust-toolchain.toml` (currently Rust 1.88.0 with `clippy`, `rustfmt`, and `rust-analyzer`).

Running or manually testing the UI needs a connected Android device or emulator. Most `cargo test` work does not.

## Before you open a pull request

### Keep your branch current with `main`

Always rebase or merge the latest `main` into your branch before opening or updating a PR. That keeps history reviewable and avoids merge conflicts on GitHub:

```bash
git fetch origin
git merge origin/main
# or: git rebase origin/main
```

Resolve any conflicts locally, re-run the checks below, then push.

### Run the same checks as CI

GitHub Actions runs the jobs in [`.github/workflows/ci.yml`](.github/workflows/ci.yml). `cargo-deny` runs as its own Ubuntu job via [EmbarkStudios/cargo-deny-action](https://github.com/EmbarkStudios/cargo-deny-action); the fmt, build, clippy, and test job runs on macOS. Run the equivalent commands locally before you push or request review so failures surface on your machine first.

Install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if you do not already have it, then:

```bash
cargo deny check
cargo fmt --all -- --check
cargo build --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Fix formatting with `cargo fmt --all` if the fmt check fails. Do not submit with Clippy warnings; CI treats warnings as errors (`-D warnings`).

## Pull request guidelines

- Prefer small, focused PRs that are easy to review. Small PRs are usually reviewed within a few days; larger ones may take longer.
- Describe **what** changed and **why** in the PR body. Link related issues when applicable.
- Match existing code style and crate boundaries (`adb-client`, `ai-insight`, `android-dashboard`).
- Do not commit secrets — for example API keys, `insight.json` with credentials, or machine-specific paths.
- Ensure CI checks pass on the tip of your branch.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE) that covers this project.
