# lob-engine-rs

`lob-engine-rs` is a Rust market microstructure simulator for deterministic replay, exchange-style matching, latency competition, and execution research. The project uses the LOBSTER AAPL sample derived from Nasdaq TotalView-ITCH and validates reconstructed 10-level book state against the supplied ground-truth orderbook file.

## Demo

This is a demo of the web lab connected to the Rust backend.

https://github.com/user-attachments/assets/128f6824-e570-4fdf-91eb-d84a422d9c05

## Highlights

- Deterministic replay of real Nasdaq-derived order flow.
- Validation against the LOBSTER 10-level orderbook CSV.
- Normalized market events independent of the source data format.
- FIFO order book and synthetic matching engine with market, limit, cancel, replace, IOC, and FOK support.
- Deterministic co-location latency simulator with jitter, packet loss, and arrival-time ordering.
- Strategy experiments for inventory-aware market making, order-flow imbalance, and latency arbitrage.
- Release-mode benchmarks, p50/p99 hot-path timing, and flamegraph.
- Live web lab backed by Rust server-sent events and Chart.js visualizations.

See [DOCUMENTATION.md](./DOCUMENTATION.md) for details regarding code structure and usage as a library.

## Metrics

See [PERFORMANCE.md](./PERFORMANCE.md) for detailed metrics evaluation.

Latest local benchmark snapshot:

| Area | Metric | Result |
| --- | --- | ---: |
| Dataset replay | LOBSTER events validated | 400,391 |
| Correctness | 10-level orderbook validation | Passed |
| Parser throughput | Message CSV parse | 3.88M events/sec |
| Snapshot parsing | Orderbook CSV parse | 710K snapshots/sec |
| Replay throughput | Validated replay | 1.03M events/sec |
| Matching engine | Synthetic matching | 8.89M orders/sec |
| Latency simulator | Scheduler throughput | 9.08M orders/sec |
| Strategy suite | Deterministic strategy runs | 139K runs/sec |
| Replay hot path | p50 / p99 update plus snapshot | 600 ns / 1.2 us |
| Matching hot path | p50 / p99 synthetic process | 100 ns / 200 ns |
| Latency race | Colocated wins vs remote wins | 10 / 0 |
| Latency race | Average queue advantage | 24.0 ms |
| Market making demo | PnL / fill ratio | $12.50 / 29.68% |

| Benchmark | Work | Time | Throughput |
| --- | ---: | ---: | ---: |
| message_parse | 400391 events | 0.103 s | 3876313.39 events/s |
| orderbook_parse | 400391 snapshots | 0.563 s | 710604.07 snapshots/s |
| validated_replay | 400391 events | 0.389 s | 1028823.64 events/s |
| synthetic_matching | 100000 orders | 0.011 s | 8894739.65 orders/s |
| latency_scheduler | 50000 orders | 0.006 s | 9082157.19 orders/s |
| strategy_suite | 2000 runs | 0.014 s | 139248.61 runs/s |

| Hot path | Samples | p50 | p99 |
| --- | ---: | ---: | ---: |
| lobster_replay_update_plus_snapshot | 50000 | 600 ns | 1.200 us |
| synthetic_matching_process | 50000 | 100 ns | 200 ns |

## Installation

Requirements:

- Rust stable toolchain with Cargo.
- A modern browser for the live web lab.
- Optional: `cargo-flamegraph` for profiling.

Clone or open the repository, then verify the toolchain:

```bash
cargo --version
rustc --version
```

The sample input files are expected under `data/`:

```text
data/AAPL_2012-06-21_34200000_57600000_message_10.csv
data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv
data/LOBSTER_SampleFiles_ReadMe.txt
```

## Usage

Run the full test suite:

```bash
cargo test --workspace
```

Validate the LOBSTER dataset:

```bash
cargo run -p cli -- \
  --message-file data/AAPL_2012-06-21_34200000_57600000_message_10.csv \
  --orderbook-file data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv
```

Expected output:

```text
Loaded 400391 events
Replaying...
Validation passed
```

Generate a JSON report:

```bash
cargo run -p cli -- \
  --message-file data/AAPL_2012-06-21_34200000_57600000_message_10.csv \
  --orderbook-file data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv \
  --report-file docs/report.json
```

Run benchmarks and refresh the performance document:

```bash
cargo run --release -p benchmarks -- --output-file docs/PERFORMANCE.md
```

Start the live web lab:

```bash
cargo run -p server
```

Then open:

```text
http://127.0.0.1:8080
```

The web lab streams directly from the Rust backend:

- LOBSTER replay events and validated book snapshots.
- FIFO matching queue consumption.
- Co-located versus remote latency races.
- Strategy equity and inventory behavior.

## Correctness Model

LOBSTER provides a published 10-level visible orderbook after each message event. The validator compares reconstructed visible state against that ground truth event by event and fails immediately on mismatches with the event index, expected snapshot, and actual snapshot.

The sample message file is scoped to the requested visible depth, so some off-window activity is not sufficient to reconstruct unlimited depth. The validator therefore enforces observable top-10 transitions and resynchronizes to the published 10-level state when off-window liquidity re-enters visibility.

## Systems Techniques

- Integer tick prices and integer nanosecond timestamps avoid floating-point drift.
- Deterministic replay and fixed-seed latency sampling make runs reproducible.
- Ordered price levels with FIFO queues implement price-time priority.
- Order ID indexes provide direct cancel, delete, execution, and replace lookup.
- FOK orders perform liquidity pre-checks before book mutation.
- Arrival ordering uses `(arrival_ts, sequence)` for deterministic tie-breaking.
- Packet loss is modeled as integer parts-per-million.
- Strategy analytics use const-generic fixed-size series for bounded memory behavior.

## Profiling

Install flamegraph tooling:

```bash
cargo install flamegraph
```

The release profile includes debug symbols so flamegraph samples resolve to useful Rust symbols.
