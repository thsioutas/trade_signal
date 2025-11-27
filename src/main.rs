use anyhow::{Context, Result};
use chrono::{DateTime, Timelike, Utc};
use clap::Parser;
use csv::ReaderBuilder;
use serde::Deserialize;

use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;

const BREAKOUT_LOOKBACK: usize = 5;

#[derive(Debug, Parser)]
struct Args {
    /// Path to the CSV file (timestamp,price)
    #[arg(long)]
    input: PathBuf,
}

#[derive(Debug, Deserialize)]
struct PriceRow {
    timestamp: String,
    price: f64,
}

#[derive(Debug, Clone)]
struct Sample {
    ts: DateTime<Utc>,
    price: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input)
        .with_context(|| format!("failed to open input file: {:?}", args.input))?;

    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut samples: Vec<Sample> = Vec::new();

    for result in rdr.deserialize::<PriceRow>() {
        let row: PriceRow = result.with_context(|| "failed to deserialize CSV row")?;
        let ts = DateTime::parse_from_rfc3339(&row.timestamp)
            .with_context(|| format!("failed to parse timestamp: {}", row.timestamp))?
            .with_timezone(&Utc);
        samples.push(Sample {
            ts,
            price: row.price,
        });
    }

    if samples.is_empty() {
        println!("No data found in CSV.");
        return Ok(());
    }

    // Resample to hourly closes
    let hourly = resample_to_hourly(&samples);

    println!(
        "Loaded {} raw points, {} hourly candles after resampling.",
        samples.len(),
        hourly.len()
    );

    if hourly.is_empty() {
        println!("No hourly data after resampling.");
        return Ok(());
    }

    // Use hourly prices for SMA logic
    let prices: Vec<f64> = hourly.iter().map(|s| s.price).collect();

    // For a proper crossover signal we need:
    // - current SMA50 (based on all data)
    // - previous SMA50 (based on all but last point)
    //
    // So we need at least 51 prices:
    //  - 50 for previous SMA50
    //  - 50 for current SMA50 (including the last price)
    if prices.len() < 51 {
        println!(
            "Not enough data: need at least 51 prices for SMA20/50 crossover logic, got {}.",
            prices.len()
        );
        return Ok(());
    }

    let last = hourly.last().unwrap();
    let last_price = last.price;
    let ts = last.ts.to_rfc3339();

    // Current SMAs (using all prices)
    let sma20 = simple_moving_average(&prices, 20).expect("we checked len >= 51");
    let sma50 = simple_moving_average(&prices, 50).expect("we checked len >= 51");

    // Previous SMAs (using all but the last price -> "previous candle")
    let prev_slice = &prices[..prices.len() - 1];
    let prev_sma20 = simple_moving_average(prev_slice, 20).expect("len-1 >= 50");
    let prev_sma50 = simple_moving_average(prev_slice, 50).expect("len-1 >= 50");

    println!("Last (hourly) timestamp: {}", ts);
    println!("Last (hourly) price:     {:.4}", last_price);
    println!("SMA(20):        {:.4}", sma20);
    println!("SMA(50):        {:.4}", sma50);
    println!("Prev SMA(20):   {:.4}", prev_sma20);
    println!("Prev SMA(50):   {:.4}", prev_sma50);

    let (suggestion, detail) = suggest_action(&prices, sma20, sma50, prev_sma20, prev_sma50);

    println!("Suggestion:     {}", suggestion);
    if let Some(detail) = detail {
        println!("Reason:         {}", detail);
    }

    Ok(())
}

/// Resample raw samples (possibly 5m + 1h mixed) into 1-hour "closes".
/// For each hour bucket, we keep the *last* price observed in that hour.
fn resample_to_hourly(samples: &[Sample]) -> Vec<Sample> {
    let mut buckets: BTreeMap<DateTime<Utc>, Sample> = BTreeMap::new();

    for s in samples {
        // Truncate to the start of the hour for the bucket key
        let hour_start =
            s.ts.with_minute(0)
                .and_then(|dt| dt.with_second(0))
                .and_then(|dt| dt.with_nanosecond(0))
                .expect("valid hour truncation");

        // Because we iterate in chronological order,
        // inserting again for the same hour will overwrite with the *latest* sample.
        buckets.insert(
            hour_start,
            Sample {
                ts: s.ts,       // we keep the original timestamp of the last tick in this hour
                price: s.price, // close price
            },
        );
    }

    buckets.into_values().collect()
}

/// Compute the simple moving average over the last `window` values.
/// Returns None if there isn't enough data.
fn simple_moving_average(prices: &[f64], window: usize) -> Option<f64> {
    if prices.len() < window {
        return None;
    }

    let start = prices.len() - window;
    let slice = &prices[start..];
    let sum: f64 = slice.iter().copied().sum();
    Some(sum / window as f64)
}

