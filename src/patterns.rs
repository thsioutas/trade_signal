/// Check if we have a breakdown below a recent low.
///
/// - Lookback N (e.g. 5) means:
///   Use the last N-1 candles *before* the current one
///   and see if the last price < min of those lows.
pub fn is_breakdown_below_recent_low(prices: &[f64], lookback: usize) -> bool {
    if prices.len() < lookback + 1 {
        return false;
    }

    let last_idx = prices.len() - 1;
    let start = last_idx.saturating_sub(lookback);
    // Window: from start .. last_idx (excluding the last candle)
    let window = &prices[start..last_idx];

    let recent_low = window.iter().copied().fold(f64::INFINITY, f64::min);

    let last_price = prices[last_idx];

    // Small epsilon so exact equality doesn't count as breakdown
    let epsilon = 1e-6;
    last_price < recent_low * (1.0 - epsilon)
}

/// Check if we have a pullback up to SMA(short) and rejection down:
///
/// Pattern over last 3 closes:
/// - p2 (2 candles ago) < sma_short
/// - p1 > p2 and near/above sma_short
/// - p0 < sma_short and p0 < p1
///
/// `tol` below SMA(short) considered "touching" from below
pub fn is_pullback_to_sma_short_and_reject_down(prices: &[f64], sma_short: f64, tol: f64) -> bool {
    if prices.len() < 3 {
        return false;
    }

    let n = prices.len();
    let p2 = prices[n - 3];
    let p1 = prices[n - 2];
    let p0 = prices[n - 1];

    let was_below = p2 < sma_short;
    let pulled_back_near = p1 > p2 && p1 >= sma_short * (1.0 - tol); // close to or slightly above SMA(short)
    let rejected = p0 < sma_short && p0 < p1;

    was_below && pulled_back_near && rejected
}

/// Check if we have a breakout above a recent high.
///
/// - Lookback N (e.g. 5) means:
///   Use the last N-1 candles *before* the current one
///   and see if the last price > max of those highs.
pub fn is_breakout_above_recent_high(prices: &[f64], lookback: usize) -> bool {
    if prices.len() < lookback + 1 {
        return false;
    }

    let last_idx = prices.len() - 1;
    let start = last_idx.saturating_sub(lookback);
    // Window: from start .. last_idx (excluding the last candle)
    let window = &prices[start..last_idx];

    let recent_high = window.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let last_price = prices[last_idx];

    // Small epsilon so exact equality doesn't count as breakout
    let epsilon = 1e-6;
    last_price > recent_high * (1.0 + epsilon)
}

