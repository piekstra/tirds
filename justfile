# TIRDS development commands
# Install just: https://github.com/casey/just

# Run all checks (format, lint, test) — same as CI
check: fmt clippy test

# Auto-format all Rust code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy with warnings as errors
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run the full test suite
test:
    cargo test --workspace

# Run tests for a single crate (e.g., just test-crate tirds-agents)
test-crate crate:
    cargo test -p {{ crate }}

# Build the workspace
build:
    cargo build --workspace

# Build documentation
doc:
    cargo doc --workspace --no-deps

# Run the CLI integration tests (requires Claude CLI + credentials)
test-cli:
    cargo test -p tirds-agents --test cli_integration -- --ignored
