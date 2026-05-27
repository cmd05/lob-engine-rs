use market_events::{OrderId, Price, Quantity, Side, Timestamp};
use orderbook::{Level, Snapshot, EMPTY_ASK_PRICE, EMPTY_BID_PRICE};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderKind {
    Market,
    Limit { price: Price },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderRequest {
    Submit {
        ts: Timestamp,
        order_id: OrderId,
        side: Side,
        qty: Quantity,
        kind: OrderKind,
        tif: TimeInForce,
    },
    Cancel {
        ts: Timestamp,
        order_id: OrderId,
    },
    Replace {
        ts: Timestamp,
        order_id: OrderId,
        new_qty: Quantity,
        new_price: Price,
        tif: TimeInForce,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    Filled,
    PartiallyFilled,
    Rested,
    Canceled,
    Replaced,
    Expired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub resting_order_id: OrderId,
    pub incoming_order_id: OrderId,
    pub price: Price,
    pub qty: Quantity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub ts: Timestamp,
    pub order_id: OrderId,
    pub status: ReportStatus,
    pub filled_qty: Quantity,
    pub remaining_qty: Quantity,
    pub resting_price: Option<Price>,
    pub fills: Vec<Fill>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchingError {
    DuplicateOrder(OrderId),
    UnknownOrder(OrderId),
    InvalidQuantity,
    MarketOrderCannotRest,
    FokNotFillable {
        requested: Quantity,
        available: Quantity,
    },
}

impl Display for MatchingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateOrder(order_id) => write!(f, "duplicate order id {order_id}"),
            Self::UnknownOrder(order_id) => write!(f, "unknown order id {order_id}"),
            Self::InvalidQuantity => write!(f, "quantity must be positive"),
            Self::MarketOrderCannotRest => write!(f, "market orders cannot rest"),
            Self::FokNotFillable {
                requested,
                available,
            } => write!(
                f,
                "FOK order not fillable: requested {requested}, available {available}"
            ),
        }
    }
}

impl Error for MatchingError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestingOrder {
    id: OrderId,
    side: Side,
    price: Price,
    qty: Quantity,
    sequence: u64,
}

