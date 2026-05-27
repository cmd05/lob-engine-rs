pub type Timestamp = u64;
pub type OrderId = u64;
pub type Price = i64;
pub type Quantity = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn from_lobster_direction(direction: i32) -> Option<Self> {
        match direction {
            1 => Some(Self::Buy),
            -1 => Some(Self::Sell),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MarketEvent {
    AddLimit {
        ts: Timestamp,
        order_id: OrderId,
        side: Side,
        price: Price,
        qty: Quantity,
    },

    PartialCancel {
        ts: Timestamp,
        order_id: OrderId,
        side: Side,
        price: Price,
        qty: Quantity,
    },

    FullDelete {
        ts: Timestamp,
        order_id: OrderId,
        side: Side,
        price: Price,
        qty: Quantity,
    },

    VisibleExecution {
        ts: Timestamp,
        order_id: OrderId,
        qty: Quantity,
        price: Price,
        side: Side,
    },

    HiddenExecution {
        ts: Timestamp,
        order_id: OrderId,
        qty: Quantity,
        price: Price,
        side: Side,
    },

    TradingHalt {
        ts: Timestamp,
    },
}
