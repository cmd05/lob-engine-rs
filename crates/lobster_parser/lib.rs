use market_events::{MarketEvent, Side, Timestamp};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "message parse error on line {}: {}", self.line, self.message)
    }
}

impl Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

pub fn parse_message_file<P: AsRef<Path>>(path: P) -> ParseResult<Vec<MarketEvent>> {
    let file = File::open(path.as_ref())
        .map_err(|err| ParseError::new(0, format!("failed to open file: {err}")))?;
    let capacity = file
        .metadata()
        .ok()
        .map(|metadata| (metadata.len() / 42).max(1) as usize)
        .unwrap_or(0);
    let reader = BufReader::new(file);
    let mut events = Vec::with_capacity(capacity);

    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.map_err(|err| ParseError::new(line_no, err.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        events.push(parse_message_row_with_line(&line, line_no)?);
    }

    Ok(events)
}

pub fn parse_message_row(row: &str) -> ParseResult<MarketEvent> {
    parse_message_row_with_line(row, 1)
}

fn parse_message_row_with_line(row: &str, line: usize) -> ParseResult<MarketEvent> {
    let mut fields = row.split(',');
    let ts = parse_timestamp(next_field(&mut fields, line, "time")?, line)?;
    let event_type = parse_i32(next_field(&mut fields, line, "type")?, line, "type")?;
    let order_id = parse_u64(next_field(&mut fields, line, "order id")?, line, "order id")?;
    let qty = parse_u32(next_field(&mut fields, line, "size")?, line, "size")?;
    let price = parse_i64(next_field(&mut fields, line, "price")?, line, "price")?;
    let direction = parse_i32(next_field(&mut fields, line, "direction")?, line, "direction")?;

    if fields.next().is_some() {
        return Err(ParseError::new(line, "expected exactly 6 columns"));
    }

    match event_type {
        1 => Ok(MarketEvent::AddLimit {
            ts,
            order_id,
            side: parse_side(direction, line)?,
            price,
            qty,
        }),
        2 => Ok(MarketEvent::PartialCancel {
            ts,
            order_id,
            side: parse_side(direction, line)?,
            price,
            qty,
        }),
        3 => Ok(MarketEvent::FullDelete {
            ts,
            order_id,
            side: parse_side(direction, line)?,
            price,
            qty,
        }),
        4 => Ok(MarketEvent::VisibleExecution {
            ts,
            order_id,
            qty,
            price,
            side: parse_side(direction, line)?,
        }),
        5 => Ok(MarketEvent::HiddenExecution {
            ts,
            order_id,
            qty,
            price,
            side: parse_side(direction, line)?,
        }),
        7 => Ok(MarketEvent::TradingHalt { ts }),
        other => Err(ParseError::new(line, format!("unknown event type {other}"))),
    }
}

fn next_field<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    line: usize,
    name: &str,
) -> ParseResult<&'a str> {
    fields
        .next()
        .map(str::trim)
        .ok_or_else(|| ParseError::new(line, format!("missing {name} column")))
}

fn parse_side(direction: i32, line: usize) -> ParseResult<Side> {
    Side::from_lobster_direction(direction)
        .ok_or_else(|| ParseError::new(line, format!("invalid side direction {direction}")))
}

fn parse_timestamp(raw: &str, line: usize) -> ParseResult<Timestamp> {
    let (seconds, fraction) = raw
        .split_once('.')
        .map_or((raw, ""), |(seconds, fraction)| (seconds, fraction));
    let seconds = parse_u64(seconds, line, "time seconds")?;
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::new(line, format!("invalid timestamp {raw}")));
    }

    let mut nanos = 0_u64;
    let mut scale = 100_000_000_u64;
    for digit in fraction.bytes().take(9) {
        nanos += u64::from(digit - b'0') * scale;
        scale /= 10;
    }

    seconds
        .checked_mul(1_000_000_000)
        .and_then(|base| base.checked_add(nanos))
        .ok_or_else(|| ParseError::new(line, "timestamp overflow"))
}

fn parse_i32(raw: &str, line: usize, name: &str) -> ParseResult<i32> {
    raw.parse::<i32>()
        .map_err(|_| ParseError::new(line, format!("invalid {name}: {raw}")))
}

fn parse_i64(raw: &str, line: usize, name: &str) -> ParseResult<i64> {
    raw.parse::<i64>()
        .map_err(|_| ParseError::new(line, format!("invalid {name}: {raw}")))
}

fn parse_u64(raw: &str, line: usize, name: &str) -> ParseResult<u64> {
    raw.parse::<u64>()
        .map_err(|_| ParseError::new(line, format!("invalid {name}: {raw}")))
}

fn parse_u32(raw: &str, line: usize, name: &str) -> ParseResult<u32> {
    raw.parse::<u32>()
        .map_err(|_| ParseError::new(line, format!("invalid {name}: {raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_limit_row() {
        let event = parse_message_row("34200.004241176,1,16113575,18,5853300,1").unwrap();

        assert_eq!(
            event,
            MarketEvent::AddLimit {
                ts: 34_200_004_241_176,
                order_id: 16_113_575,
                side: Side::Buy,
                price: 5_853_300,
                qty: 18
            }
        );
    }

    #[test]
    fn parses_visible_execution_row() {
        let event = parse_message_row("34200.189608068,4,16113798,9,5853000,-1").unwrap();

        assert_eq!(
            event,
            MarketEvent::VisibleExecution {
                ts: 34_200_189_608_068,
                order_id: 16_113_798,
                qty: 9,
                price: 5_853_000,
                side: Side::Sell
            }
        );
    }

    #[test]
    fn rejects_malformed_row() {
        let err = parse_message_row("34200.0,1,1").unwrap_err();
        assert_eq!(err.line(), 1);
    }
}
