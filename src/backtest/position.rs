use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::backtest::{Backtester, Candidate, TradingMetrics};
use crate::data::Sample;
use crate::indicators::compute_smas;
use crate::signal::analyze;

use super::common::{Signal, suggestion_to_signal};

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub side: PositionSide,
    pub entry_time: DateTime<Utc>,
    pub exit_time: Option<DateTime<Utc>>,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub entry_reason: String,
    pub exit_reason: Option<String>,
    pub size: f64,
    pub profit: Option<f64>,
    pub return_pct: Option<f64>,
    /// Gross collateral removed from cash at entry (before entry fee).
    pub entry_collateral_gross: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PositionSide {
    Long,
    Short,
}

impl From<Signal> for PositionSide {
    fn from(s: Signal) -> Self {
        match s {
            Signal::Buy => Self::Long,
            Signal::Sell => Self::Short,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionBacktestResult {
    pub initial_equity: f64,
    pub positions: Vec<Position>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub final_equity: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub win_rate_pct: f64,
}

fn position_liquidation_value(pos: &Position, price: f64) -> f64 {
    if price <= 0.0 || pos.size <= 0.0 {
        return 0.0;
    }

    match pos.side {
        PositionSide::Long => pos.size * price,
        PositionSide::Short => {
            let gross_pnl = (pos.entry_price - price) * pos.size;
            pos.entry_collateral_gross + gross_pnl
        }
    }
}

fn close_position(
    mut pos: Position,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    exit_reason: String,
) -> Position {
    pos.exit_price = Some(exit_price);
    pos.exit_time = Some(exit_time);
    pos.exit_reason = Some(exit_reason);

    let gross_pnl = match pos.side {
        PositionSide::Long => (exit_price - pos.entry_price) * pos.size,
        PositionSide::Short => (pos.entry_price - exit_price) * pos.size,
    };

    let profit = gross_pnl;
    let ret = if pos.entry_collateral_gross > 0.0 {
        profit / pos.entry_collateral_gross
    } else {
        0.0
    };

    pos.profit = Some(profit);
    pos.return_pct = Some(ret);
    pos
}

fn open_position(
    side: PositionSide,
    price: f64,
    ts: DateTime<Utc>,
    cash: &mut f64,
    entry_frac: f64,
    reason: String,
) -> Option<Position> {
    if price <= 0.0 || *cash <= 0.0 || entry_frac <= 0.0 {
        return None;
    }

    let entry_collateral_gross = (*cash) * entry_frac;
    if entry_collateral_gross <= 0.0 {
        return None;
    }

    let size = entry_collateral_gross / price;
    if size <= 0.0 {
        return None;
    }

    *cash -= entry_collateral_gross;

    Some(Position {
        side,
        entry_time: ts,
        exit_time: None,
        entry_price: price,
        exit_price: None,
        entry_reason: reason,
        exit_reason: None,
        size,
        entry_collateral_gross,
        profit: None,
        return_pct: None,
    })
}

fn compute_max_drawdown(curve: &[(DateTime<Utc>, f64)]) -> f64 {
    if curve.is_empty() {
        return 0.0;
    }

    let mut peak = curve[0].1;
    let mut max_dd = 0.0;

    for &(_, equity) in curve {
        if equity > peak {
            peak = equity;
        }
        if peak > 0.0 {
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }

    max_dd
}

fn compute_win_rate(positions: &[Position]) -> f64 {
    if positions.is_empty() {
        return 0.0;
    }

    let wins = positions
        .iter()
        .filter(|p| p.profit.unwrap_or(0.0) > 0.0)
        .count() as f64;

    wins / positions.len() as f64
}

pub fn buy_and_hold_equity(hourly: &[Sample], initial_cash: f64) -> Option<f64> {
    if hourly.is_empty() {
        return None;
    }
    let first = hourly.first().unwrap().price;
    let last = hourly.last().unwrap().price;
    if first <= 0.0 {
        return None;
    }

    let qty = initial_cash / first;
    Some(qty * last)
}

/// Simple CLI-style summary you can reuse in a binary.
pub fn print_summary(result: &PositionBacktestResult) {
    println!("=== Backtest Summary ===");
    println!("Initial equity:  {:.2}", result.initial_equity);
    println!("Final equity:     {:.2}", result.final_equity);
    println!("Total return:     {:.2}%", result.total_return_pct * 100.0);
    println!("Max drawdown:     {:.2}%", result.max_drawdown_pct * 100.0);
    println!("Positions:           {}", result.positions.len());
    println!("Win rate:         {:.2}%", result.win_rate_pct * 100.0);
}

pub struct PositionBacktester<L> {
    initial_cash: f64,
    logger: L,
}

impl PositionBacktester<NoopLogger> {
    pub fn new(initial_cash: f64) -> Self {
        Self {
            initial_cash,
            logger: NoopLogger,
        }
    }
}

impl<L: PositionLogger> PositionBacktester<L> {
    pub fn with_logger(initial_cash: f64, logger: L) -> Self
    where
        L: PositionLogger,
    {
        Self {
            initial_cash,
            logger,
        }
    }
}

impl<L: PositionLogger> Backtester for PositionBacktester<L> {
    type Output = PositionBacktestResult;
    fn run_backtest(
        &self,
        samples: &[Sample],
        candidate: &Candidate,
    ) -> Result<Self::Output, String> {
        if samples.len() < candidate.strategy.sma_config.long_window + 1 {
            return Err("Not enough data".into());
        }

        let initial_equity = self.initial_cash;

        let mut prices: Vec<f64> = Vec::with_capacity(samples.len());
        let mut equity_curve: Vec<(DateTime<Utc>, f64)> = Vec::with_capacity(samples.len());
        let mut open: Option<Position> = None;
        let mut closed: Vec<Position> = Vec::new();

        // Initial portfolio state
        let mut cash = self.initial_cash;

        let buy_frac = candidate.buy_sell_fraction.clamp(0.0, 1.0);

        for (i, candle) in samples.iter().enumerate() {
            let price = candle.price;
            prices.push(price);

            let equity = cash
                + open
                    .as_ref()
                    .map(|p| position_liquidation_value(p, price))
                    .unwrap_or(0.0);
            equity_curve.push((candle.ts, equity));

            if prices.len() < candidate.strategy.sma_config.long_window + 1 {
                // Not enough data yet for SMAs
                continue;
            }

            let Some(smas) = compute_smas(&prices, candidate.strategy.sma_config) else {
                continue;
            };

            let analysis = analyze(&samples[..=i], &prices, smas, candidate.strategy);
            let signal = suggestion_to_signal(&analysis.suggestion);

            match signal {
                Some(signal) => {
                    let want_side = signal.into();
                    let same_side = open.as_ref().map(|p| p.side == want_side).unwrap_or(false);
                    if !same_side {
                        // close old if exists
                        if let Some(pos) = open.take() {
                            let closed_pos =
                                close_position(pos, price, candle.ts, analysis.reason.clone());
                            self.logger.log(&closed_pos)?;
                            cash += closed_pos.entry_collateral_gross
                                + closed_pos.profit.unwrap_or(0.0);
                            closed.push(closed_pos);
                        }
                        // open new
                        if let Some(pos) = open_position(
                            want_side,
                            price,
                            candle.ts,
                            &mut cash,
                            buy_frac,
                            analysis.reason,
                        ) {
                            open = Some(pos);
                        }
                    }
                }
                _ => {
                    // HOLD or suggestion that doesn't change position
                }
            }
        }

        // If a position is open close it
        if let Some(pos) = open.take() {
            let last = samples.last().unwrap();
            let closed_pos = close_position(pos, last.price, last.ts, "EOF".to_string());
            self.logger.log(&closed_pos)?;
            cash += closed_pos.entry_collateral_gross + closed_pos.profit.unwrap_or(0.0);
            closed.push(closed_pos);
        }
        let final_equity = cash;
        let total_return_pct = final_equity / initial_equity - 1.0;

        let max_drawdown_pct = compute_max_drawdown(&equity_curve);
        let win_rate_pct = compute_win_rate(&closed);

        Ok(PositionBacktestResult {
            initial_equity,
            positions: closed,
            equity_curve,
            final_equity,
            total_return_pct,
            max_drawdown_pct,
            win_rate_pct,
        })
    }
}

impl TradingMetrics for PositionBacktestResult {
    fn total_return_pct(&self) -> f64 {
        self.total_return_pct
    }

    fn max_drawdown_pct(&self) -> f64 {
        self.max_drawdown_pct
    }
}

pub trait PositionLogger: Sync {
    fn log(&self, position: &Position) -> Result<(), String>;
}

pub struct NdjsonLogger {
    pub path: PathBuf,
}

impl NdjsonLogger {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl PositionLogger for NdjsonLogger {
    fn log(&self, pos: &Position) -> Result<(), String> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|err| err.to_string())?;
        let line = serde_json::to_string(pos).map_err(|err| err.to_string())?;
        writeln!(f, "{line}").map_err(|err| err.to_string())?;
        Ok(())
    }
}

pub struct NoopLogger;

impl PositionLogger for NoopLogger {
    fn log(&self, _pos: &Position) -> Result<(), String> {
        Ok(())
    }
}
