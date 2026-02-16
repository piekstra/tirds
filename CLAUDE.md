# TIRDS - Trading Information Relevance Decider System

## Architecture

Rust workspace with 4 crates:
- `tirds-models` - Shared data contracts (no logic). All structs derive `Serialize, Deserialize`.
- `tirds-cache` - Read-only cache reader (shared SQLite + moka in-memory hot layer).
- `tirds-agents` - Agentic decision layer (Claude CLI subprocesses, orchestrator, specialist agents).
- `tirds` - Binary + lib re-export facade.

## Key Conventions

- All monetary values use `rust_decimal::Decimal` (string-serialized for precision).
- All timestamps use `chrono::DateTime<Utc>`.
- All IDs use `uuid::Uuid`.
- Config is TOML format (`config/tirds.toml`). Example at `config/tirds.example.toml`.
- Tracing via `tracing` crate. Set `RUST_LOG=tirds=debug` for verbose output.
- Logs go to stderr, structured JSON output goes to stdout.

## Cache Contract

TIRDS reads from a shared SQLite database written by an external data pipeline.
The expected schema is defined in `tirds-models/src/cache_schema.rs` (`CACHE_TABLE_DDL`).
Key patterns are defined in `cache_schema::key_patterns`.

## Agent Model

- Specialist agents (Haiku) run as parallel `claude` CLI subprocesses.
- Synthesizer (Sonnet) aggregates specialist reports into a `TradeDecision`.
- `SpecialistAgent` trait is mockable for testing.
- JSON extraction handles markdown-wrapped, prefix-text, and clean JSON responses.

## Testing

```bash
cargo test --workspace        # All tests
cargo test -p tirds-models    # Models only
cargo test -p tirds-cache     # Cache only
cargo test -p tirds-agents    # Agents only
```

## Running

```bash
# Via stdin
echo '{"id":"...","schema_version":1,"symbol":"AAPL",...}' | cargo run -- -c config/tirds.toml

# Via file
cargo run -- -c config/tirds.toml -i proposal.json --pretty
```