#[derive(Debug, Clone)]
pub struct MatchingEngine {
    bids: BTreeMap<Price, VecDeque<RestingOrder>>,
    asks: BTreeMap<Price, VecDeque<RestingOrder>>,
    orders: HashMap<OrderId, (Side, Price)>,
    next_sequence: u64,
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: HashMap::new(),
            next_sequence: 0,
        }
    }

    pub fn process(&mut self, request: OrderRequest) -> ExecutionReport {
        match request {
            OrderRequest::Submit {
                ts,
                order_id,
                side,
                qty,
                kind,
                tif,
            } => self.submit(ts, order_id, side, qty, kind, tif),
            OrderRequest::Cancel { ts, order_id } => self.cancel(ts, order_id),
            OrderRequest::Replace {
                ts,
                order_id,
                new_qty,
                new_price,
                tif,
            } => self.replace(ts, order_id, new_qty, new_price, tif),
        }
    }

    pub fn submit(
        &mut self,
        ts: Timestamp,
        order_id: OrderId,
        side: Side,
        qty: Quantity,
        kind: OrderKind,
        tif: TimeInForce,
    ) -> ExecutionReport {
        if qty == 0 {
            return rejected(ts, order_id, MatchingError::InvalidQuantity);
        }
        if self.orders.contains_key(&order_id) {
            return rejected(ts, order_id, MatchingError::DuplicateOrder(order_id));
        }
        if kind == OrderKind::Market && tif == TimeInForce::Gtc {
            return rejected(ts, order_id, MatchingError::MarketOrderCannotRest);
        }
        if tif == TimeInForce::Fok {
            let available = self.available_to_match(side, kind);
            if available < qty {
                return rejected(
                    ts,
                    order_id,
                    MatchingError::FokNotFillable {
                        requested: qty,
                        available,
                    },
                );
            }
        }

        let mut remaining = qty;
        let fills = self.match_incoming(order_id, side, kind, &mut remaining);
        let filled_qty = qty - remaining;

        if remaining == 0 {
            return ExecutionReport {
                ts,
                order_id,
                status: ReportStatus::Filled,
                filled_qty,
                remaining_qty: 0,
                resting_price: None,
                fills,
                reason: None,
            };
        }

        match (kind, tif) {
            (OrderKind::Limit { price }, TimeInForce::Gtc) => {
                self.rest(order_id, side, price, remaining);
                ExecutionReport {
                    ts,
                    order_id,
                    status: if filled_qty > 0 {
                        ReportStatus::PartiallyFilled
                    } else {
                        ReportStatus::Rested
                    },
                    filled_qty,
                    remaining_qty: remaining,
                    resting_price: Some(price),
                    fills,
                    reason: None,
                }
            }
            _ => ExecutionReport {
                ts,
                order_id,
                status: if filled_qty > 0 {
                    ReportStatus::PartiallyFilled
                } else {
                    ReportStatus::Expired
                },
                filled_qty,
                remaining_qty: remaining,
                resting_price: None,
                fills,
                reason: Some("unfilled quantity expired by time-in-force".to_string()),
            },
        }
    }

    pub fn cancel(&mut self, ts: Timestamp, order_id: OrderId) -> ExecutionReport {
        match self.remove_order(order_id) {
            Some(order) => ExecutionReport {
                ts,
                order_id,
                status: ReportStatus::Canceled,
                filled_qty: 0,
                remaining_qty: order.qty,
                resting_price: Some(order.price),
                fills: Vec::new(),
                reason: None,
            },
            None => rejected(ts, order_id, MatchingError::UnknownOrder(order_id)),
        }
    }

    pub fn replace(
        &mut self,
        ts: Timestamp,
        order_id: OrderId,
        new_qty: Quantity,
        new_price: Price,
        tif: TimeInForce,
    ) -> ExecutionReport {
        if new_qty == 0 {
            return rejected(ts, order_id, MatchingError::InvalidQuantity);
        }
        let Some(old) = self.remove_order(order_id) else {
            return rejected(ts, order_id, MatchingError::UnknownOrder(order_id));
        };

        let mut report = self.submit(
            ts,
            order_id,
            old.side,
            new_qty,
            OrderKind::Limit { price: new_price },
            tif,
        );
        if report.status == ReportStatus::Rested {
            report.status = ReportStatus::Replaced;
        }
        report
    }

    pub fn snapshot(&self, levels: usize) -> Snapshot {
        let asks = self
            .asks
            .iter()
            .filter_map(|(&price, orders)| aggregate_level(price, orders))
            .take(levels)
            .collect::<Vec<_>>();
        let bids = self
            .bids
            .iter()
            .rev()
            .filter_map(|(&price, orders)| aggregate_level(price, orders))
            .take(levels)
            .collect::<Vec<_>>();

        pad_snapshot(Snapshot { asks, bids }, levels)
    }

    pub fn resting_order_count(&self) -> usize {
        self.orders.len()
    }

    fn match_incoming(
        &mut self,
        incoming_order_id: OrderId,
        side: Side,
        kind: OrderKind,
        remaining: &mut Quantity,
    ) -> Vec<Fill> {
        let mut fills = Vec::new();
        while *remaining > 0 {
            let Some(price) = self.best_match_price(side, kind) else {
                break;
            };
            let mut removed_ids = Vec::new();
            {
                let queue = self
                    .opposite_levels_mut(side)
                    .get_mut(&price)
                    .expect("best price came from this side");
                while *remaining > 0 {
                    let Some(resting) = queue.front_mut() else {
                        break;
                    };
                    let fill_qty = (*remaining).min(resting.qty);
                    resting.qty -= fill_qty;
                    *remaining -= fill_qty;
                    fills.push(Fill {
                        resting_order_id: resting.id,
                        incoming_order_id,
                        price,
                        qty: fill_qty,
                    });
                    if resting.qty == 0 {
                        let removed = queue.pop_front().expect("front existed");
                        removed_ids.push(removed.id);
                    }
                }
                if queue.is_empty() {
                    self.opposite_levels_mut(side).remove(&price);
                }
            }
            for removed_id in removed_ids {
                self.orders.remove(&removed_id);
            }
        }
        fills
    }

    fn rest(&mut self, order_id: OrderId, side: Side, price: Price, qty: Quantity) {
        let order = RestingOrder {
            id: order_id,
            side,
            price,
            qty,
            sequence: self.next_sequence,
        };
        self.next_sequence += 1;
        self.levels_mut(side).entry(price).or_default().push_back(order);
        self.orders.insert(order_id, (side, price));
    }

    fn remove_order(&mut self, order_id: OrderId) -> Option<RestingOrder> {
        let (side, price) = *self.orders.get(&order_id)?;
        let levels = self.levels_mut(side);
        let queue = levels.get_mut(&price)?;
        let pos = queue.iter().position(|order| order.id == order_id)?;
        let removed = queue.remove(pos)?;
        if queue.is_empty() {
            levels.remove(&price);
        }
        self.orders.remove(&order_id);
        Some(removed)
    }

    fn available_to_match(&self, side: Side, kind: OrderKind) -> Quantity {
        let mut available = 0_u32;
        let levels = self.opposite_levels(side);
        match side {
            Side::Buy => {
                for (&price, queue) in levels.iter() {
                    if !price_matches(side, kind, price) {
                        break;
                    }
                    available = available.saturating_add(queue_qty(queue));
                }
            }
            Side::Sell => {
                for (&price, queue) in levels.iter().rev() {
                    if !price_matches(side, kind, price) {
                        break;
                    }
                    available = available.saturating_add(queue_qty(queue));
                }
            }
        }
        available
    }

    fn best_match_price(&self, side: Side, kind: OrderKind) -> Option<Price> {
        let price = match side {
            Side::Buy => self.asks.keys().next().copied(),
            Side::Sell => self.bids.keys().next_back().copied(),
        }?;
        price_matches(side, kind, price).then_some(price)
    }

    fn levels_mut(&mut self, side: Side) -> &mut BTreeMap<Price, VecDeque<RestingOrder>> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    fn opposite_levels(&self, side: Side) -> &BTreeMap<Price, VecDeque<RestingOrder>> {
        match side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        }
    }

    fn opposite_levels_mut(&mut self, side: Side) -> &mut BTreeMap<Price, VecDeque<RestingOrder>> {
        match side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        }
    }
}

