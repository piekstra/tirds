# Contributing

## Prerequisites

- **Rust stable toolchain** (with `clippy`, `rustfmt`, and `llvm-tools-preview` components)
- **Git access** to the private dependency repos:
  - [market-data](https://github.com/piekstra/market-data)
  - [market-calculations](https://github.com/piekstra/market-calculations)
  - [trading-data-stream](https://github.com/piekstra/trading-data-stream)

Cargo fetches these via HTTPS. If you use SSH, configure git to rewrite URLs:

```bash
git config --global url."git@github.com:".insteadOf "https://github.com/"
```

### Optional tools

- **[just](https://github.com/casey/just)** — task runner. `just check` runs fmt + clippy + test in one command.
- **[cargo-deny](https://github.com/EmbarkStudios/cargo-deny)** — license and advisory checking. `cargo deny check`.
- **[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)** — test coverage reporting. `cargo llvm-cov --workspace`.

## Setup

After cloning, configure git to use the shared hooks:

```bash
git config core.hooksPath .githooks
```

This enables the pre-commit hook that auto-formats Rust code and runs clippy before each commit.

## Building

```bash
cargo build --workspace
```

## Testing

```bash
# Full suite (what CI runs)
cargo test --workspace

# Single crate
cargo test -p tirds-models
cargo test -p tirds-cache
cargo test -p tirds-agents
cargo test -p tirds-loader

# Or with just
just check       # fmt + clippy + test
just test-crate tirds-agents  # single crate
```

All agent tests use mocks — no Claude CLI or API credentials are needed to run the test suite.

## Code Style

The pre-commit hook auto-formats and runs clippy (see [Setup](#setup)). CI also enforces:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

### Conventions

- **Monetary values**: `rust_decimal::Decimal` (string-serialized)
- **Timestamps**: `chrono::DateTime<Utc>`
- **IDs**: `uuid::Uuid`
- **Errors**: `thiserror` derive macros — no `unwrap()` or `expect()` in production code
- **Logging**: `tracing` crate (`debug!`, `warn!`, `error!`)
- **Config**: TOML format, parsed into typed structs in `tirds-models`

## Submitting Changes

1. Create a branch from `main`
2. Make your changes, including tests for new behavior
3. Run `just check` (or `cargo fmt && cargo clippy ... && cargo test --workspace`)
4. Open a PR against `main` — CI will run automatically

## Data Sources

When adding new data integrations or information sources, prefer free and publicly available sources over paid subscriptions. If a paid source has a free alternative that provides comparable data quality, use the free option.
