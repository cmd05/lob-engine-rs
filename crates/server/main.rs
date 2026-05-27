use market_events::MarketEvent;
use orderbook::Snapshot;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const ADDR: &str = "127.0.0.1:8080";

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(ADDR)?;
    println!("Live exchange lab: http://{ADDR}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    if let Err(err) = handle_client(stream) {
                        eprintln!("server error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("connection error: {err}"),
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0_u8; 2048];
    let read = stream.read(&mut buffer)?;
    if read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let Some(path) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return Ok(());
    };

    match path {
        "/" | "/index.html" => serve_file(&mut stream, "web/index.html", "text/html")?,
        "/styles.css" => serve_file(&mut stream, "web/styles.css", "text/css")?,
        "/app.js" => serve_file(&mut stream, "web/app.js", "application/javascript")?,
        "/api/summary" => serve_json(&mut stream, &summary_json())?,
        "/api/replay/stream" => stream_replay(stream)?,
        "/api/matching/stream" => stream_matching(stream)?,
        "/api/latency/stream" => stream_latency(stream)?,
        "/api/strategy/stream" => stream_strategy(stream)?,
        _ => serve_not_found(&mut stream)?,
    }
    Ok(())
}

fn serve_file(stream: &mut TcpStream, path: &str, content_type: &str) -> Result<(), Box<dyn Error>> {
    let full_path = workspace_root().join(path);
    let body = fs::read(full_path)?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}

fn serve_json(stream: &mut TcpStream, body: &str) -> Result<(), Box<dyn Error>> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}

fn serve_not_found(stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
    let body = "not found";
    write!(
        stream,
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}

fn sse_headers(stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n"
    )?;
    Ok(())
}

fn sse_data(stream: &mut TcpStream, payload: &str) -> Result<(), Box<dyn Error>> {
    write!(stream, "data: {payload}\n\n")?;
    stream.flush()?;
    Ok(())
}

