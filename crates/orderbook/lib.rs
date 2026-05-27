use market_events::{MarketEvent, OrderId, Price, Quantity, Side};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const EMPTY_ASK_PRICE: Price = 9_999_999_999;
pub const EMPTY_BID_PRICE: Price = -9_999_999_999;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub price: Price,
    pub qty: Quantity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub asks: Vec<Level>,
    pub bids: Vec<Level>,
}

impl Snapshot {
    pub fn top_n(&self, levels: usize) -> Self {
        let mut asks = self.asks.iter().take(levels).cloned().collect::<Vec<_>>();
        let mut bids = self.bids.iter().take(levels).cloned().collect::<Vec<_>>();

        while asks.len() < levels {
            asks.push(Level {
                price: EMPTY_ASK_PRICE,
                qty: 0,
            });
        }
        while bids.len() < levels {
            bids.push(Level {
                price: EMPTY_BID_PRICE,
                qty: 0,
            });
        }

        Self { asks, bids }
    }

    pub fn stable_hash(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for level in self.asks.iter().chain(self.bids.iter()) {
            hash ^= level.price as u64;
            hash = hash.wrapping_mul(0x1000_0000_01b3);
            hash ^= u64::from(level.qty);
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
        hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    DuplicateOrder(OrderId),
    UnknownOrder(OrderId),
    InvalidQuantity,
    CancelExceedsResting {
        order_id: OrderId,
        resting: Quantity,
        requested: Quantity,
    },
    ExecutionExceedsResting {
        order_id: OrderId,
        resting: Quantity,
        requested: Quantity,
    },
    CrossedBook {
        bid: Price,
        ask: Price,
    },
}

impl Display for BookError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateOrder(order_id) => write!(f, "duplicate order id {order_id}"),
            Self::UnknownOrder(order_id) => write!(f, "unknown order id {order_id}"),
            Self::InvalidQuantity => write!(f, "quantity must be positive"),
            Self::CancelExceedsResting {
                order_id,
                resting,
                requested,
            } => write!(
                f,
                "cancel for order {order_id} exceeds resting qty: requested {requested}, resting {resting}"
            ),
            Self::ExecutionExceedsResting {
                order_id,
                resting,
                requested,
            } => write!(
                f,
                "execution for order {order_id} exceeds resting qty: requested {requested}, resting {resting}"
            ),
            Self::CrossedBook { bid, ask } => write!(f, "crossed book: bid {bid} >= ask {ask}"),
        }
    }
}

impl Error for BookError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestingOrder {
    id: OrderId,
    side: Side,
    price: Price,
    qty: Quantity,
}

