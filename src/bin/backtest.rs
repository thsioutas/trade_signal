use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use sma_analyzer::backtest::{BacktestConfig, print_summary, run_backtest};
use sma_analyzer::data::{get_samples_from_input_file, resample_to_hourly};

#[derive(Debug, Parser)]
struct Args {
    /// Path to the CSV file (timestamp,price)pub
    #[arg(long)]
    input: PathBuf,

    /// Initial cash for the backtest
    #[arg(long, default_value_t = 10_000.0)]
    initial_cash: f64,

    /// Coins you already hold at the first candle
    #[arg(long, default_value_t = 0.0)]
    initial_coin: f64,

    /// Fee in basis points per trade side (e.g. 10 = 0.10%)
    #[arg(long, default_value_t = 10.0)]
    fee_bps: f64,

    /// Fraction of *available cash* to allocate on each BUY signal (0.0–1.0)
    #[arg(long, default_value_t = 0.01)]
    buy_fraction: f64,

    /// Fraction of *current position* to sell on each SELL signal (0.0–1.0)
    #[arg(long, default_value_t = 0.01)]
    sell_fraction: f64,

    /// Whether ATR gate filter should be used
    #[arg(long, default_value_t = false)]
    atr_enabled: bool,

    /// Whether regime filter should be used
    #[arg(long, default_value_t = false)]
    regime_enabled: bool,

    /// How many candles to lookback for a brekdown
    #[arg(long, default_value_t = 5)]
    breakout_lookback: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let samples = get_samples_from_input_file(&args.input)
        .with_context(|| format!("failed to load samples from {:?}", args.input))?;

    if samples.is_empty() {
        println!("No data found in CSV.");
        return Ok(());
    }

    let hourly = resample_to_hourly(&samples);

    println!(
        "Loaded {} raw points, {} hourly candles after resampling.",
        samples.len(),
        hourly.len()
    );

    let cfg = BacktestConfig {
        initial_cash: args.initial_cash,
        initial_coin: args.initial_coin,
        fee_bps: args.fee_bps,
        buy_fraction: args.buy_fraction,
        sell_fraction: args.sell_fraction,
        atr_enabled: args.atr_enabled,
        regime_enabled: args.regime_enabled,
        breakout_lookback: args.breakout_lookback,
    };

    let Some(result) = run_backtest(&hourly, &cfg) else {
        println!(
            "Not enough hourly data: need at least 51 candles, got {}.",
            hourly.len()
        );
        return Ok(());
    };

    print_summary(&result);

    Ok(())
}
