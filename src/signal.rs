use crate::data::Sample;
use crate::indicators::{AtrFilter, Smas};
use crate::patterns::{
    is_breakdown_below_recent_low, is_breakout_above_recent_high, is_pullback_to_sma20_and_bounce,
    is_pullback_to_sma20_and_reject_down,
};

pub struct AnalysisResult {
    pub last: Sample,
    pub smas: Smas,
    pub suggestion: String,
    pub reason: String,
}

/// Advanced trading rule based on:
/// - Breakout above recent high in an uptrend
/// - Breakout below recent low in a downtrend
/// - Pullback to SMA20 + bounce (uptrend)
/// - Pullback to SMA20 + rejection (downtrend)
/// - Golden Cross / Death Cross detection (using previous + current SMAs)
/// - Trend filter using SMA50 slope
/// - Price confirmation (price relative to SMA20 & SMA50)
///
/// Returns (short_suggestion, optional_detailed_reason)
pub fn analyze(
    hourly: &[Sample],
    prices: &[f64],
    smas: Smas,
    atr_filter: Option<AtrFilter>,
    breakout_lookback: usize,
) -> AnalysisResult {
    let last = hourly.last().expect("hourly is non-empty").to_owned();
    let (suggestion, reason) = suggest_action(prices, smas, atr_filter, breakout_lookback);
    AnalysisResult {
        last,
        smas,
        suggestion,
        reason,
    }
}

