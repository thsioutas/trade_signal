use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;

use trade_signal::backtest::spot::{SpotBacktester, buy_and_hold_equity, print_summary};
use trade_signal::backtest::{Backtester, Candidate};
use trade_signal::data::{get_samples_from_input_file, resample_to_hourly};
use trade_signal::indicators::sma::SmaConfig;
use trade_signal::indicators::{AtrFilter, RegimeFilter};
use trade_signal::signal::{BreakoutConfig, FilterConfig, PullbackConfig, StrategyConfig};

#[derive(Debug, Parser)]
struct Args {
    /// config-file path
    #[arg(long)]
    config: PathBuf,
}

#[derive(Deserialize)]
pub struct Config {
    /// Path to the CSV file (timestamp,price)pub
    input: PathBuf,

    /// Initial cash for the backtest
    initial_cash: f64,

    /// Coins you already hold at the first candle
    initial_coin: f64,

    /// Fee in basis points per trade side (e.g. 10 = 0.10%)
    fee_bps: f64,

    /// Fraction of *available cash* to allocate on each BUY/SELL signal (0.0–1.0)
    buy_sell_fraction: f64,

    /// Whether ATR gate filter should be used
    atr_enabled: bool,

    /// Whether regime filter should be used
    regime_enabled: bool,

    /// How many candles to lookback for a brekdown
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

    println!("Initial cash:      {}", config.initial_cash);
    println!("Initial coin:      {}", config.initial_coin);
    println!("Fee bps:           {}", config.fee_bps);
    println!("Buy/Sell fraction: {}", config.buy_sell_fraction);
    println!("Strategy:          {}", strategy.describe_config());

    let backtester = SpotBacktester::new(config.initial_cash, config.initial_coin, config.fee_bps);
    let candidate = Candidate {
        buy_sell_fraction: config.buy_sell_fraction,
        strategy,
    };
    let result = backtester.run_backtest(&hourly, &candidate).unwrap();

    print_summary(&result);
    if let Some(hold_equity) =
        buy_and_hold_equity(&hourly, config.initial_cash, config.initial_coin)
    {
        println!();
        println!("Buy & hold final equity: {:.2}", hold_equity);
    }

    Ok(())
}
