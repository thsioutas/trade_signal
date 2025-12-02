#[derive(Debug, Clone, Copy)]
pub struct AtrFilter {
    period: usize,
    floor: f64, // ATR% as fraction, e.g. 0.003 = 0.3%
}

impl AtrFilter {
    /// Create a filter with a fixed floor (e.g. from config).
    pub fn new_fixed(period: usize, floor: f64) -> Self {
        Self { period, floor }
    }

    /// Example: percentile = 0.4 => 40th percentile.
    pub fn from_history(prices: &[f64], period: usize, percentile: f64) -> Option<Self> {
        if prices.len() < period + 2 {
            return None;
        }

        let mut atr_percents = Vec::new();

        for end in (period + 1)..=prices.len() {
            if let Some(atr_p) = atr_percent(&prices[..end], period) {
                atr_percents.push(atr_p);
            }
        }

        if atr_percents.is_empty() {
            return None;
        }

        atr_percents.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let p = percentile.clamp(0.0, 1.0);
        let idx = ((atr_percents.len() - 1) as f64 * p).round() as usize;

        Some(Self {
            period,
            floor: atr_percents[idx],
        })
    }

    /// ATR% at the end of `prices` using this filter's period.
    pub fn atr_percent(&self, prices: &[f64]) -> Option<f64> {
        atr_percent(prices, self.period)
    }

    pub fn period(&self) -> usize {
        self.period
    }

    pub fn floor(&self) -> f64 {
        self.floor
    }
}

/// Simple ATR approximation using only close prices:
/// TR_i = |close_i - close_{i-1}|
/// ATR  = mean(TR_last_period)
pub fn atr(prices: &[f64], period: usize) -> Option<f64> {
    if prices.len() < period + 1 || period == 0 {
        return None;
    }

    let n = prices.len();
    let mut sum_tr = 0.0;

    // Compute TR for the last `period` intervals
    // i runs over the last `period` closes
    for i in 1..=period {
        let prev = prices[n - 1 - i];
        let curr = prices[n - i];
        let tr = (curr - prev).abs();
        sum_tr += tr;
    }

    Some(sum_tr / period as f64)
}

/// ATR as a fraction of price (e.g. 0.02 = 2%).
pub fn atr_percent(prices: &[f64], period: usize) -> Option<f64> {
    let atr_val = atr(prices, period)?;
    let last_price = *prices.last()?;
    if last_price <= 0.0 {
        return None;
    }
    Some(atr_val / last_price)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_atr_returns_none_when_not_enough_data() {
        let prices = vec![100.0, 101.0, 102.0];
        // Need period + 1 = 4 points for period=3
        assert_eq!(atr(&prices, 3), None);
        assert_eq!(atr(&prices, 0), None); // guard period == 0
    }

    #[test]
    fn test_atr_is_zero_for_flat_prices() {
        let prices = vec![100.0, 100.0, 100.0, 100.0];
        // TRs: 0, 0, 0 -> ATR = 0
        let result = atr(&prices, 3).unwrap();
        assert!((result - 0.0).abs() < 1e-12);
    }

    #[test]
    fn test_atr_for_increasing_prices_is_mean_of_diffs() {
        // Prices: 10, 11, 13, 16
        // TRs: |11-10|=1, |13-11|=2, |16-13|=3 => ATR = (1+2+3)/3 = 2
        let prices = vec![10.0, 11.0, 13.0, 16.0];
        let result = atr(&prices, 3).unwrap();
        assert!((result - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_atr_with_period_one_is_last_abs_change() {
        // Prices: 10, 13, 9
        // TR_last = |9 - 13| = 4
        let prices = vec![10.0, 13.0, 9.0];
        let result = atr(&prices, 1).unwrap();
        assert!((result - 4.0).abs() < 1e-12);
    }

    #[test]
    fn test_atr_filter_from_history_returns_none_when_not_enough_data() {
        let prices = vec![100.0, 101.0, 102.0, 103.0];
        let period = 3;

        // Need prices.len() >= period + 2 (5 here). We only have 4.
        let result = AtrFilter::from_history(&prices, period, 0.4);
        assert!(result.is_none());
    }

    #[test]
    fn test_atr_filter_from_history_computes_percentiles_correctly_for_simple_series() {
        // Use a small synthetic series where ATR% is easy to reason about.
        //
        // Prices: 10, 11, 13, 16, 15
        //
        // period = 2
        // end = 3 -> slice [10, 11, 13]
        //   TRs: |11-10| = 1, |13-11| = 2 -> ATR = 1.5
        //   ATR% = 1.5 / 13 ≈ 0.1153846
        //
        // end = 4 -> slice [10, 11, 13, 16]
        //   TRs: |11-10| = 1, |13-11| = 2, |16-13| = 3
        //   Last 2 TRs for period=2: 2, 3 -> ATR = 2.5
        //   ATR% = 2.5 / 16 = 0.15625
        //
        // end = 5 -> slice [10, 11, 13, 16, 15]
        //   Last 2 TRs: |16-13| = 3, |15-16| = 1 -> ATR = 2.0
        //   ATR% = 2.0 / 15 ≈ 0.1333333
        //
        // atr_percents (unsorted) ≈ [0.11538, 0.15625, 0.13333]
        // sorted ≈ [0.11538, 0.13333, 0.15625]

        let prices = vec![10.0, 11.0, 13.0, 16.0, 15.0];
        let period = 2;

        // Percentile 0.0 -> clamped to 0 -> idx = 0 -> lowest value
        let f0 = AtrFilter::from_history(&prices, period, 0.0).unwrap();
        assert_eq!(f0.period, period);
        assert!((f0.floor - 0.1153846).abs() < 1e-6);

        // Percentile 1.0 -> idx = len-1 -> highest value
        let f1 = AtrFilter::from_history(&prices, period, 1.0).unwrap();
        assert_eq!(f1.period, period);
        assert!((f1.floor - 0.15625).abs() < 1e-6);

        // Percentile 0.5 -> (len-1)*0.5 = 2*0.5 = 1.0 -> round -> idx = 1
        // should pick the middle value ≈ 0.13333
        let fmid = AtrFilter::from_history(&prices, period, 0.5).unwrap();
        assert_eq!(fmid.period, period);
        assert!((fmid.floor - 0.1333333).abs() < 1e-6);
    }

    #[test]
    fn test_atr_filter_from_history_clamps_percentile_below_zero_to_zero() {
        let prices = vec![10.0, 11.0, 13.0, 16.0, 15.0];
        let period = 2;

        // percentile = -1.0 -> clamped to 0.0
        let f = AtrFilter::from_history(&prices, period, -1.0).unwrap();
        // Should equal what percentile=0.0 would give
        assert!((f.floor - 0.1153846).abs() < 1e-6);
    }

    #[test]
    fn test_atr_filter_from_history_clamps_percentile_above_one_to_one() {
        let prices = vec![10.0, 11.0, 13.0, 16.0, 15.0];
        let period = 2;

        // percentile = 2.0 -> clamped to 1.0
        let f = AtrFilter::from_history(&prices, period, 2.0).unwrap();
        // Should equal what percentile=1.0 would give
        assert!((f.floor - 0.15625).abs() < 1e-6);
    }
}