#[derive(Debug, Clone)]
pub struct OrderBook {
    bids: BTreeMap<Price, VecDeque<RestingOrder>>,
    asks: BTreeMap<Price, VecDeque<RestingOrder>>,
    orders: HashMap<OrderId, (Side, Price)>,
    next_seed_order_id: OrderId,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: HashMap::new(),
            next_seed_order_id: u64::MAX,
        }
    }

    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut book = Self::new();
        for level in &snapshot.asks {
            if level.qty > 0 && level.price != EMPTY_ASK_PRICE {
                book.add_seed_level(Side::Sell, level.price, level.qty);
            }
        }
        for level in &snapshot.bids {
            if level.qty > 0 && level.price != EMPTY_BID_PRICE {
                book.add_seed_level(Side::Buy, level.price, level.qty);
            }
        }
        book
    }

    pub fn apply(&mut self, event: &MarketEvent) -> Result<(), BookError> {
        match *event {
            MarketEvent::AddLimit {
                order_id,
                side,
                price,
                qty,
                ..
            } => self.add_limit(order_id, side, price, qty),
            MarketEvent::PartialCancel { order_id, qty, .. } => {
                self.partial_cancel(order_id, qty)
            }
            MarketEvent::FullDelete { order_id, .. } => self.full_delete(order_id),
            MarketEvent::VisibleExecution {
                order_id, qty, price, side, ..
            } => self.visible_execution(order_id, side, price, qty),
            MarketEvent::HiddenExecution { .. } | MarketEvent::TradingHalt { .. } => Ok(()),
        }
    }

    pub fn apply_lobster_delta(&mut self, event: &MarketEvent) -> Result<(), BookError> {
        match *event {
            MarketEvent::PartialCancel {
                order_id,
                qty,
                side,
                price,
                ..
            } => {
                if self.orders.contains_key(&order_id) {
                    self.partial_cancel(order_id, qty)
                } else {
                    self.reduce_aggregate(side, price, qty)
                }
            }
            MarketEvent::FullDelete {
                order_id,
                side,
                price,
                qty,
                ..
            } => {
                if self.orders.contains_key(&order_id) {
                    self.full_delete(order_id)
                } else {
                    self.reduce_aggregate(side, price, qty)
                }
            }
            MarketEvent::VisibleExecution {
                order_id,
                qty,
                price,
                side,
                ..
            } => {
                if self.orders.contains_key(&order_id) {
                    self.visible_execution(order_id, side, price, qty)
                } else {
                    self.reduce_aggregate(side, price, qty)
                }
            }
            MarketEvent::HiddenExecution { .. } | MarketEvent::TradingHalt { .. } => Ok(()),
            _ => self.apply(event),
        }
    }

    pub fn add_limit(
        &mut self,
        order_id: OrderId,
        side: Side,
        price: Price,
        qty: Quantity,
    ) -> Result<(), BookError> {
        if qty == 0 {
            return Err(BookError::InvalidQuantity);
        }
        if self.orders.contains_key(&order_id) {
            return Err(BookError::DuplicateOrder(order_id));
        }
        self.reject_if_crossing(side, price)?;

        let order = RestingOrder {
            id: order_id,
            side,
            price,
            qty,
        };
        self.levels_mut(side).entry(price).or_default().push_back(order);
        self.orders.insert(order_id, (side, price));
        Ok(())
    }

    pub fn partial_cancel(&mut self, order_id: OrderId, qty: Quantity) -> Result<(), BookError> {
        if qty == 0 {
            return Err(BookError::InvalidQuantity);
        }
        let resting = self.order_qty(order_id)?;
        if qty > resting {
            return Err(BookError::CancelExceedsResting {
                order_id,
                resting,
                requested: qty,
            });
        }
        self.reduce_order(order_id, qty)
    }

    pub fn full_delete(&mut self, order_id: OrderId) -> Result<(), BookError> {
        let resting = self.order_qty(order_id)?;
        self.reduce_order(order_id, resting)
    }

    pub fn visible_execution(
        &mut self,
        order_id: OrderId,
        side: Side,
        price: Price,
        qty: Quantity,
    ) -> Result<(), BookError> {
        if qty == 0 {
            return Err(BookError::InvalidQuantity);
        }
        let resting = self.order_qty(order_id)?;
        if qty > resting {
            return Err(BookError::ExecutionExceedsResting {
                order_id,
                resting,
                requested: qty,
            });
        }
        let (stored_side, stored_price) = self.orders[&order_id];
        if stored_side != side || stored_price != price {
            return Err(BookError::UnknownOrder(order_id));
        }
        self.reduce_order(order_id, qty)
    }

    pub fn snapshot(&self, levels: usize) -> Snapshot {
        let asks = self
            .asks
            .iter()
            .filter_map(|(&price, orders)| level_from_orders(price, orders))
            .take(levels)
            .collect::<Vec<_>>();
        let bids = self
            .bids
            .iter()
            .rev()
            .filter_map(|(&price, orders)| level_from_orders(price, orders))
            .take(levels)
            .collect::<Vec<_>>();

        Snapshot { asks, bids }.top_n(levels)
    }

    fn add_seed_level(&mut self, side: Side, price: Price, qty: Quantity) {
        let order_id = self.next_seed_order_id;
        self.next_seed_order_id -= 1;
        let order = RestingOrder {
            id: order_id,
            side,
            price,
            qty,
        };
        self.levels_mut(side).entry(price).or_default().push_back(order);
        self.orders.insert(order_id, (side, price));
    }

    fn reduce_aggregate(
        &mut self,
        side: Side,
        price: Price,
        mut qty: Quantity,
    ) -> Result<(), BookError> {
        if qty == 0 {
            return Err(BookError::InvalidQuantity);
        }
        let mut removed_ids = Vec::new();
        {
            let levels = self.levels_mut(side);
            let queue = levels
                .get_mut(&price)
                .ok_or(BookError::ExecutionExceedsResting {
                    order_id: 0,
                    resting: 0,
                    requested: qty,
                })?;
            let resting = queue.iter().map(|order| order.qty).sum::<Quantity>();
            if qty > resting {
                return Err(BookError::ExecutionExceedsResting {
                    order_id: 0,
                    resting,
                    requested: qty,
                });
            }

            while qty > 0 {
                let front = queue.front_mut().expect("queue is non-empty while reducing");
                let removed = qty.min(front.qty);
                front.qty -= removed;
                qty -= removed;
                if front.qty == 0 {
                    let removed = queue.pop_front().expect("queue is non-empty while popping");
                    removed_ids.push(removed.id);
                }
            }
            if queue.is_empty() {
                levels.remove(&price);
            }
        }
        for order_id in removed_ids {
            self.orders.remove(&order_id);
        }
        Ok(())
    }

    fn reduce_order(&mut self, order_id: OrderId, qty: Quantity) -> Result<(), BookError> {
        let (side, price) = *self
            .orders
            .get(&order_id)
            .ok_or(BookError::UnknownOrder(order_id))?;
        let remove_order = {
            let levels = self.levels_mut(side);
            let queue = levels
                .get_mut(&price)
                .ok_or(BookError::UnknownOrder(order_id))?;
            let pos = queue
                .iter()
                .position(|order| order.id == order_id)
                .ok_or(BookError::UnknownOrder(order_id))?;

            let remove_order = {
                let order = queue
                    .get_mut(pos)
                    .expect("position came from this queue and must exist");
                order.qty -= qty;
                order.qty == 0
            };

            if remove_order {
                queue.remove(pos);
            }
            if queue.is_empty() {
                levels.remove(&price);
            }
            remove_order
        };
        if remove_order {
            self.orders.remove(&order_id);
        }
        Ok(())
    }

    fn order_qty(&self, order_id: OrderId) -> Result<Quantity, BookError> {
        let (side, price) = *self
            .orders
            .get(&order_id)
            .ok_or(BookError::UnknownOrder(order_id))?;
        let queue = self
            .levels(side)
            .get(&price)
            .ok_or(BookError::UnknownOrder(order_id))?;
        queue
            .iter()
            .find(|order| order.id == order_id)
            .map(|order| order.qty)
            .ok_or(BookError::UnknownOrder(order_id))
    }

    fn levels(&self, side: Side) -> &BTreeMap<Price, VecDeque<RestingOrder>> {
        match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        }
    }

    fn levels_mut(&mut self, side: Side) -> &mut BTreeMap<Price, VecDeque<RestingOrder>> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    fn reject_if_crossing(&self, side: Side, price: Price) -> Result<(), BookError> {
        match side {
            Side::Buy => {
                if let Some((&best_ask, _)) = self.asks.iter().next() {
                    if price >= best_ask {
                        return Err(BookError::CrossedBook {
                            bid: price,
                            ask: best_ask,
                        });
                    }
                }
            }
            Side::Sell => {
                if let Some((&best_bid, _)) = self.bids.iter().next_back() {
                    if best_bid >= price {
                        return Err(BookError::CrossedBook {
                            bid: best_bid,
                            ask: price,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn level_from_orders(price: Price, orders: &VecDeque<RestingOrder>) -> Option<Level> {
    let qty = orders.iter().map(|order| order.qty).sum::<Quantity>();
    (qty > 0).then_some(Level { price, qty })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_partial_cancel_keep_expected_top_levels() {
        let mut book = OrderBook::new();

        book.add_limit(1, Side::Buy, 100, 100).unwrap();
        book.add_limit(2, Side::Sell, 101, 50).unwrap();
        book.partial_cancel(1, 20).unwrap();

        assert_eq!(
            book.snapshot(1),
            Snapshot {
                asks: vec![Level { price: 101, qty: 50 }],
                bids: vec![Level { price: 100, qty: 80 }],
            }
        );
    }

    #[test]
    fn fifo_aggregation_at_same_price() {
        let mut book = OrderBook::new();

        book.add_limit(1, Side::Buy, 100, 70).unwrap();
        book.add_limit(2, Side::Buy, 100, 30).unwrap();
        book.full_delete(1).unwrap();

        assert_eq!(
            book.snapshot(1).bids,
            vec![Level {
                price: 100,
                qty: 30
            }]
        );
    }

    #[test]
    fn rejects_crossed_add() {
        let mut book = OrderBook::new();

        book.add_limit(1, Side::Sell, 101, 50).unwrap();
        let err = book.add_limit(2, Side::Buy, 102, 10).unwrap_err();

        assert_eq!(err, BookError::CrossedBook { bid: 102, ask: 101 });
        assert_eq!(
            book.snapshot(1).asks,
            vec![Level {
                price: 101,
                qty: 50
            }]
        );
        assert_eq!(
            book.snapshot(1).bids,
            vec![Level {
                price: EMPTY_BID_PRICE,
                qty: 0
            }]
        );
    }
}
