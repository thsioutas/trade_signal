use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::Parser;
use rayon::prelude::*;

use sma_analyzer::{
    backtest::{BacktestConfig, BacktestResult, buy_and_hold_equity, print_summary, run_backtest},
    data::{get_samples_from_input_file, resample_to_hourly},
    indicators::sma::SmaConfig,
    signal::{BreakoutConfig, StrategyConfig},
};

const EPS: f64 = 1e-9;

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

    /// Whether ATR gate filter should be used
    #[arg(long, default_value_t = false)]
    atr_enabled: bool,

    /// Whether regime filter should be used
    #[arg(long, default_value_t = false)]
    regime_enabled: bool,
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

    let strategies = generate_strategies(args.min_lookback, args.max_lookback);

    let steps = args.frac_steps; // e.g. 50 => 0.01 .. 0.50

    let jobs: Vec<_> = strategies
        .iter()
        .flat_map(|&strategy| (1..=steps).map(move |step| (strategy, step)))
        .collect();

    let total_iters = jobs.len() as u64;
    let done = AtomicU64::new(0);
    let progress_every = (total_iters / 100).max(1) as u64;

    println!(
        "Running parameter sweep... ({} total combinations)",
        total_iters
    );

    let mut best_cfg: Option<BacktestConfig> = None;
    let mut best_result: Option<BacktestResult> = None;

    let best_pair: Option<(BacktestConfig, BacktestResult)> = jobs
        .into_par_iter()
        .filter_map(|(strategy, step)| {
            let current = done.fetch_add(1, Ordering::Relaxed) + 1;
            if progress_every != 0
                && (current.is_multiple_of(progress_every) || current == total_iters)
            {
                let pct = (current as f64 / total_iters as f64) * 100.0;
                println!("Progress: {:6.2}% ({}/{})", pct, current, total_iters);
            }
            let frac = (step as f64 / steps as f64) * args.max_fraction;
            let cfg = BacktestConfig {
                initial_cash: args.initial_cash,
                initial_coin: args.initial_coin,
                fee_bps: args.fee_bps,
                buy_fraction: frac,
                sell_fraction: frac,
                atr_enabled: args.atr_enabled,
                regime_enabled: args.regime_enabled,
                strategy,
            };
            let result = run_backtest(&hourly, &cfg)?;
            Some((cfg, result))
        })
        .reduce_with(|(cfg_a, res_a), (cfg_b, res_b)| {
            let a_ret = res_a.total_return_pct;
            let b_ret = res_b.total_return_pct;
            let a_dd = res_a.max_drawdown_pct;
            let b_dd = res_b.max_drawdown_pct;

            // "Better" = higher total return, tie-break by lower drawdown
            let pick_b = if b_ret > a_ret + EPS {
                true
            } else if (b_ret - a_ret).abs() < EPS {
                b_dd < a_dd
            } else {
                false
            };

            if pick_b {
                (cfg_b, res_b)
            } else {
                (cfg_a, res_a)
            }
        });

    if let Some((cfg, result)) = best_pair {
        best_cfg = Some(cfg);
        best_result = Some(result);
    }

    println!();
    if let (Some(cfg), Some(result)) = (best_cfg, best_result) {
        println!("=== Best configuration ===");
        println!("strategy:          {}", cfg.strategy.describe_config());
        println!("buy_fraction:      {:.2}", cfg.buy_fraction);
        println!("sell_fraction:     {:.2}", cfg.sell_fraction);
        println!("fee_bps:           {:.2}", cfg.fee_bps);
        println!("ATR enabled:       {}", cfg.atr_enabled);
        println!();
        print_summary(&result);

        if let Some(hold_equity) = buy_and_hold_equity(&hourly, cfg.initial_cash, cfg.initial_coin)
        {
            println!();
            println!("Buy & hold final equity: {:.2}", hold_equity);
        }
    } else {
        println!("No valid backtest result produced.");
    }
}

fn generate_strategies(min_lookback: usize, max_lookback: usize) -> Vec<StrategyConfig> {
    let mut strategies = Vec::new();

    let short_candidates = [10, 20, 30];
    let long_candidates = [40, 60, 80, 100];

    let sma_configs: Vec<SmaConfig> = short_candidates
        .iter()
        .flat_map(|&short| {
            long_candidates.iter().filter_map(move |&long| {
                if long >= short * 2 {
                    Some(SmaConfig {
                        short_window: short,
                        long_window: long,
                    })
                } else {
                    None
                }
            })
        })
        .collect();
    for sma_config in sma_configs {
        for lookback in min_lookback..=max_lookback {
            // bit 0: breakouts
            // bit 1: pullbacks
            // bit 2: crossovers
            // bit 3: bias_only
            for mask in 0u8..16 {
                let enable_breakouts = (mask & 0b0001) != 0;
                let enable_pullbacks = (mask & 0b0010) != 0;
                let enable_crossovers = (mask & 0b0100) != 0;
                let enable_bias_only = (mask & 0b1000) != 0;

                // Skip the totally empty strategy (nothing enabled).
                if !enable_breakouts && !enable_pullbacks && !enable_crossovers && !enable_bias_only
                {
                    continue;
                }

                let strategy = StrategyConfig {
                    breakouts: if enable_breakouts {
                        Some(BreakoutConfig {
                            breakout_lookback: lookback,
                        })
                    } else {
                        None
                    },
                    enable_pullbacks,
                    enable_crossovers,
                    enable_bias_only,
                    sma_config,
                };

                strategies.push(strategy);
            }
        }
    }

    strategies
}
