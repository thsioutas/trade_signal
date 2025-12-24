use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

use trade_signal::{
    backtest::{
        find_best_strategy, generate_backtest_sweep_jobs, generate_pullback_pairs,
        generate_strategies,
        spot::{SpotBacktester, buy_and_hold_equity, print_summary},
    },
    data::{get_samples_from_input_file, resample_to_hourly},
};

#[derive(Debug, Parser)]
struct Args {
    /// config-file path
    #[arg(long)]
    config: PathBuf,
}

/// Sweep over backtest parameters (i.e. lookback, buy/sell fractions)
/// and report the best configuration.
#[derive(Deserialize)]
struct Config {
    /// Path to CSV with raw timestamp,price data
    input: PathBuf,

    /// Initial cash for the backtest
    initial_cash: f64,

    /// Initial coin holdings (e.g. if you already own some SOL)
    initial_coin: f64,

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

    /// Trading fee in basis points (e.g. 10 = 0.10%)
    fee_bps: f64,
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

    let samples = get_samples_from_input_file(&config.input).expect("failed to load input CSV");
    let hourly = resample_to_hourly(&samples);

    println!(
        "Loaded {} raw samples -> {} hourly candles",
        samples.len(),
        hourly.len()
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
        &hourly,
        || SpotBacktester::new(config.initial_cash, config.initial_coin, config.fee_bps),
    );

    println!();
    if let Some((candidate, result)) = best {
        println!("=== Best configuration ===");
        println!(
            "strategy:          {}",
            candidate.strategy.describe_config()
        );
        println!("buy_fraction:      {:.2}", candidate.buy_sell_fraction);
        println!("sell_fraction:     {:.2}", candidate.buy_sell_fraction);
        println!("fee_bps:           {:.2}", config.fee_bps);
        println!();
        print_summary(&result);

        if let Some(hold_equity) =
            buy_and_hold_equity(&hourly, config.initial_cash, config.initial_coin)
        {
            println!();
            println!("Buy & hold final equity: {:.2}", hold_equity);
        }
    } else {
        println!("No valid backtest result produced.");
    }
    Ok(())
}
