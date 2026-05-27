use market_events::{OrderId, Timestamp};
use matching_engine::{ExecutionReport, MatchingEngine, OrderRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    ColocatedMarketMaker,
    RemoteTrader,
    LatencyArbitrageTrader,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ColocatedMarketMaker => "colocated_market_maker",
            Self::RemoteTrader => "remote_trader",
            Self::LatencyArbitrageTrader => "latency_arbitrage_trader",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyModel {
    pub mean_ns: u64,
    pub jitter_ns: u64,
    pub loss_ppm: u32,
}

impl LatencyModel {
    pub const fn new(mean_ns: u64, jitter_ns: u64, loss_ppm: u32) -> Self {
        Self {
            mean_ns,
            jitter_ns,
            loss_ppm,
        }
    }

    pub fn sample_ns(self, rng: &mut DeterministicRng) -> Option<u64> {
        if self.loss_ppm > 0 && rng.next_bounded(1_000_000) < u64::from(self.loss_ppm) {
            return None;
        }

        let jitter_window = self.jitter_ns.saturating_mul(2).saturating_add(1);
        let offset = rng.next_bounded(jitter_window) as i128 - self.jitter_ns as i128;
        let latency = self.mean_ns as i128 + offset;
        Some(latency.max(0) as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Agent {
    pub id: u32,
    pub kind: AgentKind,
    pub latency: LatencyModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundOrder {
    pub agent_id: u32,
    pub send_ts: Timestamp,
    pub request: OrderRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledOrder {
    pub agent_id: u32,
    pub agent_kind: AgentKind,
    pub send_ts: Timestamp,
    pub arrival_ts: Timestamp,
    pub latency_ns: u64,
    pub sequence: u64,
    pub request: OrderRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedOrder {
    pub agent_id: u32,
    pub agent_kind: AgentKind,
    pub send_ts: Timestamp,
    pub order_id: OrderId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalReport {
    pub agent_id: u32,
    pub agent_kind: AgentKind,
    pub send_ts: Timestamp,
    pub arrival_ts: Timestamp,
    pub latency_ns: u64,
    pub report: ExecutionReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyRun {
    pub arrivals: Vec<ScheduledOrder>,
    pub dropped: Vec<DroppedOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceMetrics {
    pub orders_sent: usize,
    pub orders_arrived: usize,
    pub orders_dropped: usize,
    pub colocated_wins: usize,
    pub remote_wins: usize,
    pub median_colocated_latency_ns: u64,
    pub median_remote_latency_ns: u64,
    pub avg_queue_advantage_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyRaceResult {
    pub reports: Vec<ArrivalReport>,
    pub dropped: Vec<DroppedOrder>,
    pub metrics: RaceMetrics,
}

#[derive(Debug, Clone)]
pub struct LatencySimulator {
    agents: Vec<Agent>,
    rng: DeterministicRng,
    next_sequence: u64,
}

impl LatencySimulator {
    pub fn new(seed: u64) -> Self {
        Self {
            agents: Vec::new(),
            rng: DeterministicRng::new(seed),
            next_sequence: 0,
        }
    }

    pub fn add_agent(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    pub fn schedule(&mut self, orders: &[OutboundOrder]) -> LatencyRun {
        let mut arrivals = Vec::with_capacity(orders.len());
        let mut dropped = Vec::new();

        for outbound in orders {
            let agent = self
                .agents
                .iter()
                .find(|agent| agent.id == outbound.agent_id)
                .expect("outbound order references an unknown agent");
            match agent.latency.sample_ns(&mut self.rng) {
                Some(latency_ns) => {
                    arrivals.push(ScheduledOrder {
                        agent_id: agent.id,
                        agent_kind: agent.kind,
                        send_ts: outbound.send_ts,
                        arrival_ts: outbound.send_ts.saturating_add(latency_ns),
                        latency_ns,
                        sequence: self.next_sequence,
                        request: outbound.request.clone(),
                    });
                    self.next_sequence += 1;
                }
                None => dropped.push(DroppedOrder {
                    agent_id: agent.id,
                    agent_kind: agent.kind,
                    send_ts: outbound.send_ts,
                    order_id: request_order_id(&outbound.request),
                }),
            }
        }

        arrivals.sort_by_key(|order| (order.arrival_ts, order.sequence));
        LatencyRun { arrivals, dropped }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    pub fn next_bounded(&mut self, upper_exclusive: u64) -> u64 {
        if upper_exclusive == 0 {
            return 0;
        }
        self.next_u64() % upper_exclusive
    }
}

pub fn run_latency_race(seed: u64) -> LatencyRaceResult {
    use market_events::Side;
    use matching_engine::{OrderKind, TimeInForce};

    let mut matching = MatchingEngine::new();
    matching.submit(
        0,
        10_000,
        Side::Sell,
        100,
        OrderKind::Limit { price: 101_00 },
        TimeInForce::Gtc,
    );

    let colocated = Agent {
        id: 1,
        kind: AgentKind::ColocatedMarketMaker,
        latency: LatencyModel::new(200_000, 100_000, 0),
    };
    let remote = Agent {
        id: 2,
        kind: AgentKind::RemoteTrader,
        latency: LatencyModel::new(25_000_000, 15_000_000, 0),
    };
    let latency_arb = Agent {
        id: 3,
        kind: AgentKind::LatencyArbitrageTrader,
        latency: LatencyModel::new(450_000, 250_000, 2_500),
    };

    let mut sim = LatencySimulator::new(seed);
    sim.add_agent(colocated);
    sim.add_agent(remote);
    sim.add_agent(latency_arb);

    let mut orders = Vec::new();
    for i in 0..20 {
        let send_ts = 1_000_000_000 + i * 1_000_000;
        orders.push(OutboundOrder {
            agent_id: 1,
            send_ts,
            request: OrderRequest::Submit {
                ts: send_ts,
                order_id: 20_000 + i,
                side: Side::Buy,
                qty: 5,
                kind: OrderKind::Limit { price: 101_00 },
                tif: TimeInForce::Ioc,
            },
        });
        orders.push(OutboundOrder {
            agent_id: 2,
            send_ts,
            request: OrderRequest::Submit {
                ts: send_ts,
                order_id: 30_000 + i,
                side: Side::Buy,
                qty: 5,
                kind: OrderKind::Limit { price: 101_00 },
                tif: TimeInForce::Ioc,
            },
        });
        orders.push(OutboundOrder {
            agent_id: 3,
            send_ts,
            request: OrderRequest::Submit {
                ts: send_ts,
                order_id: 40_000 + i,
                side: Side::Buy,
                qty: 5,
                kind: OrderKind::Limit { price: 101_00 },
                tif: TimeInForce::Ioc,
            },
        });
    }

    let scheduled = sim.schedule(&orders);
    let mut reports = Vec::with_capacity(scheduled.arrivals.len());
    for arrival in scheduled.arrivals {
        let mut request = arrival.request.clone();
        set_request_ts(&mut request, arrival.arrival_ts);
        let report = matching.process(request);
        reports.push(ArrivalReport {
            agent_id: arrival.agent_id,
            agent_kind: arrival.agent_kind,
            send_ts: arrival.send_ts,
            arrival_ts: arrival.arrival_ts,
            latency_ns: arrival.latency_ns,
            report,
        });
    }

    let metrics = race_metrics(orders.len(), &reports, &scheduled.dropped);
    LatencyRaceResult {
        reports,
        dropped: scheduled.dropped,
        metrics,
    }
}

fn race_metrics(
    orders_sent: usize,
    reports: &[ArrivalReport],
    dropped: &[DroppedOrder],
) -> RaceMetrics {
    let mut colocated_latencies = reports
        .iter()
        .filter(|report| report.agent_kind == AgentKind::ColocatedMarketMaker)
        .map(|report| report.latency_ns)
        .collect::<Vec<_>>();
    let mut remote_latencies = reports
        .iter()
        .filter(|report| report.agent_kind == AgentKind::RemoteTrader)
        .map(|report| report.latency_ns)
        .collect::<Vec<_>>();
    colocated_latencies.sort_unstable();
    remote_latencies.sort_unstable();

    let colocated_wins = reports
        .iter()
        .filter(|report| {
            report.agent_kind == AgentKind::ColocatedMarketMaker && report.report.filled_qty > 0
        })
        .count();
    let remote_wins = reports
        .iter()
        .filter(|report| report.agent_kind == AgentKind::RemoteTrader && report.report.filled_qty > 0)
        .count();

    let median_colocated_latency_ns = median(&colocated_latencies);
    let median_remote_latency_ns = median(&remote_latencies);

    RaceMetrics {
        orders_sent,
        orders_arrived: reports.len(),
        orders_dropped: dropped.len(),
        colocated_wins,
        remote_wins,
        median_colocated_latency_ns,
        median_remote_latency_ns,
        avg_queue_advantage_ns: median_remote_latency_ns.saturating_sub(median_colocated_latency_ns),
    }
}

fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values[values.len() / 2]
}

fn request_order_id(request: &OrderRequest) -> OrderId {
    match *request {
        OrderRequest::Submit { order_id, .. }
        | OrderRequest::Cancel { order_id, .. }
        | OrderRequest::Replace { order_id, .. } => order_id,
    }
}

fn set_request_ts(request: &mut OrderRequest, ts: Timestamp) {
    match request {
        OrderRequest::Submit { ts: request_ts, .. }
        | OrderRequest::Cancel { ts: request_ts, .. }
        | OrderRequest::Replace { ts: request_ts, .. } => *request_ts = ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_events::Side;
    use matching_engine::{OrderKind, TimeInForce};

    #[test]
    fn schedule_is_deterministic_for_same_seed() {
        let left = sample_schedule(7);
        let right = sample_schedule(7);

        assert_eq!(left, right);
    }

    #[test]
    fn colocated_arrives_before_remote_for_same_send_time() {
        let run = sample_schedule(11);

        assert_eq!(run.arrivals[0].agent_kind, AgentKind::ColocatedMarketMaker);
        assert_eq!(run.arrivals[1].agent_kind, AgentKind::RemoteTrader);
    }

    #[test]
    fn latency_race_produces_queue_advantage_metrics() {
        let result = run_latency_race(42);

        assert_eq!(result.metrics.orders_sent, 60);
        assert!(result.metrics.avg_queue_advantage_ns > 1_000_000);
        assert!(result.metrics.colocated_wins > result.metrics.remote_wins);
    }

    fn sample_schedule(seed: u64) -> LatencyRun {
        let mut sim = LatencySimulator::new(seed);
        sim.add_agent(Agent {
            id: 1,
            kind: AgentKind::ColocatedMarketMaker,
            latency: LatencyModel::new(200_000, 0, 0),
        });
        sim.add_agent(Agent {
            id: 2,
            kind: AgentKind::RemoteTrader,
            latency: LatencyModel::new(25_000_000, 0, 0),
        });

        sim.schedule(&[
            OutboundOrder {
                agent_id: 2,
                send_ts: 1_000,
                request: OrderRequest::Submit {
                    ts: 1_000,
                    order_id: 2,
                    side: Side::Buy,
                    qty: 1,
                    kind: OrderKind::Limit { price: 100 },
                    tif: TimeInForce::Ioc,
                },
            },
            OutboundOrder {
                agent_id: 1,
                send_ts: 1_000,
                request: OrderRequest::Submit {
                    ts: 1_000,
                    order_id: 1,
                    side: Side::Buy,
                    qty: 1,
                    kind: OrderKind::Limit { price: 100 },
                    tif: TimeInForce::Ioc,
                },
            },
        ])
    }
}
