# TIRDS - Trading Information Relevance Decider System

An agentic trade decision system that evaluates proposed trades using parallel Claude CLI specialist agents, fed by a shared cache of market data, technical indicators, and sentiment.

External systems submit a `TradeProposal` (JSON) and receive a structured `TradeDecision` with confidence scores, decay projections, price assessments, and trade intelligence.

## Architecture

```
                       External Data Pipelines
                  ┌──────────┬──────────┬───────────────┐
                  │market-data│market-   │trading-data-  │
                  │          │calculations│stream         │
                  └────┬─────┴────┬─────┴─────┬─────────┘
                       │          │           │
                       ▼          ▼           ▼
              ┌────────────────────────────────────────┐
              │     tirds-loader (daemon)               │
              │     Writes to shared SQLite cache       │
              └──────────────────┬─────────────────────┘
                                 │
              ┌──────────────────▼─────────────────────┐
              │     Shared Cache (SQLite + moka)        │
              │     Read by TIRDS, written by loader    │
              └──────────────────┬─────────────────────┘
                                 │ (read-only)
                                 ▼
TradeProposal (JSON) ──> Orchestrator
                              │
                    ┌─────────┼─────────┬──────────┐
                    ▼         ▼         ▼          ▼
              [Technical] [Macro]  [Sentiment] [Sector]
              (Haiku)     (Haiku)  (Haiku)     (Haiku)
                    │         │         │          │
                    └────┬────┘─────────┘──────────┘
                         ▼
                    Synthesizer (Sonnet)
                         │
                         ▼
                   TradeDecision (JSON)
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `tirds-models` | Shared data contracts: `TradeProposal`, `TradeDecision`, `AgentRequest`/`Response`, cache schema, config |
| `tirds-cache` | Read-only cache reader: moka (in-memory hot) → SQLite (shared on disk) |
| `tirds-agents` | Claude CLI orchestration: specialist agents, prompts, JSON extraction, synthesizer |
| `tirds-loader` | Cache writer daemon: populates SQLite from market-data, market-calculations, trading-data-stream |
| `tirds` | Library re-exports + CLI binary |

## Usage

### Evaluating a Trade Proposal

```bash
echo '{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "schema_version": 1,
  "symbol": "AAPL",
  "legs": [{"side": "buy", "price": "150.00", "quantity": "100"}],
  "proposed_at": "2024-02-16T10:30:00Z"
}' | cargo run -p tirds -- --pretty
```

### Running the Cache Loader

```bash
cargo run -p tirds-loader -- --config config/tirds-loader.toml
```

The loader runs as a long-lived daemon that:
- Periodically fetches candle data from market-data and writes `bars:` and `quote:` cache entries
- Computes technical indicators via market-calculations and writes `indicator:` entries
- Subscribes to trading-data-stream for real-time news/sentiment and writes `sentiment:` entries
- Cleans up expired cache entries on a configurable interval

### Configuration

Copy the example configs and customize:

```bash
cp config/tirds.example.toml config/tirds.toml
cp config/tirds-loader.example.toml config/tirds-loader.toml
```

## Cache Schema Contract

The shared SQLite cache uses this schema:

```sql
CREATE TABLE IF NOT EXISTS cache_entries (
    key         TEXT PRIMARY KEY,
    category    TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    source      TEXT NOT NULL,
    symbol      TEXT,
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
```

### Key Patterns

| Pattern | Category | Example |
|---------|----------|---------|
| `bars:{symbol}:{timeframe}` | market_data | `bars:AAPL:5m` |
| `quote:{symbol}` | market_data | `quote:AAPL` |
| `indicator:{name}:{symbol}` | indicator | `indicator:rsi_14:AAPL` |
| `ref:{symbol}` | reference_symbol | `ref:SPY` |
| `sentiment:{source}:{symbol}` | sentiment | `sentiment:news:AAPL` |

All timestamps use RFC3339 format. Entries are automatically filtered by `expires_at` on read.

## Development

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Format
cargo fmt --all -- --check
```

## License

MIT
