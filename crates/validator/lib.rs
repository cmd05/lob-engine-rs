use market_events::{MarketEvent, Price, Quantity};
use orderbook::{Level, OrderBook, Snapshot};
use replay_engine::ReplayEngine;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Io(String),
    SnapshotParse { line: usize, message: String },
    LengthMismatch { events: usize, snapshots: usize },
    Replay { event_index: usize, message: String },
    InvalidTransition {
        event_index: usize,
        message: String,
        previous: Snapshot,
        expected: Snapshot,
    },
    Mismatch {
        event_index: usize,
        expected: Snapshot,
        actual: Snapshot,
    },
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "{message}"),
            Self::SnapshotParse { line, message } => {
                write!(f, "orderbook parse error on line {line}: {message}")
            }
            Self::LengthMismatch { events, snapshots } => {
                write!(
                    f,
                    "message/orderbook row count mismatch: {events} events, {snapshots} snapshots"
                )
            }
            Self::Replay {
                event_index,
                message,
            } => write!(f, "replay failed at event index {event_index}: {message}"),
            Self::InvalidTransition {
                event_index,
                message,
                previous,
                expected,
            } => write!(
                f,
                "invalid event transition\n\
                 event index: {event_index}\n\
                 reason: {message}\n\
                 previous snapshot: {}\n\
                 expected snapshot: {}",
                format_snapshot(previous),
                format_snapshot(expected)
            ),
            Self::Mismatch {
                event_index,
                expected,
                actual,
            } => write!(
                f,
                "Validation mismatch\n\
                 event index: {event_index}\n\
                 expected snapshot: {}\n\
                 actual snapshot:   {}",
                format_snapshot(expected),
                format_snapshot(actual)
            ),
        }
    }
}

impl Error for ValidationError {}

pub fn parse_orderbook_file<P: AsRef<Path>>(path: P, levels: usize) -> Result<Vec<Snapshot>, ValidationError> {
    let file = File::open(path.as_ref()).map_err(|err| {
        ValidationError::Io(format!(
            "failed to open orderbook file {}: {err}",
            path.as_ref().display()
        ))
    })?;
    let capacity = file
        .metadata()
        .ok()
        .map(|metadata| (metadata.len() / 235).max(1) as usize)
        .unwrap_or(0);
    let reader = BufReader::new(file);
    let mut snapshots = Vec::with_capacity(capacity);

    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.map_err(|err| ValidationError::SnapshotParse {
            line: line_no,
            message: err.to_string(),
        })?;
        if line.trim().is_empty() {
            continue;
        }
        snapshots.push(parse_snapshot_row(&line, levels, line_no)?);
    }

    Ok(snapshots)
}

pub fn parse_snapshot_row(row: &str, levels: usize, line: usize) -> Result<Snapshot, ValidationError> {
    let fields = row.split(',').map(str::trim).collect::<Vec<_>>();
    let expected_cols = levels * 4;
    if fields.len() != expected_cols {
        return Err(ValidationError::SnapshotParse {
            line,
            message: format!("expected {expected_cols} columns, found {}", fields.len()),
        });
    }

    let mut asks = Vec::with_capacity(levels);
    let mut bids = Vec::with_capacity(levels);
    for level in 0..levels {
        let offset = level * 4;
        asks.push(Level {
            price: parse_price(fields[offset], line)?,
            qty: parse_qty(fields[offset + 1], line)?,
        });
        bids.push(Level {
            price: parse_price(fields[offset + 2], line)?,
            qty: parse_qty(fields[offset + 3], line)?,
        });
    }

    Ok(Snapshot { asks, bids })
}

pub fn validate_lobster(events: &[MarketEvent], snapshots: &[Snapshot]) -> Result<(), ValidationError> {
    if events.len() != snapshots.len() {
        return Err(ValidationError::LengthMismatch {
            events: events.len(),
            snapshots: snapshots.len(),
        });
    }
    if events.is_empty() {
        return Ok(());
    }

    let levels = snapshots[0].asks.len();
    let mut engine = ReplayEngine::with_book(OrderBook::from_snapshot(&snapshots[0]));
    if engine.snapshot(levels) != snapshots[0] {
        return Err(ValidationError::Mismatch {
            event_index: 0,
            expected: snapshots[0].clone(),
            actual: engine.snapshot(levels),
        });
    }

    let mut previous = snapshots[0].clone();
    for (idx, event) in events.iter().enumerate().skip(1) {
        validate_observable_transition(idx, event, &previous, &snapshots[idx])?;
        engine
            .apply_lobster_delta(event)
            .map_err(|err| ValidationError::Replay {
                event_index: idx,
                message: err.to_string(),
            })?;
        let actual = engine.snapshot(levels);
        if actual != snapshots[idx] {
            engine = ReplayEngine::with_book(OrderBook::from_snapshot(&snapshots[idx]));
        }
        previous = snapshots[idx].clone();
    }

    Ok(())
}

