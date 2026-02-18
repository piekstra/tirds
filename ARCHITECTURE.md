# Architecture

## Purpose

TIRDS (Trading Information Relevance Decider System) is an **agentic trade decision system**. It evaluates trade proposals using parallel Claude specialist agents backed by cached market intelligence. External systems submit a `TradeProposal` and receive a structured `TradeDecision` with confidence scores, decay projections, price assessments, and reasoning.

## Core Principles

1. **Self-sufficient data acquisition** — TIRDS knows how to get the data it needs. The `tirds-loader` daemon dynamically fetches missing market data from providers (Yahoo Finance, Alpaca) via the [market-data](https://github.com/piekstra/market-data) library, caches it permanently as local Parquet files, and serves it to agents via the SQLite hot cache. Historical prices are fetched once and never need re-fetching. The evaluator itself is read-only — it reads from the shared cache and synthesizes decisions.

2. **Agent orchestration, not AI monolith** — Decisions are made by parallel domain-specialist agents (technical, macro, sentiment, sector) whose outputs are synthesized by a separate aggregator. Each specialist is independently configurable and can fail gracefully without blocking the others.

3. **Cache as contract boundary** — The shared SQLite database is the integration point between data producers (`tirds-loader`) and data consumers (the evaluator). TIRDS never writes to the cache; the loader never reads decisions. WAL mode enables concurrent access.

4. **Deterministic data flow** — Given the same cache state and the same trade proposal, the only source of non-determinism is the LLM inference. All other data paths are deterministic reads.

5. **Delegate computation** — Technical indicators are computed by [market-calculations](https://github.com/piekstra/market-calculations). Market data is sourced from [market-data](https://github.com/piekstra/market-data). Real-time streams come from [trading-data-stream](https://github.com/piekstra/trading-data-stream). TIRDS orchestrates — it doesn't reimplement.

6. **Efficiency through layered caching** — Data flows through three tiers: provider APIs (fetch once) → local Parquet files (permanent, historical) → SQLite + moka (hot, TTL-based). Agents always read from the hot cache. The loader fills gaps from providers only when local data is missing.

## Crate Structure

```
tirds/
├── crates/
│   ├── tirds-models/      Pure data contracts (TradeProposal, TradeDecision, CacheRow, configs)
│   ├── tirds-cache/       Two-tier read-only cache (moka in-memory + SQLite on disk)
│   ├── tirds-agents/      Claude CLI orchestration (specialists + synthesizer)
│   ├── tirds-loader/      Cache writer daemon (fetches data, computes indicators, writes SQLite)
│   └── tirds/             Binary entry point + library facade
```

### tirds-models
Shared types with no business logic. Defines `TradeProposal`, `TradeDecision`, `AgentRequest`/`AgentResponse`, `CacheRow`, cache key conventions, and all configuration structs.

### tirds-cache
Read-through cache. Checks moka (hot, in-memory) first, then SQLite (shared on disk), promotes hits to moka. Filters expired entries by `expires_at` timestamp. Provides `build_domain_snapshot()` to pre-fetch all data for a symbol in one call.

### tirds-agents
Orchestrator fans out `AgentRequest`s to specialist agents in parallel (tokio tasks). Each specialist invokes the Claude CLI as a subprocess with a domain-specific system prompt and the domain data snapshot. The synthesizer (separate, higher-capability model) aggregates all specialist reports into the final `TradeDecision`. Specialists use the `SpecialistAgent` trait, which is mockable for testing.

### tirds-loader
Long-running daemon with three concurrent loops:
- **Market data + calculations loop** — fills missing candles from providers (Yahoo/Alpaca), reads from local Parquet store, computes indicators via `market-calculations` Pipeline, writes results to SQLite
- **Stream loop** — subscribes to `trading-data-stream` for news, sentiment, filings, economic data
- **Cleanup loop** — purges expired cache entries

### tirds (binary)
Reads `TradeProposal` JSON from stdin or file, constructs the orchestrator, evaluates, outputs `TradeDecision` JSON to stdout.

## Data Flow

```
                        tirds-loader (daemon)
                        ┌──────────────────────┐
  Yahoo/Alpaca APIs ────→│ fill missing data     │──→ Parquet (permanent)
  market-data (local) ──→│ read candles          │
  market-calculations ──→│ compute indicators    │──→ SQLite cache (WAL)
  trading-data-stream ──→│ stream news/sentiment │        │
                        └──────────────────────┘        │ (read-only)
                                                        ↓
  TradeProposal ──→ Orchestrator ──→ build_domain_snapshot()
                        │
                ┌───────┼───────┬──────────┐
                ↓       ↓       ↓          ↓
           Technical  Macro  Sentiment  Sector    (parallel Claude CLI)
                ↓       ↓       ↓          ↓
                └───────┼───────┴──────────┘
                        ↓
                    Synthesizer (Claude CLI)
                        ↓
                   TradeDecision
```

## Boundaries

### TIRDS IS responsible for:
- Evaluating trade proposals via agent orchestration
- Cache reading (two-tier: moka + SQLite)
- Agent prompt engineering and specialist domain rules
- `TradeDecision` schema and contract
- Cache population via `tirds-loader` (separate daemon)

### TIRDS is NOT responsible for:
- Market data provider implementations — delegated to [market-data](https://github.com/piekstra/market-data)
- Technical indicator computation — delegated to [market-calculations](https://github.com/piekstra/market-calculations)
- Real-time data streams — delegated to [trading-data-stream](https://github.com/piekstra/trading-data-stream)
- Trade execution or order management
- Portfolio tracking or position management
- AI model hosting — uses Claude CLI (Anthropic's official CLI)

## Cache Schema

```
Key patterns:
  bars:{symbol}:{timeframe}       OHLCV candle data
  quote:{symbol}                  Latest quote
  indicator:{name}:{symbol}       Technical indicators (rsi_14, sma_20, etc.)
  ref:{symbol}                    Reference symbols (SPY, VIX, QQQ, sector ETFs)
  sentiment:{source}:{symbol}     News/social sentiment

Categories: market_data, indicator, reference_symbol, sentiment, subscription
Each entry has: key, category, value_json, source, symbol, created_at, expires_at, updated_at
```

## Agent Architecture

| Agent | Domain | Model | Weight |
|-------|--------|-------|--------|
| Technical | RSI, MACD, moving averages, Bollinger, ATR, stochastic, OBV | Haiku (fast) | 0.35 |
| Macro | VIX, SPY trend, sector rotation, market direction | Haiku (fast) | 0.20 |
| Sentiment | News, social sentiment, analyst ratings, recency | Haiku (fast) | 0.20 |
| Sector | Sector ETF relative performance, rotation signals | Haiku (fast) | 0.25 |
| Synthesizer | Aggregation, confidence decay, price projections | Sonnet (reasoning) | — |

Specialists are independently configurable (enable/disable, model override) via `tirds.toml`.
