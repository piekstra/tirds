# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Agentic trade decision pipeline with four specialist agents (technical, macro, sentiment, sector) and a synthesizer.
- Two-tier cache (moka in-memory hot layer + SQLite cold layer) with read-through promotion.
- `tirds-loader` daemon with three concurrent tasks: market data + calculations, stream ingestion, stale cleanup.
- Trading signal interpretation rules with confidence adjustments and warning conditions.
- Scenario-based integration tests covering oversold, overbought, uptrend, downtrend, and sector rotation.
- Demo workflow with sample proposals and CLI support.
- CI pipeline: format check, clippy, full test suite, doc build.
- Release workflow: builds and uploads binary on version tags.
- `ARCHITECTURE.md` documenting core principles and system boundaries.
