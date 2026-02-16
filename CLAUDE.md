# TIRDS - Trading Information Relevance Decider System

## Architecture

Rust workspace with 5 crates:
- `tirds-models` - Shared data contracts (no logic). All structs derive `Serialize, Deserialize`.
- `tirds-cache` - Read-only cache reader (shared SQLite + moka in-memory hot layer).
- `tirds-agents` - Agentic decision layer (Claude CLI subprocesses, orchestrator, specialist agents).
- `tirds-loader` - Cache writer daemon. Populates SQLite from market-data, market-calculations, and trading-data-stream.
- `tirds` - Binary + lib re-export facade.

## Key Conventions

- All monetary values use `rust_decimal::Decimal` (string-serialized for precision).
- All timestamps use `chrono::DateTime<Utc>`.
- All IDs use `uuid::Uuid`.
- Config is TOML format (`config/tirds.toml`). Example at `config/tirds.example.toml`.
- Tracing via `tracing` crate. Set `RUST_LOG=tirds=debug` for verbose output.
- Logs go to stderr, structured JSON output goes to stdout.

## Cache Contract

TIRDS reads from a shared SQLite database populated by `tirds-loader`.
The expected schema is defined in `tirds-models/src/cache_schema.rs` (`CACHE_TABLE_DDL`).
Key patterns are defined in `cache_schema::key_patterns`.
The loader uses WAL mode for concurrent read/write access.

## Loader (tirds-loader)

Long-running daemon with three concurrent tasks:
1. **Market data + calculations loop** (periodic) - fetches candles via `market-data-core`, writes bars/quotes, computes indicators via `market-calculations`.
2. **Stream loop** (real-time) - subscribes to `tds` StreamManager for news, sentiment, filings, economic data.
3. **Stale cleanup** (periodic) - purges expired cache entries.

Config: `config/tirds-loader.toml`. Example at `config/tirds-loader.example.toml`.

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
cargo test -p tirds-loader    # Loader only
```

## Running

```bash
# Via stdin
echo '{"id":"...","schema_version":1,"symbol":"AAPL",...}' | cargo run -- -c config/tirds.toml

# Via file
cargo run -- -c config/tirds.toml -i proposal.json --pretty

# Run the cache loader daemon
cargo run -p tirds-loader -- -c config/tirds-loader.toml
```
