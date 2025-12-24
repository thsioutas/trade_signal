use chrono::{DateTime, Utc};

use crate::backtest::{Backtester, Candidate, TradingMetrics};
use crate::data::Sample;
use crate::indicators::compute_smas;
use crate::signal::analyze;

use super::common::{Signal, compute_max_drawdown, suggestion_to_signal};

#[derive(Debug, Clone)]
pub struct Trade {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub entry_value: f64,
    pub exit_value: f64,
    pub profit: f64,
    pub return_pct: f64,
}

#[derive(Debug, Clone)]
pub struct SpotBacktestResult {
    pub initial_equity: f64,
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub final_equity: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub win_rate_pct: f64,
}

fn compute_win_rate(trades: &[Trade]) -> f64 {
    if trades.is_empty() {
        return 0.0;
    }

    let wins = trades.iter().filter(|t| t.profit > 0.0).count() as f64;

    wins / trades.len() as f64
}

pub fn buy_and_hold_equity(hourly: &[Sample], initial_cash: f64, initial_coin: f64) -> Option<f64> {
    if hourly.is_empty() {
        return None;
    }
    let first = hourly.first().unwrap().price;
    let last = hourly.last().unwrap().price;
    if first <= 0.0 {
        return None;
    }

    let qty = initial_cash / first + initial_coin;
    Some(qty * last)
}

/// Simple CLI-style summary you can reuse in a binary.
pub fn print_summary(result: &SpotBacktestResult) {
    println!("=== Backtest Summary ===");
    println!("Initial equity:  {:.2}", result.initial_equity);
    println!("Final equity:     {:.2}", result.final_equity);
    println!("Total return:     {:.2}%", result.total_return_pct * 100.0);
    println!("Max drawdown:     {:.2}%", result.max_drawdown_pct * 100.0);
    println!("Trades:           {}", result.trades.len());
    println!("Win rate:         {:.2}%", result.win_rate_pct * 100.0);
}

#[derive(Clone, Copy)]
pub struct SpotBacktester {
    initial_cash: f64,
    initial_coin: f64,
    fee_bps: f64,
}

impl SpotBacktester {
    pub fn new(initial_cash: f64, initial_coin: f64, fee_bps: f64) -> Self {
        Self {
            initial_cash,
            initial_coin,
            fee_bps,
        }
    }
}