/// Check if we have a breakdown below a recent low.
///
/// - Lookback N (e.g. 5) means:
///   Use the last N-1 candles *before* the current one
///   and see if the last price < min of those lows.
fn is_breakdown_below_recent_low(prices: &[f64], lookback: usize) -> bool {
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

/// Check if we have a pullback up to SMA20 and rejection down:
///
/// Pattern over last 3 closes:
/// - p2 (2 candles ago) < sma20
/// - p1 > p2 and near/above sma20
/// - p0 < sma20 and p0 < p1
fn is_pullback_to_sma20_and_reject_down(prices: &[f64], sma20: f64) -> bool {
    if prices.len() < 3 {
        return false;
    }

    let n = prices.len();
    let p2 = prices[n - 3];
    let p1 = prices[n - 2];
    let p0 = prices[n - 1];

    let tol = 0.003; // 0.3% below SMA20 considered "touching" from below

    let was_below = p2 < sma20;
    let pulled_back_near = p1 > p2 && p1 >= sma20 * (1.0 - tol); // close to or slightly above SMA20
    let rejected = p0 < sma20 && p0 < p1;

    was_below && pulled_back_near && rejected
}

/// Check if we have a breakout above a recent high.
///
/// - Lookback N (e.g. 5) means:
///   Use the last N-1 candles *before* the current one
///   and see if the last price > max of those highs.
fn is_breakout_above_recent_high(prices: &[f64], lookback: usize) -> bool {
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

/// Check if we have a pullback to SMA20 and bounce:
///
/// Pattern over last 3 closes:
/// - p2 (2 candles ago) > sma20
/// - p1 < p2 and near/under sma20
/// - p0 > sma20 and p0 > p1
fn is_pullback_to_sma20_and_bounce(prices: &[f64], sma20: f64) -> bool {
    if prices.len() < 3 {
        return false;
    }

    let n = prices.len();
    let p2 = prices[n - 3];
    let p1 = prices[n - 2];
    let p0 = prices[n - 1];

    let tol = 0.003; // 0.3% above SMA20 considered "touching"

    let was_above = p2 > sma20;
    let pulled_back_near = p1 < p2 && p1 <= sma20 * (1.0 + tol); // can be slightly above or below SMA20
    let bounced = p0 > sma20 && p0 > p1;

    was_above && pulled_back_near && bounced
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
fn suggest_action(
    prices: &[f64],
    sma20: f64,
    sma50: f64,
    prev_sma20: f64,
    prev_sma50: f64,
) -> (&'static str, Option<&'static str>) {
    let last_price = *prices.last().unwrap();

    // Trend and slope filters
    // We combined two separate signals:
    // * Trend direction (SMA20 > SMA50 or SMA20 < SMA50)
    // * Trend slope (SMA50 rising or falling)
    // We did this because:
    // * Using only SMA50 slope is not enough to define a strong trend.
    // * Using only SMA20 > SMA50 is not safe without confirming SMA50 is rising.
    // * Combining them is a stronger, more reliable trend filter.
    let uptrend = sma20 > sma50 && sma50 >= prev_sma50;
    let downtrend = sma20 < sma50 && sma50 <= prev_sma50;

    // Detect fresh crossovers
    let golden_cross = prev_sma20 <= prev_sma50 && sma20 > sma50;
    let death_cross = prev_sma20 >= prev_sma50 && sma20 < sma50;

    // Price confirmation: where is price relative to the MAs?
    let price_above_both = last_price > sma20 && last_price > sma50;
    let price_below_both = last_price < sma20 && last_price < sma50;

    // ~~~~ SELL patterns ~~~~

    // 1. Breakdown below a recent low in a downtrend
    if downtrend && is_breakdown_below_recent_low(prices, BREAKOUT_LOOKBACK) && price_below_both {
        return (
            "SELL",
            Some("Breakdown below recent low in downtrend (SMA20 < SMA50)"),
        );
    }

    // 2. Pullback up to SMA20 + rejection in a downtrend
    if downtrend && is_pullback_to_sma20_and_reject_down(prices, sma20) {
        return (
            "SELL",
            Some("Pullback up to SMA20 and rejection in downtrend"),
        );
    }

    // ~~~~ BUY patterns ~~~~

    // 3. Breakout above a recent high in an uptrend
    if uptrend && is_breakout_above_recent_high(prices, 5) && price_above_both {
        return (
            "BUY",
            Some("Breakout above recent high in uptrend (SMA20 > SMA50)"),
        );
    }

    // 4. Pullback to SMA20 + bounce in an uptrend
    if uptrend && is_pullback_to_sma20_and_bounce(prices, sma20) {
        return ("BUY", Some("Pullback to SMA20 and bounce in uptrend"));
    }

    // ~~~~ Crossovers ~~~~

    // 5. Strong BUY: fresh Golden Cross in an uptrend with price confirmation
    if golden_cross && uptrend && price_above_both {
        return (
            "BUY",
            Some("Golden Cross + SMA50 rising + price above SMA20 & SMA50"),
        );
    }

    // 6. Strong SELL: fresh Death Cross in a downtrend with price confirmation
    if death_cross && downtrend && price_below_both {
        return (
            "SELL",
            Some("Death Cross + SMA50 falling + price below SMA20 & SMA50"),
        );
    }

    // ~~~~ Bias-only ~~~~

    // 7. No fresh cross but we are clearly in an uptrend
    if sma20 > sma50 && price_above_both {
        return (
            "HOLD / LONG BIAS",
            Some("Uptrend (SMA20 > SMA50) and price above both averages"),
        );
    }

    // 8. No fresh cross but we are clearly in a downtrend
    if sma20 < sma50 && price_below_both {
        return (
            "HOLD / SHORT BIAS",
            Some("Downtrend (SMA20 < SMA50) and price below both averages"),
        );
    }

    // 5. Otherwise, no clear edge
    (
        "HOLD",
        Some("No clear breakout, pullback bounce/rejection, or crossover signal"),
    )
}
