#[derive(Copy, Clone)]
pub struct Smas {
    pub sma_short: f64,
    pub sma_long: f64,
    pub prev_sma_short: f64,
    pub prev_sma_long: f64,
}

#[derive(Copy, Clone, Debug)]
pub struct SmaConfig {
    pub short_window: usize,
    pub long_window: usize,
}

impl SmaConfig {
    pub fn sma_20_50() -> Self {
        Self {
            short_window: 20,
            long_window: 50,
        }
    }
}

/// Compute the simple moving average over the last `window` values.
/// Returns None if there isn't enough data.
pub fn simple_moving_average(prices: &[f64], window: usize) -> Option<f64> {
    if prices.len() < window {
        return None;
    }

    let start = prices.len() - window;
    let slice = &prices[start..];
    let sum: f64 = slice.iter().copied().sum();
    Some(sum / window as f64)
}

/// Compute SMA<short>, SMA<long> and their "previous candle" versions.
/// Returns None if not enough data (needs at least <long+1> prices).
pub fn compute_smas(prices: &[f64], cfg: SmaConfig) -> Option<Smas> {
    if prices.len() < cfg.long_window + 1 {
        return None;
    }

    let sma_short = simple_moving_average(prices, cfg.short_window)?;
    let sma_long = simple_moving_average(prices, cfg.long_window)?;

    let prev_slice = &prices[..prices.len() - 1];
    let prev_sma_short = simple_moving_average(prev_slice, cfg.short_window)?;
    let prev_sma_long = simple_moving_average(prev_slice, cfg.long_window)?;

    Some(Smas {
        sma_short,
        sma_long,
        prev_sma_short,
        prev_sma_long,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, eps: f64) {
        assert!(
            (a - b).abs() <= eps,
            "expected {b}, got {a} (diff = {})",
            (a - b).abs()
        );
    }

    #[test]
    fn test_simple_moving_average_returns_none_when_not_enough_data() {
        let prices = vec![1.0, 2.0, 3.0];
        assert_eq!(simple_moving_average(&prices, 4), None);
    }

    #[test]
    fn test_simple_moving_average_with_exact_window_length_uses_all_values() {
        let prices = vec![1.0, 2.0, 3.0, 4.0];
        // average = (1 + 2 + 3 + 4) / 4 = 2.5
        let sma = simple_moving_average(&prices, 4).unwrap();
        approx_eq(sma, 2.5, 1e-9);
    }

    #[test]
    fn test_simple_moving_average_with_window_smaller_than_length_uses_last_window_values() {
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // window = 3 -> last 3 values: 3, 4, 5
        // average = (3 + 4 + 5) / 3 = 4.0
        let sma = simple_moving_average(&prices, 3).unwrap();
        approx_eq(sma, 4.0, 1e-9);
    }

    #[test]
    fn test_simple_moving_average_with_window_one_is_last_element() {
        let prices = vec![10.0, 20.0, 30.0];
        let sma = simple_moving_average(&prices, 1).unwrap();
        approx_eq(sma, 30.0, 1e-9);
    }

    #[test]
    fn test_simple_moving_average_on_empty_slice_is_none() {
        let prices: Vec<f64> = Vec::new();
        assert_eq!(simple_moving_average(&prices, 1), None);
    }

    #[test]
    fn test_compute_smas_returns_none_when_less_than_51_prices() {
        let prices: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        assert!(compute_smas(&prices, SmaConfig::sma_20_50()).is_none());
    }

    #[test]
    fn test_compute_smas_with_exactly_51_prices() {
        // prices = 1..=51
        let prices: Vec<f64> = (1..=51).map(|x| x as f64).collect();

        let smas = compute_smas(&prices, SmaConfig::sma_20_50()).expect("should have SMAs");

        // Last 20 values: 32..=51 -> average = (32 + 51) / 2 = 41.5
        approx_eq(smas.sma_short, 41.5, 1e-9);

        // Last 50 values: 2..=51 -> average = (2 + 51) / 2 = 26.5
        approx_eq(smas.sma_long, 26.5, 1e-9);

        // prev_slice = 1..=50
        // Prev SMA20: last 20 of 1..=50 -> 31..=50 -> avg = (31 + 50) / 2 = 40.5
        approx_eq(smas.prev_sma_short, 40.5, 1e-9);

        // Prev SMA50: last 50 of 1..=50 -> 1..=50 -> avg = (1 + 50) / 2 = 25.5
        approx_eq(smas.prev_sma_long, 25.5, 1e-9);
    }

    #[test]
    fn test_compute_smas_with_more_than_51_prices() {
        // prices = 1..=60
        let prices: Vec<f64> = (1..=60).map(|x| x as f64).collect();

        let smas = compute_smas(&prices, SmaConfig::sma_20_50()).expect("should have SMAs");

        // Current SMA20: last 20 values -> 41..=60 -> avg = (41 + 60) / 2 = 50.5
        approx_eq(smas.sma_short, 50.5, 1e-9);

        // Current SMA50: last 50 values -> 11..=60 -> avg = (11 + 60) / 2 = 35.5
        approx_eq(smas.sma_long, 35.5, 1e-9);

        // prev_slice = 1..=59
        // Prev SMA20: last 20 of 1..=59 -> 40..=59 -> avg = (40 + 59) / 2 = 49.5
        approx_eq(smas.prev_sma_short, 49.5, 1e-9);

        // Prev SMA50: last 50 of 1..=59 -> 10..=59 -> avg = (10 + 59) / 2 = 34.5
        approx_eq(smas.prev_sma_long, 34.5, 1e-9);
    }
}
