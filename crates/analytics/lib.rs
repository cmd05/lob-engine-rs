#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyMetrics {
    pub pnl_cents: i64,
    pub slippage_cents: i64,
    pub inventory: i64,
    pub max_inventory: i64,
    pub fills: u32,
    pub orders: u32,
    pub adverse_selection_cents: i64,
    pub queue_ahead_shares: u32,
}

impl StrategyMetrics {
    pub fn fill_ratio_bps(self) -> u32 {
        if self.orders == 0 {
            return 0;
        }
        ((u64::from(self.fills) * 10_000) / u64::from(self.orders)) as u32
    }

    pub fn pnl_dollars(self) -> f64 {
        self.pnl_cents as f64 / 100.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyReport<const N: usize> {
    pub name: &'static str,
    pub metrics: StrategyMetrics,
    pub equity_curve_cents: FixedSeries<N>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedSeries<const N: usize> {
    values: [i64; N],
    len: usize,
}

impl<const N: usize> FixedSeries<N> {
    pub const fn new() -> Self {
        Self {
            values: [0; N],
            len: 0,
        }
    }

    pub fn push(&mut self, value: i64) {
        if N == 0 {
            return;
        }
        if self.len < N {
            self.values[self.len] = value;
            self.len += 1;
        } else {
            self.values.copy_within(1..N, 0);
            self.values[N - 1] = value;
        }
    }

    pub fn as_slice(&self) -> &[i64] {
        &self.values[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for FixedSeries<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedHistogram<const BINS: usize> {
    min: i64,
    width: i64,
    bins: [u32; BINS],
}

impl<const BINS: usize> FixedHistogram<BINS> {
    pub const fn new(min: i64, width: i64) -> Self {
        Self {
            min,
            width,
            bins: [0; BINS],
        }
    }

    pub fn observe(&mut self, value: i64) {
        if BINS == 0 || self.width <= 0 {
            return;
        }
        let raw = ((value - self.min) / self.width).clamp(0, BINS as i64 - 1);
        self.bins[raw as usize] += 1;
    }

    pub fn bins(&self) -> &[u32] {
        &self.bins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_series_keeps_last_n_values_without_heap_growth() {
        let mut series = FixedSeries::<3>::new();
        series.push(1);
        series.push(2);
        series.push(3);
        series.push(4);

        assert_eq!(series.as_slice(), &[2, 3, 4]);
    }

    #[test]
    fn fill_ratio_uses_basis_points() {
        let metrics = StrategyMetrics {
            pnl_cents: 0,
            slippage_cents: 0,
            inventory: 0,
            max_inventory: 0,
            fills: 3,
            orders: 8,
            adverse_selection_cents: 0,
            queue_ahead_shares: 0,
        };

        assert_eq!(metrics.fill_ratio_bps(), 3_750);
    }
}
