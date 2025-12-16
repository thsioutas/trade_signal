use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use clap::Parser;
use rayon::prelude::*;
use serde::Deserialize;

use trade_signal::{
    backtest::spot::{
        BacktestConfig, SpotBacktestResult, buy_and_hold_equity, print_summary, run_backtest,
    },
    data::{get_samples_from_input_file, resample_to_hourly},
    indicators::sma::SmaConfig,
    signal::{BreakoutConfig, FilterConfig, PullbackConfig, StrategyConfig},
};

const EPS: f64 = 1e-9;

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
    max_fraction: f64,

    /// Number of steps for buy/sell fraction (0–1).
    /// E.g. 100 => 0.01, 0.02, ..., 1.00
    frac_steps: usize,

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

    let steps = config.frac_steps;

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
    let mut best_result: Option<SpotBacktestResult> = None;

    let best_pair: Option<(BacktestConfig, SpotBacktestResult)> = jobs
        .into_par_iter()
        .filter_map(|(strategy, step)| {
            let current = done.fetch_add(1, Ordering::Relaxed) + 1;
            if progress_every != 0
                && (current.is_multiple_of(progress_every) || current == total_iters)
            {
                let pct = (current as f64 / total_iters as f64) * 100.0;
                println!("Progress: {:6.2}% ({}/{})", pct, current, total_iters);
            }
            let frac = (step as f64 / steps as f64) * config.max_fraction;
            let cfg = BacktestConfig {
                initial_cash: config.initial_cash,
                initial_coin: config.initial_coin,
                fee_bps: config.fee_bps,
                buy_fraction: frac,
                sell_fraction: frac,
                strategy,
            };
            let result = run_backtest(&hourly, &cfg)
                .inspect_err(|err| println!("Failed to get backtest result: {}", err))
                .ok()?;
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
        println!("ATR enabled:       {}", cfg.strategy.filters.atr.is_some());
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
    Ok(())
}

fn generate_strategies(
    min_lookback: usize,
    max_lookback: usize,
    pullback_pairs: Vec<(f64, f64)>,
) -> Vec<StrategyConfig> {
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
        // bit 0: breakouts
        // bit 1: pullbacks
        // bit 2: crossovers
        // bit 3: bias_only
        for mask in 0u8..16 {
            let enable_breakouts = (mask & 0b0001) != 0;
            let enable_pullbacks = (mask & 0b0010) != 0;
            let enable_crossovers = (mask & 0b0100) != 0;
            let enable_bias_only = (mask & 0b1000) != 0;

            match (enable_breakouts, enable_pullbacks) {
                (true, true) => {
                    for lookback in min_lookback..=max_lookback {
                        for (pullback_bounce_tol, pullback_rejection_tol) in &pullback_pairs {
                            let strategy = StrategyConfig {
                                breakouts: Some(BreakoutConfig {
                                    breakout_lookback: lookback,
                                }),
                                pullbacks: Some(PullbackConfig {
                                    bounce_tolerance_pct: *pullback_bounce_tol,
                                    reject_tolerance_pct: *pullback_rejection_tol,
                                }),
                                enable_crossovers,
                                enable_bias_only,
                                sma_config,
                                filters: FilterConfig {
                                    atr: None,
                                    regime: None,
                                    require_price_confirmation: true,
                                    require_trend_filter: true,
                                },
                            };

                            strategies.push(strategy);
                        }
                    }
                }
                (true, false) => {
                    for lookback in min_lookback..=max_lookback {
                        let strategy = StrategyConfig {
                            breakouts: Some(BreakoutConfig {
                                breakout_lookback: lookback,
                            }),
                            pullbacks: None,
                            enable_crossovers,
                            enable_bias_only,
                            sma_config,
                            filters: FilterConfig {
                                atr: None,
                                regime: None,
                                require_price_confirmation: true,
                                require_trend_filter: true,
                            },
                        };

                        strategies.push(strategy);
                    }
                }
                (false, true) => {
                    for (pullback_bounce_tol, pullback_rejection_tol) in &pullback_pairs {
                        let strategy = StrategyConfig {
                            breakouts: None,
                            pullbacks: Some(PullbackConfig {
                                bounce_tolerance_pct: *pullback_bounce_tol,
                                reject_tolerance_pct: *pullback_rejection_tol,
                            }),
                            enable_crossovers,
                            enable_bias_only,
                            sma_config,
                            filters: FilterConfig {
                                atr: None,
                                regime: None,
                                require_price_confirmation: true,
                                require_trend_filter: true,
                            },
                        };

                        strategies.push(strategy);
                    }
                }
                (false, false) => {
                    // Skip the totally empty strategy (nothing enabled).
                    if !enable_breakouts
                        && !enable_pullbacks
                        && !enable_crossovers
                        && !enable_bias_only
                    {
                        continue;
                    }
                    let strategy = StrategyConfig {
                        breakouts: None,
                        pullbacks: None,
                        enable_crossovers,
                        enable_bias_only,
                        sma_config,
                        filters: FilterConfig {
                            atr: None,
                            regime: None,
                            require_price_confirmation: true,
                            require_trend_filter: true,
                        },
                    };

                    strategies.push(strategy);
                }
            }
        }
    }

    strategies
}

fn generate_pullback_pairs(min: f64, max: f64, step: f64) -> Vec<(f64, f64)> {
    let mut pairs = Vec::new();
    let mut bounce = min;
    while bounce <= max {
        let mut reject = bounce + step;
        while reject <= max {
            pairs.push((bounce, reject));
            reject += step;
        }
        bounce += step;
    }
    pairs
}