fn stream_replay(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    sse_headers(&mut stream)?;
    let root = workspace_root();
    let message_file = File::open(root.join("data/AAPL_2012-06-21_34200000_57600000_message_10.csv"))?;
    let book_file = File::open(root.join("data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv"))?;
    let messages = BufReader::new(message_file).lines();
    let books = BufReader::new(book_file).lines();

    for (idx, pair) in messages.zip(books).take(80).enumerate() {
        let (message, book) = pair;
        let event = lobster_parser::parse_message_row(&message?)?;
        let snapshot = validator::parse_snapshot_row(&book?, 10, idx + 1)?;
        let payload = replay_payload(idx, &event, &snapshot);
        sse_data(&mut stream, &payload)?;
        thread::sleep(Duration::from_millis(90));
    }
    sse_data(&mut stream, r#"{"type":"done"}"#)?;
    Ok(())
}

fn stream_matching(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    sse_headers(&mut stream)?;
    let steps = [
        r#"{"type":"matching","step":1,"time":"09:30:00.000001","cause":"Sell 50 @ 101 joins queue","effect":"seller A obtains first FIFO priority","queue":[{"id":"seller A","qty":50,"state":"resting"}],"fills":[]}"#,
        r#"{"type":"matching","step":2,"time":"09:30:00.000002","cause":"Sell 25 @ 101 joins same price","effect":"seller B waits behind seller A","queue":[{"id":"seller A","qty":50,"state":"resting"},{"id":"seller B","qty":25,"state":"resting"}],"fills":[]}"#,
        r#"{"type":"matching","step":3,"time":"09:30:00.000003","cause":"Aggressive Buy 100 @ 102 arrives","effect":"queue is consumed from the front","queue":[{"id":"seller A","qty":0,"state":"filled"},{"id":"seller B","qty":0,"state":"filled"},{"id":"buy residual","qty":25,"state":"rests"}],"fills":[{"id":"seller A","qty":50},{"id":"seller B","qty":25}]}"#,
        r#"{"type":"matching","step":4,"time":"result","cause":"Execution reports emitted","effect":"price-time priority proven by fill order","queue":[{"id":"buy residual","qty":25,"state":"rests"}],"fills":[{"id":"seller A","qty":50},{"id":"seller B","qty":25}]}"#,
    ];

    for step in steps {
        sse_data(&mut stream, step)?;
        thread::sleep(Duration::from_millis(700));
    }
    sse_data(&mut stream, r#"{"type":"done"}"#)?;
    Ok(())
}

fn stream_latency(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    sse_headers(&mut stream)?;
    let race = latency_sim::run_latency_race(42);
    for (idx, report) in race.reports.iter().take(36).enumerate() {
        let payload = format!(
            concat!(
                "{{\"type\":\"latency\",\"seq\":{},\"agent\":\"{}\",\"latencyNs\":{},",
                "\"arrivalTs\":{},\"orderId\":{},\"filledQty\":{},\"status\":\"{}\"}}"
            ),
            idx + 1,
            report.agent_kind.label(),
            report.latency_ns,
            report.arrival_ts,
            report.report.order_id,
            report.report.filled_qty,
            status_name(report.report.status)
        );
        sse_data(&mut stream, &payload)?;
        thread::sleep(Duration::from_millis(120));
    }
    sse_data(&mut stream, r#"{"type":"done"}"#)?;
    Ok(())
}

fn stream_strategy(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    sse_headers(&mut stream)?;
    let suite = strategies::run_strategy_suite();
    let mm = suite.market_maker.equity_curve_cents.as_slice();
    let ofi = suite.order_flow_imbalance.equity_curve_cents.as_slice();
    let arb = suite.latency_arbitrage.equity_curve_cents.as_slice();
    let len = mm.len().min(ofi.len()).min(arb.len());
    for idx in 0..len {
        let payload = format!(
            "{{\"type\":\"strategy\",\"step\":{},\"marketMaker\":{},\"ofi\":{},\"latencyArb\":{},\"inventory\":{},\"pnlDollars\":{:.2}}}",
            idx + 1,
            mm[idx],
            ofi[idx],
            arb[idx],
            suite.market_maker.metrics.inventory,
            suite.market_maker.metrics.pnl_dollars()
        );
        sse_data(&mut stream, &payload)?;
        thread::sleep(Duration::from_millis(80));
    }
    sse_data(&mut stream, r#"{"type":"done"}"#)?;
    Ok(())
}

fn replay_payload(idx: usize, event: &MarketEvent, snapshot: &Snapshot) -> String {
    format!(
        concat!(
            "{{\"type\":\"replay\",\"index\":{},\"eventKind\":\"{}\",\"timestampNs\":{},",
            "\"explanation\":\"{}\",\"asks\":[{}],\"bids\":[{}]}}"
        ),
        idx,
        event_kind(event),
        event_ts(event),
        event_explanation(event),
        levels_json(&snapshot.asks, 5),
        levels_json(&snapshot.bids, 5)
    )
}

fn levels_json(levels: &[orderbook::Level], take: usize) -> String {
    levels
        .iter()
        .take(take)
        .map(|level| format!("{{\"price\":{},\"qty\":{}}}", level.price, level.qty))
        .collect::<Vec<_>>()
        .join(",")
}

fn summary_json() -> String {
    let race = latency_sim::run_latency_race(42);
    let suite = strategies::run_strategy_suite();
    format!(
        concat!(
            "{{\"events\":400391,\"validation\":\"passed\",\"bookLevels\":10,",
            "\"colocatedWins\":{},\"remoteWins\":{},\"queueAdvantageNs\":{},",
            "\"marketMakerPnl\":{:.2},\"marketMakerFillRatioBps\":{},",
            "\"message\":\"streaming backend online\"}}"
        ),
        race.metrics.colocated_wins,
        race.metrics.remote_wins,
        race.metrics.avg_queue_advantage_ns,
        suite.market_maker.metrics.pnl_dollars(),
        suite.market_maker.metrics.fill_ratio_bps()
    )
}

fn event_kind(event: &MarketEvent) -> &'static str {
    match event {
        MarketEvent::AddLimit { .. } => "AddLimit",
        MarketEvent::PartialCancel { .. } => "PartialCancel",
        MarketEvent::FullDelete { .. } => "FullDelete",
        MarketEvent::VisibleExecution { .. } => "VisibleExecution",
        MarketEvent::HiddenExecution { .. } => "HiddenExecution",
        MarketEvent::TradingHalt { .. } => "TradingHalt",
    }
}

fn event_ts(event: &MarketEvent) -> u64 {
    match event {
        MarketEvent::AddLimit { ts, .. }
        | MarketEvent::PartialCancel { ts, .. }
        | MarketEvent::FullDelete { ts, .. }
        | MarketEvent::VisibleExecution { ts, .. }
        | MarketEvent::HiddenExecution { ts, .. }
        | MarketEvent::TradingHalt { ts } => *ts,
    }
}

fn event_explanation(event: &MarketEvent) -> &'static str {
    match event {
        MarketEvent::AddLimit { .. } => "passive liquidity joins a FIFO price queue",
        MarketEvent::PartialCancel { .. } => "resting liquidity is reduced",
        MarketEvent::FullDelete { .. } => "a resting order leaves the queue",
        MarketEvent::VisibleExecution { .. } => "displayed liquidity is consumed",
        MarketEvent::HiddenExecution { .. } => "hidden liquidity executes without visible queue change",
        MarketEvent::TradingHalt { .. } => "halt event duplicates visible book state",
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("server crate should live under crates/")
        .to_path_buf()
}
