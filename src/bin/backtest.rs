use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use trade_signal::indicators::sma::SmaConfig;
use trade_signal::signal::{BreakoutConfig, PullbackConfig, StrategyConfig};

use trade_signal::backtest::{BacktestConfig, buy_and_hold_equity, print_summary, run_backtest};
use trade_signal::data::{get_samples_from_input_file, resample_to_hourly};

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
    /// Do not set to not use breakout patterns
    #[arg(long)]
    breakout_lookback: Option<usize>,

    /// Do not set to not use pullback patterns
    #[arg(long)]
    pullback_bounce_tolerance_pct: Option<f64>,

    /// Do not set to not use pullback patterns
    #[arg(long)]
    pullback_rejection_tolerance_pct: Option<f64>,

    /// Whether pullback signals should be used
    #[arg(long, default_value_t = false)]
    enable_pullbacks: bool,

    /// Whether sma crossover signals should be used
    #[arg(long, default_value_t = false)]
    enable_crossovers: bool,

    /// Whether bias_only signals should be used
    #[arg(long, default_value_t = false)]
    enable_bias_only: bool,

    /// SMA short window
    #[arg(long, default_value_t = 20)]
    sma_short_window: usize,

    /// SMA long window
    #[arg(long, default_value_t = 50)]
    sma_long_window: usize,
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

    let pullbacks = match (
        args.pullback_bounce_tolerance_pct,
        args.pullback_rejection_tolerance_pct,
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
        breakouts: args.breakout_lookback.map(|v| BreakoutConfig {
            breakout_lookback: v,
        }),
        pullbacks,
        enable_crossovers: args.enable_crossovers,
        enable_bias_only: args.enable_bias_only,
        sma_config: SmaConfig {
            short_window: args.sma_short_window,
            long_window: args.sma_long_window,
        },
    };

    let cfg = BacktestConfig {
        initial_cash: args.initial_cash,
        initial_coin: args.initial_coin,
        fee_bps: args.fee_bps,
        buy_fraction: args.buy_fraction,
        sell_fraction: args.sell_fraction,
        atr_enabled: args.atr_enabled,
        regime_enabled: args.regime_enabled,
        strategy,
    };

    println!("Initial cash:      {}", cfg.initial_cash);
    println!("Initial coin:      {}", cfg.initial_coin);
    println!("Fee bps:           {}", cfg.initial_coin);
    println!("Fee bps:           {}", cfg.fee_bps);
    println!("Buy fraction:      {}", cfg.buy_fraction);
    println!("Sell fraction:     {}", cfg.sell_fraction);
    println!("ATR enabled:       {}", cfg.atr_enabled);
    println!("Regime enabled:    {}", cfg.regime_enabled);
    println!("Strategy:          {}", strategy.describe_config());

    let Some(result) = run_backtest(&hourly, &cfg) else {
        println!(
            "Not enough hourly data: need at least 51 candles, got {}.",
            hourly.len()
        );
        return Ok(());
    };

    print_summary(&result);
    if let Some(hold_equity) = buy_and_hold_equity(&hourly, cfg.initial_cash, cfg.initial_coin) {
        println!();
        println!("Buy & hold final equity: {:.2}", hold_equity);
    }

    Ok(())
}
