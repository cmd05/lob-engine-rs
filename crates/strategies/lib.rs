use analytics::{FixedSeries, StrategyMetrics, StrategyReport};

pub const CURVE_POINTS: usize = 64;

pub type Report = StrategyReport<CURVE_POINTS>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategySuite {
    pub market_maker: Report,
    pub order_flow_imbalance: Report,
    pub latency_arbitrage: Report,
}

pub fn run_strategy_suite() -> StrategySuite {
    StrategySuite {
        market_maker: inventory_aware_market_maker(),
        order_flow_imbalance: order_flow_imbalance_strategy(),
        latency_arbitrage: latency_arbitrage_strategy(),
    }
}

fn inventory_aware_market_maker() -> Report {
    let mut cash_cents = 0_i64;
    let mut inventory = 0_i64;
    let mut max_inventory = 0_i64;
    let mut fills = 0_u32;
    let mut orders = 0_u32;
    let mut adverse_selection_cents = 0_i64;
    let mut curve = FixedSeries::<CURVE_POINTS>::new();

    let mut mid = 10_100_i64;
    for step in 0..CURVE_POINTS {
        let drift = match step % 7 {
            0 | 1 => 4,
            2 => -2,
            3 | 4 => 3,
            _ => -5,
        };
        let previous_mid = mid;
        mid += drift;
        let skew = inventory.clamp(-30, 30);
        let bid = mid - 2 - skew / 10;
        let ask = mid + 2 - skew / 10;
        orders += 2;

        if step % 3 == 0 {
            let qty = 10_i64;
            inventory += qty;
            cash_cents -= bid * qty;
            fills += 1;
            if mid < previous_mid {
                adverse_selection_cents += (previous_mid - mid) * qty;
            }
        }
        if step % 4 == 1 {
            let qty = 10_i64.min(inventory.max(0));
            if qty > 0 {
                inventory -= qty;
                cash_cents += ask * qty;
                fills += 1;
                if mid > previous_mid {
                    adverse_selection_cents += (mid - previous_mid) * qty;
                }
            }
        }

        max_inventory = max_inventory.max(inventory.abs());
        curve.push(cash_cents + inventory * mid);
    }

    let pnl_cents = cash_cents + inventory * mid;
    Report {
        name: "inventory_aware_market_maker",
        metrics: StrategyMetrics {
            pnl_cents,
            slippage_cents: 0,
            inventory,
            max_inventory,
            fills,
            orders,
            adverse_selection_cents,
            queue_ahead_shares: 420,
        },
        equity_curve_cents: curve,
    }
}

fn order_flow_imbalance_strategy() -> Report {
    let mut cash_cents = 0_i64;
    let mut inventory = 0_i64;
    let mut max_inventory = 0_i64;
    let mut fills = 0_u32;
    let mut orders = 0_u32;
    let mut slippage_cents = 0_i64;
    let mut curve = FixedSeries::<CURVE_POINTS>::new();
    let mut mid = 10_100_i64;

    for step in 0..CURVE_POINTS {
        let bid_size = 800 + ((step * 37) % 520) as i64;
        let ask_size = 780 + ((step * 53 + 90) % 540) as i64;
        let imbalance_bps = ((bid_size - ask_size) * 10_000) / (bid_size + ask_size);
        mid += imbalance_bps / 1_500;

        if imbalance_bps > 650 {
            let price = mid + 1;
            inventory += 5;
            cash_cents -= price * 5;
            slippage_cents += 5;
            fills += 1;
            orders += 1;
        } else if imbalance_bps < -650 && inventory > 0 {
            let price = mid - 1;
            inventory -= 5;
            cash_cents += price * 5;
            slippage_cents += 5;
            fills += 1;
            orders += 1;
        } else {
            orders += 1;
        }

        max_inventory = max_inventory.max(inventory.abs());
        curve.push(cash_cents + inventory * mid);
    }

    Report {
        name: "order_flow_imbalance",
        metrics: StrategyMetrics {
            pnl_cents: cash_cents + inventory * mid,
            slippage_cents,
            inventory,
            max_inventory,
            fills,
            orders,
            adverse_selection_cents: slippage_cents / 2,
            queue_ahead_shares: 0,
        },
        equity_curve_cents: curve,
    }
}

fn latency_arbitrage_strategy() -> Report {
    let race = latency_sim::run_latency_race(42);
    let edge_cents = 3_i64;
    let colocated_filled = race
        .reports
        .iter()
        .filter(|report| {
            report.agent_kind == latency_sim::AgentKind::ColocatedMarketMaker
                && report.report.filled_qty > 0
        })
        .map(|report| i64::from(report.report.filled_qty))
        .sum::<i64>();
    let remote_filled = race
        .reports
        .iter()
        .filter(|report| {
            report.agent_kind == latency_sim::AgentKind::RemoteTrader && report.report.filled_qty > 0
        })
        .map(|report| i64::from(report.report.filled_qty))
        .sum::<i64>();
    let mut curve = FixedSeries::<CURVE_POINTS>::new();
    let mut pnl = 0_i64;

    for report in race.reports.iter().take(CURVE_POINTS) {
        if report.report.filled_qty > 0 {
            let qty = i64::from(report.report.filled_qty);
            let agent_edge = match report.agent_kind {
                latency_sim::AgentKind::ColocatedMarketMaker => edge_cents,
                latency_sim::AgentKind::LatencyArbitrageTrader => edge_cents / 2,
                latency_sim::AgentKind::RemoteTrader => -edge_cents,
            };
            pnl += qty * agent_edge;
        }
        curve.push(pnl);
    }

    Report {
        name: "toy_latency_arbitrage",
        metrics: StrategyMetrics {
            pnl_cents: colocated_filled * edge_cents - remote_filled * edge_cents,
            slippage_cents: 0,
            inventory: 0,
            max_inventory: 0,
            fills: race.metrics.colocated_wins as u32,
            orders: race.metrics.orders_sent as u32,
            adverse_selection_cents: remote_filled * edge_cents,
            queue_ahead_shares: race.metrics.avg_queue_advantage_ns.min(u64::from(u32::MAX)) as u32,
        },
        equity_curve_cents: curve,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_produces_all_strategy_reports() {
        let suite = run_strategy_suite();

        assert_eq!(suite.market_maker.name, "inventory_aware_market_maker");
        assert_eq!(suite.order_flow_imbalance.name, "order_flow_imbalance");
        assert_eq!(suite.latency_arbitrage.name, "toy_latency_arbitrage");
        assert!(!suite.market_maker.equity_curve_cents.is_empty());
    }

    #[test]
    fn latency_arbitrage_benefits_from_queue_advantage() {
        let report = latency_arbitrage_strategy();

        assert!(report.metrics.pnl_cents > 0);
        assert!(report.metrics.queue_ahead_shares > 1_000_000);
    }
}