fn suggest_action(
    prices: &[f64],
    smas: Smas,
    atr_filter_opt: Option<AtrFilter>,
    breakout_lookback: usize,
) -> (String, String) {
    let last_price = *prices.last().expect("prices is non-empty");

    if let Some(atr_filter) = atr_filter_opt {
        // ~~~~ Volatility filter (ATR) ~~~~
        let atr_p = match atr_filter.atr_percent(prices) {
            Some(v) => v,
            None => {
                return (
                    "HOLD".into(),
                    format!(
                        "Insufficient data for ATR({}) volatility filter",
                        atr_filter.period()
                    ),
                );
            }
        };

        if atr_p < atr_filter.floor() {
            let atr_pct = atr_p * 100.0;
            let floor_pct = atr_filter.floor() * 100.0;
            return (
                "HOLD".into(),
                format!(
                    "Volatility too low: ATR({}) = {:.2}% < floor {:.2}%",
                    atr_filter.period(),
                    atr_pct,
                    floor_pct
                ),
            );
        }
    }

    // Trend and slope filters
    // We combined two separate signals:
    // * Trend direction (SMA20 > SMA50 or SMA20 < SMA50)
    // * Trend slope (SMA50 rising or falling)
    // We did this because:
    // * Using only SMA50 slope is not enough to define a strong trend.
    // * Using only SMA20 > SMA50 is not safe without confirming SMA50 is rising.
    // * Combining them is a stronger, more reliable trend filter.
    let uptrend = smas.sma20 > smas.sma50 && smas.sma50 >= smas.prev_sma50;
    let downtrend = smas.sma20 < smas.sma50 && smas.sma50 <= smas.prev_sma50;

    // Price confirmation: where is price relative to the MAs?
    let price_above_both = last_price > smas.sma20 && last_price > smas.sma50;
    let price_below_both = last_price < smas.sma20 && last_price < smas.sma50;

    // ~~~~ SELL patterns ~~~~

    // 1. Breakdown below a recent low in a downtrend
    if downtrend && is_breakdown_below_recent_low(prices, breakout_lookback) && price_below_both {
        return (
            "SELL".into(),
            "Breakdown below recent low in downtrend (SMA20 < SMA50)".into(),
        );
    }

    // 2. Pullback up to SMA20 + rejection in a downtrend
    if downtrend && is_pullback_to_sma20_and_reject_down(prices, smas.sma20) {
        return (
            "SELL".into(),
            "Pullback up to SMA20 and rejection in downtrend".into(),
        );
    }

    // ~~~~ BUY patterns ~~~~

    // 3. Breakout above a recent high in an uptrend
    if uptrend && is_breakout_above_recent_high(prices, breakout_lookback) && price_above_both {
        return (
            "BUY".into(),
            "Breakout above recent high in uptrend (SMA20 > SMA50)".into(),
        );
    }

    // 4. Pullback to SMA20 + bounce in an uptrend
    if uptrend && is_pullback_to_sma20_and_bounce(prices, smas.sma20) {
        return (
            "BUY".into(),
            "Pullback to SMA20 and bounce in uptrend".into(),
        );
    }

    // ~~~~ Crossovers ~~~~

    // 5. Strong BUY: fresh Golden Cross in an uptrend with price confirmation
    let golden_cross = smas.prev_sma20 <= smas.prev_sma50 && smas.sma20 > smas.sma50;
    if golden_cross && uptrend && price_above_both {
        return (
            "BUY".into(),
            "Golden Cross + SMA50 rising + price above SMA20 & SMA50".into(),
        );
    }

    // 6. Strong SELL: fresh Death Cross in a downtrend with price confirmation
    let death_cross = smas.prev_sma20 >= smas.prev_sma50 && smas.sma20 < smas.sma50;
    if death_cross && downtrend && price_below_both {
        return (
            "SELL".into(),
            "Death Cross + SMA50 falling + price below SMA20 & SMA50".into(),
        );
    }

    // ~~~~ Bias-only ~~~~

    // 7. No fresh cross but we are clearly in an uptrend
    if smas.sma20 > smas.sma50 && price_above_both {
        return (
            "HOLD / LONG BIAS".into(),
            "Uptrend (SMA20 > SMA50) and price above both averages".into(),
        );
    }

    // 8. No fresh cross but we are clearly in a downtrend
    if smas.sma20 < smas.sma50 && price_below_both {
        return (
            "HOLD / SHORT BIAS".into(),
            "Downtrend (SMA20 < SMA50) and price below both averages".into(),
        );
    }

    // 5. Otherwise, no clear edge
    (
        "HOLD".into(),
        "No clear breakout, pullback bounce/rejection, or crossover signal".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BREAKOUT_LOOKBACK: usize = 5;

    impl Smas {
        fn downtrend_for_breakdown() -> Self {
            Self {
                sma20: 95.0,
                sma50: 100.0,
                prev_sma20: 96.0,
                prev_sma50: 101.0, // sma50 <= prev_sma50 => 100 <= 101
            }
        }

        fn downtrend_for_pullback() -> Self {
            Self {
                sma20: 100.0,
                sma50: 110.0,
                prev_sma20: 101.0,
                prev_sma50: 111.0,
            }
        }

        fn uptrend_for_breakout() -> Self {
            Self {
                sma20: 105.0,
                sma50: 100.0,
                prev_sma20: 104.0,
                prev_sma50: 99.0, // sma50 >= prev_sma50 => 100 >= 99
            }
        }

        fn uptrend_for_bounce() -> Self {
            Self {
                sma20: 100.0,
                sma50: 95.0,
                prev_sma20: 99.0,
                prev_sma50: 94.0,
            }
        }

        fn golden_cross() -> Self {
            Self {
                sma20: 105.0,
                sma50: 100.0,
                prev_sma20: 95.0,
                prev_sma50: 100.0, // prev_sma20 <= prev_sma50 && sma20 > sma50
            }
        }

        fn death_cross() -> Self {
            Self {
                sma20: 95.0,
                sma50: 100.0,
                prev_sma20: 105.0,
                prev_sma50: 100.0, // prev_sma20 >= prev_sma50 && sma20 < sma50
            }
        }

        fn long_bias_only() -> Self {
            Self {
                sma20: 105.0,
                sma50: 100.0,
                prev_sma20: 105.0,
                prev_sma50: 100.0, // no golden cross (prev_sma20 <= prev_sma50 is false)
            }
        }

        fn short_bias_only() -> Self {
            Self {
                sma20: 95.0,
                sma50: 100.0,
                prev_sma20: 95.0,
                prev_sma50: 100.0, // no death cross (prev_sma20 >= prev_sma50 is false)
            }
        }
    }

    #[test]
    fn test_suggest_action_sell_on_breakdown_below_recent_low_in_downtrend() {
        // window = [100, 99, 98, 97, 96], recent_low = 96
        // last_price = 90 < 96 * (1 - eps) -> breakdown
        let prices = vec![100.0, 99.0, 98.0, 97.0, 96.0, 90.0];
        let smas = Smas::downtrend_for_breakdown();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "SELL");
        assert_eq!(
            reason,
            "Breakdown below recent low in downtrend (SMA20 < SMA50)"
        );
    }

    #[test]
    fn test_suggest_action_sell_on_pullback_to_sma20_and_rejection_in_downtrend() {
        // Last 3 candles:
        // p2 = 95 (below SMA20)
        // p1 = 100 (pullback to SMA20)
        // p0 =  98 (reject down)
        //
        // len = 3 => breakdown / breakout can't trigger (need >= 6)
        let prices = vec![95.0, 100.0, 98.0];
        let smas = Smas::downtrend_for_pullback();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "SELL");
        assert_eq!(reason, "Pullback up to SMA20 and rejection in downtrend");
    }

    #[test]
    fn test_suggest_action_buy_on_breakout_above_recent_high_in_uptrend() {
        // prices: [100, 101, 102, 103, 104, 110]
        // window (lookback=5) = [100..104], recent_high = 104
        // last_price = 110 > 104 * (1 + eps) => breakout
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0, 110.0];
        let smas = Smas::uptrend_for_breakout();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "BUY");
        assert_eq!(
            reason,
            "Breakout above recent high in uptrend (SMA20 > SMA50)"
        );
    }

    #[test]
    fn test_suggest_action_buy_on_pullback_to_sma20_and_bounce_in_uptrend() {
        // Last 3 candles:
        // p2 = 105 (> sma20=100)
        // p1 = 100 (pullback to SMA20)
        // p0 = 103 (bounce above)
        //
        // len = 3 => no breakout/breakdown. Uptrend is true.
        let prices = vec![105.0, 100.0, 103.0];
        let smas = Smas::uptrend_for_bounce();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "BUY");
        assert_eq!(reason, "Pullback to SMA20 and bounce in uptrend");
    }

    #[test]
    fn test_suggest_action_buy_on_golden_cross_with_confirmation() {
        // Uptrend + golden cross + price_above_both.
        // prices: [100, 102, 106]; last_price = 106
        let prices = vec![100.0, 102.0, 106.0];
        let smas = Smas::golden_cross();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "BUY");
        assert_eq!(
            reason,
            "Golden Cross + SMA50 rising + price above SMA20 & SMA50"
        );
    }

    #[test]
    fn test_suggest_action_sell_on_death_cross_with_confirmation() {
        // Downtrend + death cross + price_below_both.
        // prices: [100, 99, 94]; last_price = 94
        let prices = vec![100.0, 99.0, 94.0];
        let smas = Smas::death_cross();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "SELL");
        assert_eq!(
            reason,
            "Death Cross + SMA50 falling + price below SMA20 & SMA50"
        );
    }

    #[test]
    fn test_suggest_action_hold_long_bias_when_uptrend_but_no_strong_signal() {
        // Uptrend, price above both MAs, but no cross / breakout / pullback pattern.
        // prices: [101, 103, 106]; p2 = 101 (not > sma20=105) => no bounce pattern.
        let prices = vec![101.0, 103.0, 106.0];
        let smas = Smas::long_bias_only();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "HOLD / LONG BIAS");
        assert_eq!(
            reason,
            "Uptrend (SMA20 > SMA50) and price above both averages"
        );
    }

    #[test]
    fn test_suggest_action_hold_short_bias_when_downtrend_but_no_strong_signal() {
        // Downtrend, price below both MAs, but no cross / breakdown / pullback pattern.
        // prices: [100, 95, 90]; len=3 -> no breakdown
        let prices = vec![100.0, 95.0, 90.0];
        let smas = Smas::short_bias_only();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "HOLD / SHORT BIAS");
        assert_eq!(
            reason,
            "Downtrend (SMA20 < SMA50) and price below both averages"
        );
    }

    #[test]
    fn test_suggest_action_generic_hold_when_no_trend_or_signal() {
        // Flat SMAs, price neither above nor below both.
        let prices = vec![100.0, 100.0, 100.0];
        let smas = Smas {
            sma20: 100.0,
            sma50: 100.0,
            prev_sma20: 100.0,
            prev_sma50: 100.0,
        };

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, None, TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "HOLD");
        assert_eq!(
            reason,
            "No clear breakout, pullback bounce/rejection, or crossover signal"
        );
    }

    #[test]
    fn test_suggest_action_hold_when_volatility_below_floor() {
        // Flat / almost-flat prices -> ATR% ≈ 0, definitely below a 1% floor.
        // This should trigger the ATR gate *before* any trend / pattern logic.
        let prices = vec![100.0; 40]; // enough points for ATR(14) to be computed

        let smas = Smas {
            sma20: 100.0,
            sma50: 100.0,
            prev_sma20: 100.0,
            prev_sma50: 100.0,
        };

        // High-ish floor: 1% ATR required.
        // Since prices are constant, ATR% ≈ 0 -> won't pass the gate
        let atr_filter = AtrFilter::new_fixed(14, 0.01);

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, Some(atr_filter), TEST_BREAKOUT_LOOKBACK);

        assert_eq!(suggestion, "HOLD");
        assert!(
            reason.contains("Volatility too low"),
            "Expected 'Volatility too low' in reason, got: {reason}"
        );
    }
}
