use crate::indicators::simple_moving_average;

/// Market regime in the *bigger picture*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    TrendingUp,
    TrendingDown,
    Sideways,
}

#[derive(Debug, Clone, Copy)]
pub struct RegimeFilter {
    /// Long MA window for big-picture trend (in candles).
    /// On 1h data, 200 ≈ ~8 days.
    pub long_window: usize,

    /// Window used to measure price slope (in candles).
    /// On 1h data, 48 ≈ 2 days.
    pub slope_window: usize,

    /// Minimum % move over `slope_window` to consider it a trend.
    /// Example: 0.02 = 2% move over the slope window.
    pub min_trend_strength: f64,

    /// Minimum total range over slope window to avoid "dead" chop.
    /// Example: 0.03 = 3% high/low range over the slope window.
    pub min_range: f64,
}

impl Default for RegimeFilter {
    fn default() -> Self {
        Self {
            long_window: 200,         // big picture trend
            slope_window: 48,         // last 2 days (on 1h)
            min_trend_strength: 0.02, // 2% over slope window
            min_range: 0.03,          // 3% high/low range
        }
    }
}

impl RegimeFilter {
    /// Detect macro regime (1h candles expected).
    ///
    /// Logic:
    /// 1. Need enough data for long_window & slope_window.
    /// 2. Compute long SMA.
    /// 3. Compute trend over slope_window: price_change%
    /// 4. Compute price range over slope_window.
    /// 5. If trend is small AND range is small => Sideways
    /// 6. Else:
    ///    - if price > long SMA and trend up -> TrendingUp
    ///    - if price < long SMA and trend down -> TrendingDown
    ///    - otherwise Sideways
    pub fn detect_regime(&self, prices: &[f64]) -> Regime {
        let n = prices.len();
        let required = self.long_window.max(self.slope_window) + 1;
        if n < required {
            // Not enough history -> treat as Sideways to avoid overconfidence.
            return Regime::Sideways;
        }

        let sma_long = match simple_moving_average(prices, self.long_window) {
            Some(v) if v > 0.0 => v,
            _ => return Regime::Sideways,
        };

        let end = n - 1;
        let start = n - 1 - self.slope_window;
        let start_price = prices[start];
        let end_price = prices[end];

        if start_price <= 0.0 {
            return Regime::Sideways;
        }

        // % move over slope_window
        let trend = (end_price / start_price) - 1.0;

        // High/low range over slope_window
        let window = &prices[start..=end];
        let (min_p, max_p) = window
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &p| {
                (min.min(p), max.max(p))
            });

        let range = if sma_long > 0.0 {
            (max_p - min_p) / sma_long
        } else {
            0.0
        };

        // Sideways: weak trend AND tiny range
        if trend.abs() < self.min_trend_strength || range < self.min_range {
            return Regime::Sideways;
        }

        // Direction must agree with long SMA & trend
        if end_price > sma_long && trend > 0.0 {
            Regime::TrendingUp
        } else if end_price < sma_long && trend < 0.0 {
            Regime::TrendingDown
        } else {
            Regime::Sideways
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl RegimeFilter {
        fn test_default_regime() -> Self {
            Self {
                long_window: 10,
                slope_window: 5,
                min_trend_strength: 0.02, // 2%
                min_range: 0.03,          // 3%
            }
        }
    }

    #[test]
    fn test_detect_regime_sideways_when_not_enough_history() {
        let rf = RegimeFilter::test_default_regime();
        let required = rf.long_window.max(rf.slope_window) + 1;

        // one less than required
        let prices: Vec<f64> = (0..(required - 1)).map(|i| 100.0 + i as f64).collect();

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_sideways_when_long_sma_invalid_or_zero() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 4;
        rf.slope_window = 2;

        // Last 4 values will be [0, 0, 0, 0] => sma_long == 0
        let prices = vec![100.0, 101.0, 102.0, 0.0, 0.0, 0.0, 0.0];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_sideways_when_start_price_non_positive() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 4;
        rf.slope_window = 2;

        // prices: [..., 0.0, positive, positive]
        // slope_window = 2 => start index is pointing at 0.0
        let prices = vec![100.0, 101.0, 0.0, 102.0, 103.0];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_sideways_when_trend_and_range_below_thresholds() {
        let rf = RegimeFilter::test_default_regime();

        // Almost flat series: small noise around 100
        let prices = vec![
            100.0, 100.1, 99.9, 100.0, 100.2, 99.8, 100.1, 100.0, 100.1, 99.9, 100.0, 100.1, 100.0,
        ];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_trending_up_when_strong_uptrend_and_above_long_sma() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 10;
        rf.slope_window = 5;
        rf.min_trend_strength = 0.01; // 1%
        rf.min_range = 0.01; // 1%

        // Monotonic uptrend
        // Last 6 values: 115, 116, 117, 118, 119, 120
        // => trend over slope_window is positive and > min_trend_strength
        // range over window is also > min_range
        let mut prices = Vec::new();
        for p in 100..=120 {
            prices.push(p as f64);
        }

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::TrendingUp);
    }

    #[test]
    fn test_detect_regime_trending_down_when_strong_downtrend_and_below_long_sma() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 10;
        rf.slope_window = 5;
        rf.min_trend_strength = 0.01;
        rf.min_range = 0.01;

        // Monotonic downtrend
        // Last 6 values decreasing
        let mut prices = Vec::new();
        for p in (80..=100).rev() {
            prices.push(p as f64);
        }

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::TrendingDown);
    }

    #[test]
    fn test_detect_regime_sideways_when_trend_not_strong_enough() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 10;
        rf.slope_window = 5;
        rf.min_trend_strength = 0.05; // require 5%

        // Very slight upward drift: ~1% over slope_window
        let prices = vec![
            100.0, 100.1, 100.2, 100.3, 100.4, 100.5, 100.6, 100.7, 100.8, 100.9, 101.0, 101.1,
        ];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_sideways_when_range_too_small_even_if_trend_exists() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 10;
        rf.slope_window = 5;
        rf.min_trend_strength = 0.0; // ignore trend
        rf.min_range = 0.05; // require 5% range

        // Trend is there but very small range in the window (less than 5%)
        let prices = vec![
            100.0, 100.5, 100.6, 100.7, 100.8, 101.0, 101.1, 101.2, 101.3, 101.4, 101.5,
        ];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }

    #[test]
    fn test_detect_regime_sideways_when_direction_conflicts_with_long_sma() {
        let mut rf = RegimeFilter::test_default_regime();
        rf.long_window = 4;
        rf.slope_window = 3;
        rf.min_trend_strength = 0.01; // 1%
        rf.min_range = 0.01; // 1%

        // Last 4 prices (for SMA & slope window): [100, 140, 130, 110]
        // sma_long = 120
        // start_price = 100, end_price = 110 => trend > 0
        // end_price < sma_long => direction conflicts with long SMA => Sideways.
        let prices = vec![
            50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 100.0, 140.0, 130.0, 110.0,
        ];

        let regime = rf.detect_regime(&prices);

        assert_eq!(regime, Regime::Sideways);
    }
}
