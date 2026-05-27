use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug)]
struct Args {
    message_file: PathBuf,
    orderbook_file: PathBuf,
    report_file: Option<PathBuf>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let parse_started = Instant::now();
    let events = lobster_parser::parse_message_file(&args.message_file)?;
    let parse_elapsed = parse_started.elapsed();
    println!("Loaded {} events", events.len());

    let snapshots = validator::parse_orderbook_file(&args.orderbook_file, 10)?;
    println!("Replaying...");

    let validation_started = Instant::now();
    validator::validate_lobster(&events, &snapshots)?;
    let validation_elapsed = validation_started.elapsed();
    println!("Validation passed");

    if let Some(path) = args.report_file {
        let report = build_report(events.len(), parse_elapsed.as_secs_f64(), validation_elapsed.as_secs_f64());
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&path, report)?;
        println!("Wrote report {}", path.display());
    }

    Ok(())
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut message_file = None;
    let mut orderbook_file = None;
    let mut report_file = None;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--message-file" => {
                message_file = Some(PathBuf::from(
                    args.next().ok_or("--message-file requires a path")?,
                ));
            }
            "--orderbook-file" => {
                orderbook_file = Some(PathBuf::from(
                    args.next().ok_or("--orderbook-file requires a path")?,
                ));
            }
            "--report-file" => {
                report_file = Some(PathBuf::from(
                    args.next().ok_or("--report-file requires a path")?,
                ));
            }
            "-h" | "--help" => {
                println!(
                    "Usage: cargo run -p cli -- --message-file <message.csv> --orderbook-file <orderbook.csv> [--report-file <report.json>]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok(Args {
        message_file: message_file.ok_or("missing --message-file")?,
        orderbook_file: orderbook_file.ok_or("missing --orderbook-file")?,
        report_file,
    })
}

fn build_report(event_count: usize, parse_seconds: f64, validation_seconds: f64) -> String {
    let demo = matching_demo();
    let latency = latency_demo();
    let strategy = strategy_demo();
    let events_per_sec = if validation_seconds > 0.0 {
        event_count as f64 / validation_seconds
    } else {
        0.0
    };

    format!(
        concat!(
            "{{\n",
            "  \"dataset\": {{\n",
            "    \"symbol\": \"AAPL\",\n",
            "    \"date\": \"2012-06-21\",\n",
            "    \"source\": \"LOBSTER sample derived from Nasdaq TotalView-ITCH\",\n",
            "    \"events\": {event_count},\n",
            "    \"bookLevels\": 10,\n",
            "    \"validation\": \"passed\"\n",
            "  }},\n",
            "  \"performance\": {{\n",
            "    \"parseSeconds\": {parse_seconds:.6},\n",
            "    \"validationSeconds\": {validation_seconds:.6},\n",
            "    \"eventsPerSecond\": {events_per_sec:.2}\n",
            "  }},\n",
            "  \"matching\": {{\n",
            "    \"scenario\": \"FIFO price-time demo with GTC, IOC, FOK, cancel and replace support\",\n",
            "    \"fills\": {fills},\n",
            "    \"filledQuantity\": {filled_qty},\n",
            "    \"restingOrders\": {resting_orders},\n",
            "    \"bestBid\": {best_bid},\n",
            "    \"bestBidSize\": {best_bid_size},\n",
            "    \"bestAsk\": {best_ask},\n",
            "    \"bestAskSize\": {best_ask_size},\n",
            "    \"reports\": [\n",
            "{reports}\n",
            "    ]\n",
            "  }},\n",
            "  \"latency\": {{\n",
            "    \"ordersSent\": {latency_orders_sent},\n",
            "    \"ordersArrived\": {latency_orders_arrived},\n",
            "    \"ordersDropped\": {latency_orders_dropped},\n",
            "    \"colocatedWins\": {colocated_wins},\n",
            "    \"remoteWins\": {remote_wins},\n",
            "    \"medianColocatedLatencyNs\": {median_colocated_latency_ns},\n",
            "    \"medianRemoteLatencyNs\": {median_remote_latency_ns},\n",
            "    \"avgQueueAdvantageNs\": {avg_queue_advantage_ns},\n",
            "    \"arrivalTape\": [\n",
            "{arrival_tape}\n",
            "    ]\n",
            "  }},\n",
            "  \"strategies\": [\n",
            "{strategies_json}\n",
            "  ],\n",
            "  \"optimizations\": [\n",
            "    \"integer tick prices and nanosecond timestamps\",\n",
            "    \"deterministic fixed-seed latency sampling\",\n",
            "    \"order-id index for cancel/delete/replace lookup\",\n",
            "    \"const-generic fixed-size metric series for strategy curves\",\n",
            "    \"FOK liquidity pre-check before book mutation\",\n",
            "    \"arrival ordering by timestamp plus sequence tie-break\"\n",
            "  ]\n",
            "}}\n"
        ),
        event_count = event_count,
        parse_seconds = parse_seconds,
        validation_seconds = validation_seconds,
        events_per_sec = events_per_sec,
        fills = demo.fills,
        filled_qty = demo.filled_qty,
        resting_orders = demo.resting_orders,
        best_bid = demo.best_bid,
        best_bid_size = demo.best_bid_size,
        best_ask = demo.best_ask,
        best_ask_size = demo.best_ask_size,
        reports = demo.reports_json,
        latency_orders_sent = latency.orders_sent,
        latency_orders_arrived = latency.orders_arrived,
        latency_orders_dropped = latency.orders_dropped,
        colocated_wins = latency.colocated_wins,
        remote_wins = latency.remote_wins,
        median_colocated_latency_ns = latency.median_colocated_latency_ns,
        median_remote_latency_ns = latency.median_remote_latency_ns,
        avg_queue_advantage_ns = latency.avg_queue_advantage_ns,
        arrival_tape = latency.arrival_tape_json,
        strategies_json = strategy
    )
}

