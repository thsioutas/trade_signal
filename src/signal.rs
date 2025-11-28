use crate::data::Sample;
use crate::indicators::Smas;
use crate::patterns::{
    is_breakdown_below_recent_low, is_breakout_above_recent_high, is_pullback_to_sma20_and_bounce,
    is_pullback_to_sma20_and_reject_down,
};

const BREAKOUT_LOOKBACK: usize = 5;

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
pub fn analyze(hourly: &[Sample], prices: &[f64], smas: Smas) -> AnalysisResult {
    let last = hourly.last().expect("hourly is non-empty").to_owned();
    let (suggestion, reason) = suggest_action(prices, smas);
    AnalysisResult {
        last,
        smas,
        suggestion,
        reason,
    }
}

fn suggest_action(prices: &[f64], smas: Smas) -> (String, String) {
    let last_price = *prices.last().expect("prices is non-empty");
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
    if downtrend && is_breakdown_below_recent_low(prices, BREAKOUT_LOOKBACK) && price_below_both {
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
    if uptrend && is_breakout_above_recent_high(prices, 5) && price_above_both {
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
