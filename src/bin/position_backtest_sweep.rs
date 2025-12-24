use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use trade_signal::backtest::{
    find_best_strategy, generate_backtest_sweep_jobs, generate_pullback_pairs, generate_strategies,
};

use trade_signal::backtest::position::{PositionBacktester, buy_and_hold_equity, print_summary};
use trade_signal::data::{get_samples_from_input_file, resample_to_n_hours};

#[derive(Debug, Parser)]
struct Args {
    /// config-file path
    #[arg(long)]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    /// Path to the CSV file (timestamp,price)pub
    input: PathBuf,

    /// Resample input to <sample_hours> hours (i.e. 1h, 4h, 6h, ...)
    sample_hours: i64,

    /// Initial cash for the backtest
    initial_cash: f64,

    /// Min breakout lookback window (e.g. 3)
    min_lookback: usize,

    /// Max breakout lookback window (e.g. 10)
    max_lookback: usize,

    /// Min pullback tolerances (e.g. 0.001)
    min_pullback_pct: f64,

    /// Max pullback tolerances (e.g. 0.01)
    max_pullback_pct: f64,

    /// Maximum fraction for buy/sell (e.g. 0.5 = at most 50%)
    max_buy_sell_fraction: f64,

    /// Number of steps for buy/sell fraction (0–1).
    /// E.g. 100 => 0.01, 0.02, ..., 1.00
    buy_sell_frac_steps: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = args
        .config
        .into_os_string()
        .into_string()
        .expect("Failed to translate config file path into string");
    let config: Config = config::Config::builder()
        .add_source(config::File::with_name(&config_path))
        .build()?
        .try_deserialize()?;

    let samples = get_samples_from_input_file(&config.input)
        .with_context(|| format!("failed to load samples from {:?}", config.input))?;

    if samples.is_empty() {
        println!("No data found in CSV.");
        return Ok(());
    }

    let resampled = resample_to_n_hours(&samples, config.sample_hours);

    println!(
        "Loaded {} raw points, {} {}h-candles after resampling.",
        samples.len(),
        resampled.len(),
        config.sample_hours,
    );

    let pullback_pairs =
        generate_pullback_pairs(config.min_pullback_pct, config.max_pullback_pct, 0.001);

    let strategies = generate_strategies(config.min_lookback, config.max_lookback, pullback_pairs);

    let buy_sell_frac_steps = config.buy_sell_frac_steps;

    let jobs = generate_backtest_sweep_jobs(strategies, buy_sell_frac_steps);

    let best = find_best_strategy(
        jobs,
        config.max_buy_sell_fraction,
        buy_sell_frac_steps,
        &samples,
        || PositionBacktester::new(config.initial_cash),
    );

    println!();
    if let Some((candidate, result)) = best {
        println!("=== Best configuration ===");
        println!(
            "strategy:          {}",
            candidate.strategy.describe_config()
        );
        println!("buy_fraction:      {:.2}", candidate.buy_sell_fraction);
        println!();
        print_summary(&result);

        if let Some(hold_equity) = buy_and_hold_equity(&samples, result.initial_equity) {
            println!();
            println!("Buy & hold final equity: {:.2}", hold_equity);
        }
    } else {
        println!("No valid backtest result produced.");
    }

    Ok(())
}
