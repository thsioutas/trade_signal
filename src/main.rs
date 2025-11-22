use anyhow::{Context, Result};
use clap::Parser;
use csv::ReaderBuilder;
use serde::Deserialize;
use std::fs::File;
use std::path::PathBuf;

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

fn main() -> Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input)
        .with_context(|| format!("failed to open input file: {:?}", args.input))?;

    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut prices: Vec<f64> = Vec::new();
    let mut last_timestamp: Option<String> = None;

    for result in rdr.deserialize::<PriceRow>() {
        let row = result.with_context(|| "failed to deserialize CSV row")?;
        last_timestamp = Some(row.timestamp);
        prices.push(row.price);
    }

    if prices.is_empty() {
        println!("No data found in CSV.");
        return Ok(());
    }

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

    let last_price = *prices.last().unwrap();
    let ts = last_timestamp.unwrap_or_else(|| "<unknown>".to_string());

    // Current SMAs (using all prices)
    let sma20 = simple_moving_average(&prices, 20).expect("we checked len >= 51");
    let sma50 = simple_moving_average(&prices, 50).expect("we checked len >= 51");

    // Previous SMAs (using all but the last price -> "previous candle")
    let prev_slice = &prices[..prices.len() - 1];
    let prev_sma20 = simple_moving_average(prev_slice, 20).expect("len-1 >= 50");
    let prev_sma50 = simple_moving_average(prev_slice, 50).expect("len-1 >= 50");

    println!("Last timestamp: {}", ts);
    println!("Last price:     {:.4}", last_price);
    println!("SMA(20):        {:.4}", sma20);
    println!("SMA(50):        {:.4}", sma50);
    println!("Prev SMA(20):   {:.4}", prev_sma20);
    println!("Prev SMA(50):   {:.4}", prev_sma50);

    let (suggestion, detail) = suggest_action(last_price, sma20, sma50, prev_sma20, prev_sma50);

    println!("Suggestion:     {}", suggestion);
    if let Some(detail) = detail {
        println!("Reason:         {}", detail);
    }

    Ok(())
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

/// Advanced trading rule based on:
/// - Golden Cross / Death Cross detection (using previous + current SMAs)
/// - Trend filter using SMA50 slope
/// - Price confirmation (price relative to SMA20 & SMA50)
///
/// Returns (short_suggestion, optional_detailed_reason)
fn suggest_action(
    last_price: f64,
    sma20: f64,
    sma50: f64,
    prev_sma20: f64,
    prev_sma50: f64,
) -> (&'static str, Option<&'static str>) {
    // Detect fresh crossovers
    let golden_cross = prev_sma20 <= prev_sma50 && sma20 > sma50;
    let death_cross = prev_sma20 >= prev_sma50 && sma20 < sma50;

    // Trend filter: is SMA50 sloping up or down?
    let sma50_up = sma50 > prev_sma50;
    let sma50_down = sma50 < prev_sma50;

    // Price confirmation: where is price relative to the MAs?
    let price_above_both = last_price > sma20 && last_price > sma50;
    let price_below_both = last_price < sma20 && last_price < sma50;

    // 1. Strong BUY: fresh Golden Cross in an uptrend with price confirmation
    if golden_cross && sma50_up && price_above_both {
        return (
            "BUY",
            Some("Golden Cross + SMA50 rising + price above SMA20 & SMA50"),
        );
    }

    // 2. Strong SELL: fresh Death Cross in a downtrend with price confirmation
    if death_cross && sma50_down && price_below_both {
        return (
            "SELL",
            Some("Death Cross + SMA50 falling + price below SMA20 & SMA50"),
        );
    }

    // 3. No fresh cross but we are clearly in an uptrend
    if sma20 > sma50 && price_above_both {
        return (
            "HOLD / LONG BIAS",
            Some("Uptrend (SMA20 > SMA50) and price above both averages"),
        );
    }

    // 4. No fresh cross but we are clearly in a downtrend
    if sma20 < sma50 && price_below_both {
        return (
            "HOLD / SHORT BIAS",
            Some("Downtrend (SMA20 < SMA50) and price below both averages"),
        );
    }

    // 5. Otherwise, no clear edge
    ("HOLD", Some("No clear crossover or conflicting signals"))
}