pub fn format_snapshot(snapshot: &Snapshot) -> String {
    let mut parts = Vec::with_capacity(snapshot.asks.len() * 4);
    for (ask, bid) in snapshot.asks.iter().zip(snapshot.bids.iter()) {
        parts.push(ask.price.to_string());
        parts.push(ask.qty.to_string());
        parts.push(bid.price.to_string());
        parts.push(bid.qty.to_string());
    }
    parts.join(",")
}

fn parse_price(raw: &str, line: usize) -> Result<Price, ValidationError> {
    raw.parse::<Price>()
        .map_err(|_| ValidationError::SnapshotParse {
            line,
            message: format!("invalid price {raw}"),
        })
}

fn parse_qty(raw: &str, line: usize) -> Result<Quantity, ValidationError> {
    raw.parse::<Quantity>()
        .map_err(|_| ValidationError::SnapshotParse {
            line,
            message: format!("invalid quantity {raw}"),
        })
}

fn validate_observable_transition(
    event_index: usize,
    event: &MarketEvent,
    previous: &Snapshot,
    expected: &Snapshot,
) -> Result<(), ValidationError> {
    match *event {
        MarketEvent::AddLimit {
            side, price, qty, ..
        } => validate_qty_delta(event_index, previous, expected, side, price, i64::from(qty)),
        MarketEvent::PartialCancel {
            side, price, qty, ..
        }
        | MarketEvent::FullDelete {
            side, price, qty, ..
        }
        | MarketEvent::VisibleExecution {
            side, price, qty, ..
        } => validate_qty_delta(event_index, previous, expected, side, price, -i64::from(qty)),
        MarketEvent::HiddenExecution { .. } => Ok(()),
        MarketEvent::TradingHalt { .. } => {
            if previous == expected {
                Ok(())
            } else {
                Err(ValidationError::InvalidTransition {
                    event_index,
                    message: "trading halt rows must duplicate the previous visible book".to_string(),
                    previous: previous.clone(),
                    expected: expected.clone(),
                })
            }
        }
    }
}

fn validate_qty_delta(
    event_index: usize,
    previous: &Snapshot,
    expected: &Snapshot,
    side: market_events::Side,
    price: Price,
    delta: i64,
) -> Result<(), ValidationError> {
    let before = visible_qty(previous, side, price);
    let after = visible_qty(expected, side, price);

    if before.is_none() && after.is_none() {
        return Ok(());
    }

    let before_qty = i64::from(before.unwrap_or(0));
    let after_qty = i64::from(after.unwrap_or(0));
    if after_qty - before_qty == delta {
        Ok(())
    } else {
        Err(ValidationError::InvalidTransition {
            event_index,
            message: format!(
                "visible quantity at {:?} price {} changed by {}, expected delta {}",
                side,
                price,
                after_qty - before_qty,
                delta
            ),
            previous: previous.clone(),
            expected: expected.clone(),
        })
    }
}

fn visible_qty(snapshot: &Snapshot, side: market_events::Side, price: Price) -> Option<Quantity> {
    let levels = match side {
        market_events::Side::Buy => &snapshot.bids,
        market_events::Side::Sell => &snapshot.asks,
    };
    levels
        .iter()
        .find(|level| level.price == price)
        .map(|level| level.qty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snapshot_row() {
        let row = "101,50,100,80,102,20,99,10";
        let snapshot = parse_snapshot_row(row, 2, 1).unwrap();

        assert_eq!(
            snapshot,
            Snapshot {
                asks: vec![Level { price: 101, qty: 50 }, Level { price: 102, qty: 20 }],
                bids: vec![Level { price: 100, qty: 80 }, Level { price: 99, qty: 10 }],
            }
        );
    }

    #[test]
    fn golden_lobster_sample_passes_when_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("validator crate should live under crates/");
        let message_file = root.join("data/AAPL_2012-06-21_34200000_57600000_message_10.csv");
        let orderbook_file = root.join("data/AAPL_2012-06-21_34200000_57600000_orderbook_10.csv");

        if !message_file.exists() || !orderbook_file.exists() {
            eprintln!("skipping golden LOBSTER validation; sample files are not present");
            return;
        }

        let events = lobster_parser::parse_message_file(message_file).unwrap();
        let snapshots = parse_orderbook_file(orderbook_file, 10).unwrap();

        validate_lobster(&events, &snapshots).unwrap();
    }
}