impl Backtester for SpotBacktester {
    type Output = SpotBacktestResult;
    fn run_backtest(
        &self,
        samples: &[Sample],
        candidate: &Candidate,
    ) -> Result<Self::Output, String> {
        if samples.len() < candidate.strategy.sma_config.long_window + 1 {
            return Err("Not enough data".to_string());
        }

        // TODO: This doesn't have to be the first price available in my sample
        // For example, I can run my backtest with other much "newer" data
        let first_price = samples[0].price.max(0.0);
        let initial_equity = self.initial_cash + self.initial_coin * first_price;

        let mut prices: Vec<f64> = Vec::with_capacity(samples.len());
        let mut equity_curve: Vec<(DateTime<Utc>, f64)> = Vec::with_capacity(samples.len());
        let mut trades: Vec<Trade> = Vec::new();

        // Initial portfolio state
        let mut cash = self.initial_cash;
        let mut coin = self.initial_coin;

        // Treat existing coin as if it was "bought" at the first price (no fee)
        let mut cost_basis_total = self.initial_coin * first_price;

        let mut in_position = self.initial_coin > 0.0;
        let mut entry_time = samples[0].ts;
        let mut avg_entry_price = if coin > 0.0 { first_price } else { 0.0 };

        let fee = self.fee_bps / 10_000.0; // e.g. 10bp => 0.001
        let fee_mult = 1.0 - fee;

        let buy_sell_frac = candidate.buy_sell_fraction.clamp(0.0, 1.0);

        for (i, candle) in samples.iter().enumerate() {
            let price = candle.price;
            prices.push(price);

            // Mark current equity (mark-to-market); no fee on unrealized
            let equity = cash + coin * price;
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
                Some(Signal::Buy) => {
                    if buy_sell_frac <= 0.0 || cash <= 0.0 || price <= 0.0 {
                        continue;
                    }

                    // Amount of cash we plan to deploy *before* fees
                    let invest_gross = cash * buy_sell_frac;
                    if invest_gross <= 0.0 {
                        continue;
                    }

                    // Net after fee
                    let invest_net = invest_gross * fee_mult;
                    let qty = invest_net / price;
                    if qty <= 0.0 {
                        continue;
                    }

                    // If this is the first time we go from flat -> long, set entry time
                    if !in_position && coin == 0.0 {
                        in_position = true;
                        entry_time = candle.ts;
                    };

                    // Update state
                    cash -= invest_gross; // we spend the gross amount (fee is embedded)
                    coin += qty;
                    cost_basis_total += invest_net; // our cost basis increases by net invested

                    // Update average entry price just for reporting
                    avg_entry_price = if coin > 0.0 {
                        cost_basis_total / coin
                    } else {
                        0.0
                    };
                }
                Some(Signal::Sell) => {
                    if buy_sell_frac <= 0.0 || coin <= 0.0 || price <= 0.0 {
                        continue;
                    }

                    let pos_before = coin;
                    let sell_qty = pos_before * buy_sell_frac;
                    if sell_qty <= 0.0 {
                        continue;
                    }

                    let gross = sell_qty * price;
                    let exit_value = gross * fee_mult;

                    // Allocate a *fraction* of cost basis to the sold chunk
                    let (entry_value_for_chunk, avg_entry_for_chunk) =
                        if cost_basis_total > 0.0 && pos_before > 0.0 {
                            let fraction_sold = sell_qty / pos_before;
                            let chunk_basis = cost_basis_total * fraction_sold;
                            cost_basis_total -= chunk_basis;

                            let avg_entry = chunk_basis / sell_qty;
                            (chunk_basis, avg_entry)
                        } else {
                            (0.0, avg_entry_price)
                        };

                    cash += exit_value;
                    coin = pos_before - sell_qty;

                    // Record this partial trade
                    let profit = exit_value - entry_value_for_chunk;
                    let ret = if entry_value_for_chunk > 0.0 {
                        exit_value / entry_value_for_chunk - 1.0
                    } else {
                        0.0
                    };

                    trades.push(Trade {
                        entry_time,
                        exit_time: candle.ts,
                        entry_price: avg_entry_for_chunk,
                        exit_price: price,
                        entry_value: entry_value_for_chunk,
                        exit_value,
                        profit,
                        return_pct: ret,
                    });

                    if coin <= 0.0 {
                        in_position = false;
                        cost_basis_total = 0.0;
                        avg_entry_price = 0.0;
                    }
                }
                _ => {
                    // HOLD or suggestion that doesn't change position
                }
            }
        }

        // If still in a trade at the end, mark to market but don't close trade
        let last_price = samples.last().unwrap().price;
        let final_equity = cash + coin * last_price;
        // If user gave nonsense initial values (0 everything), avoid divide-by-zero
        let effective_initial_equity = if initial_equity > 0.0 {
            initial_equity
        } else {
            1.0
        };

        let total_return_pct = final_equity / effective_initial_equity - 1.0;

        let max_drawdown_pct = compute_max_drawdown(&equity_curve);
        let win_rate_pct = compute_win_rate(&trades);

        Ok(SpotBacktestResult {
            initial_equity,
            trades,
            equity_curve,
            final_equity,
            total_return_pct,
            max_drawdown_pct,
            win_rate_pct,
        })
    }
}

impl TradingMetrics for SpotBacktestResult {
    fn total_return_pct(&self) -> f64 {
        self.total_return_pct
    }

    fn max_drawdown_pct(&self) -> f64 {
        self.max_drawdown_pct
    }
}
