use chrono::{DateTime, Utc};

use crate::indicators::{AtrFilter, RegimeFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Buy,
    Sell,
}

pub fn suggestion_to_signal(s: &str) -> Option<Signal> {
    match s {
        "BUY" => Some(Signal::Buy),
        "SELL" => Some(Signal::Sell),
        _ => None,
    }
}

impl RegimeFilter {
    pub fn backtest() -> Self {
        Self::default()
    }
}

impl AtrFilter {
    pub fn backtest() -> Self {
        Self::new_fixed(5, 0.003)
    }
}

pub fn compute_max_drawdown(curve: &[(DateTime<Utc>, f64)]) -> f64 {
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
