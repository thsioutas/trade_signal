use std::path::PathBuf;

use clap::Parser;

use sma_analyzer::{
    backtest::{BacktestConfig, BacktestResult, print_summary, run_backtest},
    data::{get_samples_from_input_file, resample_to_hourly},
};

/// Sweep over backtest parameters (i.e. lookback, buy/sell fractions)
/// and report the best configuration.
#[derive(Debug, Parser)]
struct Args {
    /// Path to CSV with raw timestamp,price data
    #[arg(long)]
    input: PathBuf,

    /// Initial cash for the backtest
    #[arg(long, default_value_t = 1000.0)]
    initial_cash: f64,

    /// Initial coin holdings (e.g. if you already own some SOL)
    #[arg(long, default_value_t = 0.0)]
    initial_coin: f64,

    /// Min breakout lookback window (e.g. 3)
    #[arg(long, default_value_t = 3)]
    min_lookback: usize,

    /// Max breakout lookback window (e.g. 10)
    #[arg(long, default_value_t = 10)]
    max_lookback: usize,

    /// Maximum fraction for buy/sell (e.g. 0.5 = at most 50%)
    #[arg(long, default_value_t = 0.5)]
    max_fraction: f64,

    /// Number of steps for buy/sell fraction (0–1).
    /// E.g. 100 => 0.01, 0.02, ..., 1.00
    #[arg(long, default_value_t = 100)]
    frac_steps: usize,

    /// Trading fee in basis points (e.g. 10 = 0.10%)
    #[arg(long, default_value_t = 10.0)]
    fee_bps: f64,

    /// Enable ATR filter? (for now defaults to false)
    #[arg(long, default_value_t = false)]
    atr_enabled: bool,
}

fn main() {
    let args = Args::parse();

    let samples = get_samples_from_input_file(&args.input).expect("failed to load input CSV");
    let hourly = resample_to_hourly(&samples);

    println!(
        "Loaded {} raw samples -> {} hourly candles",
        samples.len(),
        hourly.len()
    );

    if hourly.len() < 51 {
        eprintln!("Not enough data for SMA20/50 logic (need >= 51 candles).");
        return;
    }

    let mut best_cfg: Option<BacktestConfig> = None;
    let mut best_result: Option<BacktestResult> = None;

    println!("Running parameter sweep...");
    println!("lookback  frac  return%   dd%   trades");

    let steps = args.frac_steps; // e.g. 50 => 0.01 .. 0.50

    for lookback in args.min_lookback..=args.max_lookback {
        for step in 1..=args.frac_steps {
            let frac = (step as f64 / steps as f64) * args.max_fraction;

            let cfg = BacktestConfig {
                initial_cash: args.initial_cash,
                initial_coin: args.initial_coin,
                fee_bps: args.fee_bps,
                buy_fraction: frac,
                sell_fraction: frac,
                atr_enabled: args.atr_enabled,
                breakout_lookback: lookback,
            };

            let Some(result) = run_backtest(&hourly, &cfg) else {
                continue;
            };

            let ret_pct = result.total_return_pct * 100.0;
            let dd_pct = result.max_drawdown_pct * 100.0;
            let trades = result.trades.len();

            // Compact per-config line
            println!(
                "{:>7} {:5.2} {:8.2} {:6.2} {:7}",
                lookback, frac, ret_pct, dd_pct, trades
            );

            // Update "best" by:
            // 1) higher total return
            // 2) if equal (within tiny epsilon), pick lower drawdown
            let is_better = match &best_result {
                None => true,
                Some(best) => {
                    if ret_pct > best.total_return_pct * 100.0 + 1e-9 {
                        true
                    } else if (ret_pct - best.total_return_pct * 100.0).abs() < 1e-9 {
                        dd_pct < best.max_drawdown_pct * 100.0
                    } else {
                        false
                    }
                }
            };

            if is_better {
                best_cfg = Some(cfg.clone());
                best_result = Some(result);
            }
        }
    }

    println!();
    if let (Some(cfg), Some(result)) = (best_cfg, best_result) {
        println!("=== Best configuration ===");
        println!("breakout_lookback: {}", cfg.breakout_lookback);
        println!("buy_fraction:      {:.2}", cfg.buy_fraction);
        println!("sell_fraction:     {:.2}", cfg.sell_fraction);
        println!("fee_bps:           {:.2}", cfg.fee_bps);
        println!("ATR enabled:       {}", cfg.atr_enabled);
        println!();
        print_summary(&result);
    } else {
        println!("No valid backtest result produced.");
    }
}
