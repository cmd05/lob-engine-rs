
## Project Overview

**lob-engine-rs** is a modular, production-grade Rust framework for simulating and analyzing market microstructure, order book dynamics, latency effects, and trading strategies. It is designed for deterministic replay, exchange-style matching, latency competition, and execution research, validated against real LOBSTER data.

---

## Repository Structure

**Workspace Layout:**

- `crates/` — All core logic, each as an independent Rust crate:
  - `analytics/` — Strategy metrics, histograms, and reporting utilities.
  - `benchmarks/` — Performance and latency benchmarking tools.
  - `cli/` — Command-line interface for validation and reporting.
  - `latency_sim/` — Deterministic agent latency and queue-race simulation.
  - `lobster_parser/` — Robust LOBSTER CSV parsing.
  - `market_events/` — Canonical event model and core types.
  - `matching_engine/` — Exchange-style matching engine with advanced order types.
  - `orderbook/` — Deterministic replay order book implementation.
  - `replay_engine/` — Timestamp-ordered event replay engine.
  - `server/` — HTTP/SSE backend for the live web lab.
  - `strategies/` — Deterministic strategy experiments and analytics.
  - `validator/` — Event-by-event LOBSTER book validation.
- `data/` — Canonical input datasets (LOBSTER sample files, orderbook CSVs).
- `docs/` — Performance and optimization notes.
- `web/` — Live exchange web lab (frontend visualization).

---

## Library Usage & Integration

This workspace is designed for both end-to-end simulation and as a set of reusable libraries. Each crate exposes a focused API. Example: using the `orderbook` and `market_events` crates in your own project:

```toml
[dependencies]
orderbook = { path = "../lob-engine-rs/crates/orderbook" }
market_events = { path = "../lob-engine-rs/crates/market_events" }
```

```rust
use orderbook::OrderBook;
use market_events::{MarketEvent, Side};

fn main() {
    let mut book = OrderBook::new();
    let event = MarketEvent::AddLimit {
        ts: 0,
        order_id: 1,
        side: Side::Buy,
        price: 10000,
        qty: 10,
    };
    book.apply(&event).unwrap();
    let snapshot = book.snapshot(10);
    println!("Top of book: {:?}", snapshot.top_n(1));
}
```

---

## Crate-by-Crate API Reference

### `market_events`
Defines the canonical event model and core types for all crates.

- **Types:**
  - `MarketEvent`: Enum for all order book actions (add, cancel, delete, execution, halt).
  - `Side`, `OrderId`, `Price`, `Quantity`, `Timestamp`: Core type aliases and enums.
- **Usage:** Used as the lingua franca for all event-driven logic.

### `orderbook`
Implements a deterministic, replayable order book for limit order markets.

- **Types:**
  - `OrderBook`: Main struct, supports `apply(&MarketEvent)`, `apply_lobster_delta(&MarketEvent)`, `snapshot(levels)`.
  - `Snapshot`, `Level`: Represent visible book state.
  - `BookError`: Error enum for invalid operations.
- **Key Methods:**
  - `OrderBook::new()`: Create a new, empty book.
  - `OrderBook::apply(&MarketEvent)`: Apply a normalized event.
  - `OrderBook::snapshot(levels)`: Get a visible book snapshot.
  - `OrderBook::apply_lobster_delta(&MarketEvent)`: Apply LOBSTER-style deltas.

### `matching_engine`
Exchange-style matching engine with support for advanced order types and time-in-force.

- **Types:**
  - `MatchingEngine`: Main struct for order matching.
  - `OrderRequest`: Enum for order submission, cancel, replace.
  - `OrderKind`, `TimeInForce`, `ExecutionReport`, `ReportStatus`, `Fill`.
- **Key Methods:**
  - `MatchingEngine::new()`, `submit(OrderRequest)`, `cancel(OrderId)`, `replace(...)`.

### `latency_sim`
Deterministic simulation of agent latency, queue races, and packet loss.

- **Types:**
  - `LatencySimulator`: Main orchestrator for agent scheduling.
  - `Agent`, `LatencyModel`, `ScheduledOrder`, `RaceMetrics`.
- **Key Methods:**
  - `LatencySimulator::new(seed)`, `add_agent(Agent)`, `schedule(&[OutboundOrder])`.

### `analytics`
Fixed-size metrics, histograms, and reporting for strategies and performance.

- **Types:**
  - `StrategyMetrics`, `StrategyReport`, `FixedSeries`, `FixedHistogram`.

### `strategies`
Deterministic strategy experiments for market making, order-flow imbalance, and latency arbitrage.

- **Types:**
  - `StrategySuite`: Bundles all strategies.
- **Key Methods:**
  - `run_strategy_suite()`: Returns a `StrategySuite` with results for each strategy.

### `lobster_parser`
Robust CSV parser for LOBSTER message and orderbook files.

- **Key Methods:**
  - `parse_message_file(path) -> Result<Vec<MarketEvent>, ParseError>`
  - `parse_orderbook_file(path, levels) -> Result<Vec<Snapshot>, ValidationError>`

### `validator`
Event-by-event validation of reconstructed book state against LOBSTER ground truth.

- **Key Methods:**
  - `validate_lobster(events, snapshots)`

### `replay_engine`
Timestamp-ordered event replay and snapshotting.

- **Key Methods:**
  - `ReplayEngine::replay(events)`
  - `ReplayEngine::snapshot(levels)`

### `benchmarks`
Performance and latency benchmarking for all core components.

### `cli`
Command-line interface for validation, reporting, and automation.

### `server`
HTTP/SSE backend for the live web lab (see `web/`).

---

## Example Workflows

### Full Dataset Validation

```sh
cargo run -p cli -- --message-file data/AAPL_2012-06-21_34200000_57600000_message_10.csv --orderbook-file data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv
```

### Generate JSON Report

```sh
cargo run -p cli -- --message-file data/AAPL_2012-06-21_34200000_57600000_message_10.csv --orderbook-file data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv --report-file docs/report.json
```

### Run Benchmarks

```sh
cargo run --release -p benchmarks -- --output-file docs/PERFORMANCE.md
```

### Start the Live Web Lab

```sh
cargo run -p server
# Then open http://127.0.0.1:8080
```

---

## Data & Validation Model

- LOBSTER sample files and orderbook CSVs are in the `data/` directory.
- The validator compares reconstructed book state against the published 10-level LOBSTER orderbook after each event, failing on mismatches.
- See `docs/` for performance notes and optimization details.

---

## References

- [LOBSTER Data](https://lobsterdata.com/)
- [Nasdaq TotalView-ITCH](https://www.nasdaqtrader.com/Trader.aspx?id=Totalview2)

---

For further details, see the README.md and inline crate documentation. For API specifics, consult the Rustdoc output for each crate.
