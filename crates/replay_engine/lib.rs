use market_events::MarketEvent;
use orderbook::{BookError, OrderBook, Snapshot};

#[derive(Debug, Clone)]
pub struct ReplayEngine {
    book: OrderBook,
}

impl ReplayEngine {
    pub fn new() -> Self {
        Self {
            book: OrderBook::new(),
        }
    }

    pub fn with_book(book: OrderBook) -> Self {
        Self { book }
    }

    pub fn replay(&mut self, events: &[MarketEvent]) -> Result<(), BookError> {
        for event in events {
            self.book.apply(event)?;
        }
        Ok(())
    }

    pub fn replay_lobster_deltas(&mut self, events: &[MarketEvent]) -> Result<(), BookError> {
        for event in events {
            self.book.apply_lobster_delta(event)?;
        }
        Ok(())
    }

    pub fn apply_lobster_delta(&mut self, event: &MarketEvent) -> Result<(), BookError> {
        self.book.apply_lobster_delta(event)
    }

    pub fn snapshot(&self, levels: usize) -> Snapshot {
        self.book.snapshot(levels)
    }

    pub fn into_book(self) -> OrderBook {
        self.book
    }
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn deterministic_replay_hash(events: &[MarketEvent], levels: usize) -> Result<u64, BookError> {
    let mut engine = ReplayEngine::new();
    engine.replay(events)?;
    Ok(engine.snapshot(levels).stable_hash())
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_events::{MarketEvent, Side};

    #[test]
    fn same_replay_twice_has_identical_hash() {
        let events = vec![
            MarketEvent::AddLimit {
                ts: 1,
                order_id: 1,
                side: Side::Buy,
                price: 100,
                qty: 100,
            },
            MarketEvent::AddLimit {
                ts: 2,
                order_id: 2,
                side: Side::Sell,
                price: 101,
                qty: 50,
            },
            MarketEvent::PartialCancel {
                ts: 3,
                order_id: 1,
                side: Side::Buy,
                price: 100,
                qty: 20,
            },
        ];

        let left = deterministic_replay_hash(&events, 10).unwrap();
        let right = deterministic_replay_hash(&events, 10).unwrap();

        assert_eq!(left, right);
    }
}