fn price_matches(side: Side, kind: OrderKind, resting_price: Price) -> bool {
    match (side, kind) {
        (_, OrderKind::Market) => true,
        (Side::Buy, OrderKind::Limit { price }) => resting_price <= price,
        (Side::Sell, OrderKind::Limit { price }) => resting_price >= price,
    }
}

fn queue_qty(queue: &VecDeque<RestingOrder>) -> Quantity {
    queue.iter().map(|order| order.qty).sum()
}

fn aggregate_level(price: Price, queue: &VecDeque<RestingOrder>) -> Option<Level> {
    let qty = queue_qty(queue);
    (qty > 0).then_some(Level { price, qty })
}

fn pad_snapshot(mut snapshot: Snapshot, levels: usize) -> Snapshot {
    while snapshot.asks.len() < levels {
        snapshot.asks.push(Level {
            price: EMPTY_ASK_PRICE,
            qty: 0,
        });
    }
    while snapshot.bids.len() < levels {
        snapshot.bids.push(Level {
            price: EMPTY_BID_PRICE,
            qty: 0,
        });
    }
    snapshot
}

fn rejected(ts: Timestamp, order_id: OrderId, err: MatchingError) -> ExecutionReport {
    ExecutionReport {
        ts,
        order_id,
        status: ReportStatus::Rejected,
        filled_qty: 0,
        remaining_qty: 0,
        resting_price: None,
        fills: Vec::new(),
        reason: Some(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_crosses_and_rests_residual_with_fifo_fills() {
        let mut engine = MatchingEngine::new();
        engine.submit(1, 10, Side::Sell, 50, OrderKind::Limit { price: 101 }, TimeInForce::Gtc);
        engine.submit(2, 11, Side::Sell, 25, OrderKind::Limit { price: 101 }, TimeInForce::Gtc);

        let report = engine.submit(
            3,
            20,
            Side::Buy,
            100,
            OrderKind::Limit { price: 102 },
            TimeInForce::Gtc,
        );

        assert_eq!(report.status, ReportStatus::PartiallyFilled);
        assert_eq!(report.filled_qty, 75);
        assert_eq!(report.remaining_qty, 25);
        assert_eq!(report.resting_price, Some(102));
        assert_eq!(report.fills[0].resting_order_id, 10);
        assert_eq!(report.fills[1].resting_order_id, 11);
        assert_eq!(
            engine.snapshot(1).bids,
            vec![Level {
                price: 102,
                qty: 25
            }]
        );
    }

    #[test]
    fn ioc_expires_unfilled_remainder() {
        let mut engine = MatchingEngine::new();
        engine.submit(1, 10, Side::Sell, 40, OrderKind::Limit { price: 101 }, TimeInForce::Gtc);

        let report = engine.submit(2, 20, Side::Buy, 100, OrderKind::Market, TimeInForce::Ioc);

        assert_eq!(report.status, ReportStatus::PartiallyFilled);
        assert_eq!(report.filled_qty, 40);
        assert_eq!(report.remaining_qty, 60);
        assert_eq!(engine.resting_order_count(), 0);
    }

    #[test]
    fn fok_rejects_without_mutating_when_not_fillable() {
        let mut engine = MatchingEngine::new();
        engine.submit(1, 10, Side::Sell, 40, OrderKind::Limit { price: 101 }, TimeInForce::Gtc);

        let report = engine.submit(
            2,
            20,
            Side::Buy,
            100,
            OrderKind::Limit { price: 101 },
            TimeInForce::Fok,
        );

        assert_eq!(report.status, ReportStatus::Rejected);
        assert_eq!(
            engine.snapshot(1).asks,
            vec![Level {
                price: 101,
                qty: 40
            }]
        );
    }

    #[test]
    fn cancel_and_replace_update_resting_book() {
        let mut engine = MatchingEngine::new();
        engine.submit(1, 10, Side::Buy, 100, OrderKind::Limit { price: 100 }, TimeInForce::Gtc);

        let replaced = engine.replace(2, 10, 70, 99, TimeInForce::Gtc);
        assert_eq!(replaced.status, ReportStatus::Replaced);
        assert_eq!(
            engine.snapshot(1).bids,
            vec![Level {
                price: 99,
                qty: 70
            }]
        );

        let canceled = engine.cancel(3, 10);
        assert_eq!(canceled.status, ReportStatus::Canceled);
        assert_eq!(engine.resting_order_count(), 0);
    }
}
