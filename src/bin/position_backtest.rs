use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use trade_signal::indicators::sma::SmaConfig;
use trade_signal::indicators::{AtrFilter, RegimeFilter};
use trade_signal::signal::{BreakoutConfig, FilterConfig, PullbackConfig, StrategyConfig};

use trade_signal::backtest::position::{
    BacktestConfig, buy_and_hold_equity, print_summary, run_backtest,
};
use trade_signal::data::{get_samples_from_input_file, resample_to_hourly};

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

    /// Initial cash for the backtest
    initial_cash: f64,

    /// Fraction of *available cash* to allocate on each position (0.0–1.0)
    buy_fraction: f64,

    /// Whether ATR gate filter should be used
    atr_enabled: bool,

    /// Whether regime filter should be used
    regime_enabled: bool,

    /// How many candles to lookback for a breakdown
    /// Do not set to not use breakout patterns
    breakout_lookback: Option<usize>,

    /// Do not set to not use pullback patterns
    pullback_bounce_tolerance_pct: Option<f64>,

    /// Do not set to not use pullback patterns
    pullback_rejection_tolerance_pct: Option<f64>,

    /// Whether sma crossover signals should be used
    enable_crossovers: bool,

    /// Whether bias_only signals should be used
    enable_bias_only: bool,

    /// SMA short window
    sma_short_window: usize,

    /// SMA long window
    sma_long_window: usize,

    /// Whether price confirmation is required
    require_price_confirmation: bool,

    /// Whether trend filter is required
    require_trend_filter: bool,
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

    let hourly = resample_to_hourly(&samples);

    println!(
        "Loaded {} raw points, {} hourly candles after resampling.",
        samples.len(),
        hourly.len()
    );

    let pullbacks = match (
        config.pullback_bounce_tolerance_pct,
        config.pullback_rejection_tolerance_pct,
    ) {
        (Some(bounce_tolerance_pct), Some(reject_tolerance_pct)) => Some(PullbackConfig {
            bounce_tolerance_pct,
            reject_tolerance_pct,
        }),
        (None, None) => None,
        (Some(v), None) => {
            println!("Using given bounce_tolerance_pct as reject_tolerance_pct");
            Some(PullbackConfig {
                bounce_tolerance_pct: v,
                reject_tolerance_pct: v,
            })
        }
        (None, Some(v)) => {
            println!("Using given reject_tolerance_pct as bounce_tolerance_pct");
            Some(PullbackConfig {
                bounce_tolerance_pct: v,
                reject_tolerance_pct: v,
            })
        }
    };

    let strategy = StrategyConfig {
        breakouts: config.breakout_lookback.map(|v| BreakoutConfig {
            breakout_lookback: v,
        }),
        pullbacks,
        enable_crossovers: config.enable_crossovers,
        enable_bias_only: config.enable_bias_only,
        sma_config: SmaConfig {
            short_window: config.sma_short_window,
            long_window: config.sma_long_window,
        },
        filters: FilterConfig {
            require_price_confirmation: config.require_price_confirmation,
            require_trend_filter: config.require_trend_filter,
            atr: if config.atr_enabled {
                Some(AtrFilter::backtest())
            } else {
                None
            },
            regime: if config.regime_enabled {
                Some(RegimeFilter::backtest())
            } else {
                None
            },
        },
    };

    let cfg = BacktestConfig {
        initial_cash: config.initial_cash,
        buy_fraction: config.buy_fraction,
        atr_enabled: config.atr_enabled,
        regime_enabled: config.regime_enabled,
        strategy,
    };

    println!("Initial cash:      {}", cfg.initial_cash);
    println!("Buy fraction:      {}", cfg.buy_fraction);
    println!("ATR enabled:       {}", cfg.atr_enabled);
    println!("Regime enabled:    {}", cfg.regime_enabled);
    println!("Strategy:          {}", strategy.describe_config());

    let result = run_backtest(&hourly, &cfg).unwrap();

    print_summary(&result);
    if let Some(hold_equity) = buy_and_hold_equity(&hourly, cfg.initial_cash) {
        println!();
        println!("Buy & hold final equity: {:.2}", hold_equity);
    }

    Ok(())
}