/// Check if we have a pullback to SMA(short) and bounce:
///
/// Pattern over last 3 closes:
/// - p2 (2 candles ago) > sma_short
/// - p1 < p2 and near/under sma_short
/// - p0 > sma_short and p0 > p1
///   <tol> above SMA(short) considered "touching"
pub fn is_pullback_to_sma_short_and_bounce(prices: &[f64], sma_short: f64, tol: f64) -> bool {
    if prices.len() < 3 {
        return false;
    }

    let n = prices.len();
    let p2 = prices[n - 3];
    let p1 = prices[n - 2];
    let p0 = prices[n - 1];

    let was_above = p2 > sma_short;
    let pulled_back_near = p1 < p2 && p1 <= sma_short * (1.0 + tol); // can be slightly above or below SMA(short)
    let bounced = p0 > sma_short && p0 > p1;

    was_above && pulled_back_near && bounced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_breakdown_below_recent_low_false_when_not_enough_data() {
        let prices = vec![100.0, 99.0]; // len = 2
        let lookback = 5; // needs at least lookback + 1 = 6
        assert!(!is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_breakdown_below_recent_low_false_when_last_price_equals_recent_low() {
        // Minimal length: lookback + 1
        // window = first 3, last = 4th
        //
        // window = [100.0, 98.0, 97.0]
        // recent_low = 97.0
        // last_price = 97.0  -> should NOT count as breakdown
        let prices = vec![100.0, 98.0, 97.0, 97.0];
        let lookback = 3;

        assert!(!is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_breakdown_below_recent_low_true_when_last_price_is_strictly_below_recent_low() {
        // window = [100.0, 98.0, 97.0]
        // recent_low = 97.0
        // last_price = 96.0 < 97.0 * (1 - epsilon) => breakdown
        let prices = vec![100.0, 98.0, 97.0, 96.0];
        let lookback = 3;

        assert!(is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_breakdown_below_recent_low_false_when_last_price_is_above_recent_low() {
        // window = [100.0, 99.0, 98.0]
        // recent_low = 98.0
        // last_price = 98.000001 > 98.0 * (1 - epsilon) => no breakdown
        let prices = vec![100.0, 99.0, 98.0, 98.000001];
        let lookback = 3;

        assert!(!is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_breakdown_below_recent_low_uses_only_lookback_window_not_older_lows() {
        // Here the global minimum (50.0) is outside the lookback window.
        //
        // prices:  [50.0, 60.0, 55.0, 54.0, 53.0]
        // indices:   0     1     2     3     4
        //
        // last_idx = 4
        // start    = last_idx - lookback = 4 - 3 = 1
        // window   = prices[1..4] = [60.0, 55.0, 54.0]
        // recent_low = 54.0
        // last_price = 53.0 => breakdown vs 54.0
        //
        // If implementation incorrectly used *all* prices (including 50.0),
        // recent_low would be 50.0 and the signal might differ.
        let prices = vec![50.0, 60.0, 55.0, 54.0, 53.0];
        let lookback = 3;

        assert!(is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_breakdown_below_recent_low_window_excludes_last_candle_from_recent_low() {
        // This test ensures the last candle is NOT part of the "recent low" window.
        //
        // prices:  [10.0, 9.0, 8.0, 7.0, 6.0]
        // indices:   0    1    2    3    4
        //
        // lookback = 2
        // last_idx = 4
        // start    = 4 - 2 = 2
        // window   = prices[2..4] = [8.0, 7.0]
        // recent_low = 7.0
        // last_price = 6.0 -> breakdown vs 7.0
        //
        // If the code mistakenly included last candle in the window,
        // window would be [8.0, 7.0, 6.0], recent_low = 6.0
        // and last_price < recent_low*(1 - eps) would be false.
        let prices = vec![10.0, 9.0, 8.0, 7.0, 6.0];
        let lookback = 2;

        assert!(is_breakdown_below_recent_low(&prices, lookback));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_when_not_enough_prices() {
        let sma_short = 100.0;

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &[],
            sma_short,
            0.0003
        ));
        assert!(!is_pullback_to_sma_short_and_reject_down(
            &[99.0],
            sma_short,
            0.0003
        ));
        assert!(!is_pullback_to_sma_short_and_reject_down(
            &[99.0, 100.0],
            sma_short,
            0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_true_for_valid_pullback_and_rejection_pattern()
    {
        // p2 < sma_short   (was below)
        // p1 > p2 and near/above sma_short   (pullback up to SMA(short))
        // p0 < sma_short and p0 < p1         (rejection down)
        //
        // Let sma_short = 100.0
        // p2 =  95.0   (below)
        // p1 = 100.0   (touching)
        // p0 =  98.0   (rejecting down, still below SMA(short))
        let prices = vec![95.0, 100.0, 98.0];
        let sma_short = 100.0;

        assert!(is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_if_p2_is_not_below_sma_short() {
        // p2 >= sma_short -> was_below = false
        let prices = vec![100.0, 101.0, 99.0]; // p2 = 100.0, sma_short = 100.0
        let sma_short = 100.0;

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_if_pullback_not_higher_than_p2() {
        // p1 <= p2 -> pulled_back_near = false
        let sma_short = 100.0;
        let prices = vec![95.0, 95.0, 94.0]; // p1 is not > p2

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_if_pullback_not_close_enough_to_sma_short()
     {
        // pulled_back_near requires:
        // p1 > p2 AND p1 >= sma_short * (1 - tol)
        //
        // sma_short = 100
        // tol = 0.003 -> threshold = 99.7
        //
        // p1 = 99.6  -> slightly below threshold
        let sma_short = 100.0;
        let prices = vec![
            95.0, // p2
            99.6, // p1 (too low vs 99.7)
            98.0, // p0
        ];

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_true_if_pullback_is_exactly_at_lower_tolerance_boundary()
     {
        // p1 == sma_short * (1 - tol) should pass the >= check.
        let sma_short = 100.0;
        let tol = 0.003;
        let threshold = sma_short * (1.0 - tol); // 99.7

        let prices = vec![
            95.0,      // p2 (below SMA(short))
            threshold, // p1 (exactly at tolerance boundary)
            98.0,      // p0 (rejection)
        ];

        assert!(is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_if_last_price_not_below_sma_short() {
        // rejected requires p0 < sma_short
        let sma_short = 100.0;
        let prices = vec![
            95.0,  // p2 below
            100.0, // p1 touching
            100.5, // p0 above SMA(short) => no rejection
        ];

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_false_if_last_price_not_below_p1() {
        // rejected requires p0 < p1
        let sma_short = 100.0;
        let prices = vec![
            95.0,  // p2 below
            100.0, // p1 near SMA(short)
            100.0, // p0 == p1 -> no rejection
        ];

        assert!(!is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_reject_down_works_when_more_than_three_prices_present() {
        // Function should only look at the LAST 3 prices.
        //
        // older: [1.0, 2.0, 3.0]  (ignored)
        // last3: p2 = 95.0, p1 = 100.0, p0 = 98.0  -> valid pattern
        let sma_short = 100.0;
        let prices = vec![1.0, 2.0, 3.0, 95.0, 100.0, 98.0];

        assert!(is_pullback_to_sma_short_and_reject_down(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_breakout_above_recent_high_false_when_not_enough_data() {
        let prices = vec![100.0, 101.0]; // len = 2
        let lookback = 5; // needs at least lookback + 1 = 6
        assert!(!is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_breakout_above_recent_high_false_when_last_price_equals_recent_high() {
        // Minimal length: lookback + 1
        // window = first 3, last = 4th
        //
        // window = [100.0, 102.0, 103.0]
        // recent_high = 103.0
        // last_price = 103.0  -> should NOT count as breakout
        let prices = vec![100.0, 102.0, 103.0, 103.0];
        let lookback = 3;

        assert!(!is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_breakout_above_recent_high_true_when_last_price_is_strictly_above_recent_high() {
        // window = [100.0, 102.0, 103.0]
        // recent_high = 103.0
        // last_price = 104.0 > 103.0 * (1 + epsilon) => breakout
        let prices = vec![100.0, 102.0, 103.0, 104.0];
        let lookback = 3;

        assert!(is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_breakout_above_recent_high_false_when_last_price_is_only_slightly_above_but_below_epsilon_threshold()
     {
        // pulled from epsilon = 1e-6 in implementation
        //
        // recent_high = 100.0
        // threshold = 100.0 * (1 + 1e-6) = 100.0001
        // last_price = 100.00001 < 100.0001 -> should NOT count as breakout
        let prices = vec![
            99.0,      // in window
            100.0,     // recent high
            100.00001, // last, slightly above but not enough
        ];
        let lookback = 2; // window = [99.0, 100.0], recent_high = 100.0

        assert!(!is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_breakout_above_recent_high_uses_only_lookback_window_not_older_highs() {
        // Here the global maximum (200.0) is outside the lookback window.
        //
        // prices:  [200.0, 90.0, 95.0, 100.0, 101.0]
        // indices:   0      1     2      3      4
        //
        // last_idx = 4
        // start    = last_idx - lookback = 4 - 3 = 1
        // window   = prices[1..4] = [90.0, 95.0, 100.0]
        // recent_high = 100.0
        // last_price  = 101.0 -> breakout vs 100.0
        //
        // If implementation incorrectly used *all* prices (including 200.0),
        // recent_high would be 200.0 and this would NOT be a breakout.
        let prices = vec![200.0, 90.0, 95.0, 100.0, 101.0];
        let lookback = 3;

        assert!(is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_breakout_above_recent_high_window_excludes_last_candle_from_recent_high() {
        // This test ensures the last candle is NOT part of the "recent high" window.
        //
        // prices:  [90.0, 95.0, 100.0, 105.0]
        // indices:   0     1      2      3
        //
        // lookback = 2
        // last_idx = 3
        // start    = 3 - 2 = 1
        // window   = prices[1..3] = [95.0, 100.0]
        // recent_high = 100.0
        // last_price  = 105.0 -> breakout vs 100.0
        //
        // If the code mistakenly included the last candle in the window,
        // window would be [95.0, 100.0, 105.0], recent_high = 105.0
        // and last_price > recent_high*(1 + eps) would be false.
        let prices = vec![90.0, 95.0, 100.0, 105.0];
        let lookback = 2;

        assert!(is_breakout_above_recent_high(&prices, lookback));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_when_not_enough_prices() {
        let sma_short = 100.0;

        assert!(!is_pullback_to_sma_short_and_bounce(&[], sma_short, 0.0003));
        assert!(!is_pullback_to_sma_short_and_bounce(
            &[101.0],
            sma_short,
            0.0003
        ));
        assert!(!is_pullback_to_sma_short_and_bounce(
            &[101.0, 100.0],
            sma_short,
            0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_true_for_valid_pullback_and_bounce_pattern() {
        // p2 > sma_short      (was above)
        // p1 < p2 and near/below/just-above sma_short   (pullback)
        // p0 > sma_short and p0 > p1                    (bounce)
        //
        // Let sma_short = 100.0:
        // p2 = 105.0 (above)
        // p1 = 100.0 (pullback to SMA(short))
        // p0 = 103.0 (bounce above SMA(short) and above p1)
        let sma_short = 100.0;
        let prices = vec![105.0, 100.0, 103.0];

        assert!(is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_if_p2_not_above_sma_short() {
        // was_above requires p2 > sma_short
        let sma_short = 100.0;
        let prices = vec![
            100.0, // p2 == sma_short -> not strictly above
            99.0, 101.0,
        ];

        assert!(!is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_if_no_pullback_p1_not_lower_than_p2() {
        // pulled_back_near requires p1 < p2
        let sma_short = 100.0;
        let prices = vec![
            105.0, // p2
            105.0, // p1 (no pullback)
            106.0, // p0
        ];

        assert!(!is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_if_pullback_too_far_above_sma_short() {
        // pulled_back_near requires:
        // p1 < p2 AND p1 <= sma_short * (1 + tol)
        //
        // sma_short = 100
        // tol = 0.003 -> upper bound = 100.3
        //
        // p1 = 100.4  -> just above allowed band
        let sma_short = 100.0;
        let tol = 0.003;
        let upper = sma_short * (1.0 + tol); // 100.3

        let prices = vec![
            110.0,       // p2 > sma_short
            upper + 0.1, // p1 too high above SMA(short)
            105.0,       // p0
        ];

        assert!(!is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_true_if_pullback_exactly_at_upper_tolerance_boundary()
     {
        // p1 == sma_short * (1 + tol) should pass the <= check.
        let sma_short = 100.0;
        let tol = 0.003;
        let upper = sma_short * (1.0 + tol); // 100.3

        let prices = vec![
            110.0, // p2 above
            upper, // p1 exactly at tolerance boundary
            105.0, // p0 bounce above SMA(short)
        ];

        assert!(is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_if_last_price_not_above_sma_short() {
        // bounced requires p0 > sma_short
        let sma_short = 100.0;
        let prices = vec![
            105.0, // p2 above
            100.0, // p1 pullback
            99.5,  // p0 below SMA(short) -> no bounce
        ];

        assert!(!is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_false_if_last_price_not_above_p1() {
        // bounced requires p0 > p1
        let sma_short = 100.0;
        let prices = vec![
            105.0, // p2 above
            100.0, // p1
            100.0, // p0 == p1 -> no bounce
        ];

        assert!(!is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }

    #[test]
    fn test_is_pullback_to_sma_short_and_bounce_works_when_more_than_three_prices_present() {
        // Only the last 3 candles matter.
        //
        // older: [1.0, 2.0, 3.0] (ignored)
        // last3:  p2 = 105.0, p1 = 100.0, p0 = 103.0 -> valid pattern
        let sma_short = 100.0;
        let prices = vec![1.0, 2.0, 3.0, 105.0, 100.0, 103.0];

        assert!(is_pullback_to_sma_short_and_bounce(
            &prices, sma_short, 0.0003
        ));
    }
}