struct DemoReport {
    fills: usize,
    filled_qty: u32,
    resting_orders: usize,
    best_bid: i64,
    best_bid_size: u32,
    best_ask: i64,
    best_ask_size: u32,
    reports_json: String,
}

struct LatencyReport {
    orders_sent: usize,
    orders_arrived: usize,
    orders_dropped: usize,
    colocated_wins: usize,
    remote_wins: usize,
    median_colocated_latency_ns: u64,
    median_remote_latency_ns: u64,
    avg_queue_advantage_ns: u64,
    arrival_tape_json: String,
}

fn matching_demo() -> DemoReport {
    use market_events::Side;
    use matching_engine::{MatchingEngine, OrderKind, TimeInForce};

    let mut engine = MatchingEngine::new();
    let reports = vec![
        engine.submit(1, 1001, Side::Sell, 50, OrderKind::Limit { price: 101_00 }, TimeInForce::Gtc),
        engine.submit(2, 1002, Side::Sell, 25, OrderKind::Limit { price: 101_00 }, TimeInForce::Gtc),
        engine.submit(3, 2001, Side::Buy, 100, OrderKind::Limit { price: 102_00 }, TimeInForce::Gtc),
        engine.submit(4, 3001, Side::Buy, 500, OrderKind::Market, TimeInForce::Ioc),
        engine.submit(5, 3002, Side::Buy, 500, OrderKind::Limit { price: 103_00 }, TimeInForce::Fok),
    ];

    let snapshot = engine.snapshot(1);
    let fills = reports.iter().map(|report| report.fills.len()).sum();
    let filled_qty = reports.iter().map(|report| report.filled_qty).sum();
    let reports_json = reports
        .iter()
        .enumerate()
        .map(|(idx, report)| {
            format!(
                "      {{ \"seq\": {}, \"orderId\": {}, \"status\": \"{}\", \"filledQty\": {}, \"remainingQty\": {}, \"fills\": {} }}",
                idx + 1,
                report.order_id,
                status_name(report.status),
                report.filled_qty,
                report.remaining_qty,
                report.fills.len()
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    DemoReport {
        fills,
        filled_qty,
        resting_orders: engine.resting_order_count(),
        best_bid: snapshot.bids[0].price,
        best_bid_size: snapshot.bids[0].qty,
        best_ask: snapshot.asks[0].price,
        best_ask_size: snapshot.asks[0].qty,
        reports_json,
    }
}

fn status_name(status: matching_engine::ReportStatus) -> &'static str {
    match status {
        matching_engine::ReportStatus::Filled => "Filled",
        matching_engine::ReportStatus::PartiallyFilled => "PartiallyFilled",
        matching_engine::ReportStatus::Rested => "Rested",
        matching_engine::ReportStatus::Canceled => "Canceled",
        matching_engine::ReportStatus::Replaced => "Replaced",
        matching_engine::ReportStatus::Expired => "Expired",
        matching_engine::ReportStatus::Rejected => "Rejected",
    }
}

fn latency_demo() -> LatencyReport {
    let result = latency_sim::run_latency_race(42);
    let metrics = result.metrics;
    let arrival_tape_json = result
        .reports
        .iter()
        .take(12)
        .enumerate()
        .map(|(idx, report)| {
            format!(
                "      {{ \"seq\": {}, \"agent\": \"{}\", \"arrivalNs\": {}, \"latencyNs\": {}, \"orderId\": {}, \"filledQty\": {}, \"status\": \"{}\" }}",
                idx + 1,
                report.agent_kind.label(),
                report.arrival_ts,
                report.latency_ns,
                report.report.order_id,
                report.report.filled_qty,
                status_name(report.report.status)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    LatencyReport {
        orders_sent: metrics.orders_sent,
        orders_arrived: metrics.orders_arrived,
        orders_dropped: metrics.orders_dropped,
        colocated_wins: metrics.colocated_wins,
        remote_wins: metrics.remote_wins,
        median_colocated_latency_ns: metrics.median_colocated_latency_ns,
        median_remote_latency_ns: metrics.median_remote_latency_ns,
        avg_queue_advantage_ns: metrics.avg_queue_advantage_ns,
        arrival_tape_json,
    }
}

fn strategy_demo() -> String {
    let suite = strategies::run_strategy_suite();
    [
        strategy_json(&suite.market_maker),
        strategy_json(&suite.order_flow_imbalance),
        strategy_json(&suite.latency_arbitrage),
    ]
    .join(",\n")
}

fn strategy_json(report: &strategies::Report) -> String {
    let curve = report
        .equity_curve_cents
        .as_slice()
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "    {{\n",
            "      \"name\": \"{name}\",\n",
            "      \"pnlCents\": {pnl_cents},\n",
            "      \"pnlDollars\": {pnl_dollars:.2},\n",
            "      \"slippageCents\": {slippage_cents},\n",
            "      \"inventory\": {inventory},\n",
            "      \"maxInventory\": {max_inventory},\n",
            "      \"fills\": {fills},\n",
            "      \"orders\": {orders},\n",
            "      \"fillRatioBps\": {fill_ratio_bps},\n",
            "      \"adverseSelectionCents\": {adverse_selection_cents},\n",
            "      \"queueAheadShares\": {queue_ahead_shares},\n",
            "      \"equityCurveCents\": [{curve}]\n",
            "    }}"
        ),
        name = report.name,
        pnl_cents = report.metrics.pnl_cents,
        pnl_dollars = report.metrics.pnl_dollars(),
        slippage_cents = report.metrics.slippage_cents,
        inventory = report.metrics.inventory,
        max_inventory = report.metrics.max_inventory,
        fills = report.metrics.fills,
        orders = report.metrics.orders,
        fill_ratio_bps = report.metrics.fill_ratio_bps(),
        adverse_selection_cents = report.metrics.adverse_selection_cents,
        queue_ahead_shares = report.metrics.queue_ahead_shares,
        curve = curve
    )
}
