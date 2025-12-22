use crate::data::Sample;
use crate::indicators::sma::SmaConfig;
use crate::indicators::{AtrFilter, Regime, RegimeFilter, Smas};
use crate::patterns::{
    is_breakdown_below_recent_low, is_breakout_above_recent_high,
    is_pullback_to_sma_short_and_bounce, is_pullback_to_sma_short_and_reject_down,
};

#[derive(Clone, Copy, Debug)]
pub struct StrategyConfig {
    pub breakouts: Option<BreakoutConfig>,
    pub pullbacks: Option<PullbackConfig>,
    pub enable_crossovers: bool,
    pub enable_bias_only: bool,
    pub sma_config: SmaConfig,
    pub filters: FilterConfig,
}

impl StrategyConfig {
    pub fn describe_config(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!(
            "SMA{}/{}",
            self.sma_config.short_window, self.sma_config.long_window,
        ));
        if let Some(b) = &self.breakouts {
            parts.push(format!("breakout(lookback={})", b.breakout_lookback));
        }
        if let Some(p) = self.pullbacks {
            parts.push(format!(
                "pullback(bounce={:.3},rejection{:.3})",
                p.bounce_tolerance_pct, p.reject_tolerance_pct
            ));
        }
        if self.enable_crossovers {
            parts.push("crossovers".to_string());
        }
        if self.enable_bias_only {
            parts.push("bias_only".to_string());
        }
        if self.filters.require_price_confirmation {
            parts.push("require_price_confirmation".to_string());
        }
        if self.filters.require_trend_filter {
            parts.push("require_trend_filter".to_string());
        }
        if let Some(atr) = self.filters.atr {
            let atr_description = format!("atr(floor={}, period={})", atr.floor(), atr.period());
            parts.push(atr_description);
        }
        if let Some(regime) = self.filters.regime {
            let atr_description = format!(
                "regime(long_window={}, slope_window={}, min_trend_strength={}, min_range={})",
                regime.long_window,
                regime.slope_window,
                regime.min_trend_strength,
                regime.min_range
            );
            parts.push(atr_description);
        }

        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(" + ")
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BreakoutConfig {
    pub breakout_lookback: usize,
}

/// e.g. 0.003 = 0.3% tolerance around SMA
#[derive(Clone, Copy, Debug)]
pub struct PullbackConfig {
    pub bounce_tolerance_pct: f64,
    pub reject_tolerance_pct: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct FilterConfig {
    pub require_trend_filter: bool,
    pub require_price_confirmation: bool,
    pub atr: Option<AtrFilter>,
    pub regime: Option<RegimeFilter>,
}

pub struct AnalysisResult {
    pub last: Sample,
    pub smas: Smas,
    pub suggestion: String,
    pub reason: String,
}

/// Advanced trading rule based on:
/// - Breakout above recent high in an uptrend
/// - Breakout below recent low in a downtrend
/// - Pullback to SMA(short) + bounce (uptrend)
/// - Pullback to SMA(short) + rejection (downtrend)
/// - Golden Cross / Death Cross detection (using previous + current SMAs)
/// - Trend filter using SMA(long) slope
/// - Price confirmation (price relative to SMA(short) & SMA(long))
///
/// Returns (short_suggestion, optional_detailed_reason)
pub fn analyze(
    hourly: &[Sample],
    prices: &[f64],
    smas: Smas,
    strategy: StrategyConfig,
) -> AnalysisResult {
    let last = hourly.last().expect("hourly is non-empty").to_owned();
    let (suggestion, reason) = suggest_action(prices, smas, strategy);
    AnalysisResult {
        last,
        smas,
        suggestion,
        reason,
    }
}

struct AnalysisCtx {
    pub smas: Smas,
    pub gate_long: Option<String>,
    pub gate_short: Option<String>,
}

impl AnalysisCtx {
    pub fn new(prices: &[f64], smas: Smas, strategy: &StrategyConfig) -> Self {
        let last_price = *prices.last().expect("prices non-empty");

        let uptrend = smas.sma_short > smas.sma_long && smas.sma_long >= smas.prev_sma_long;
        let downtrend = smas.sma_short < smas.sma_long && smas.sma_long <= smas.prev_sma_long;

        let price_above_both = last_price > smas.sma_short && last_price > smas.sma_long;
        let price_below_both = last_price < smas.sma_short && last_price < smas.sma_long;

        let (regime_up, regime_down) = strategy
            .filters
            .regime
            .map(|rf| {
                let r = rf.detect_regime(prices);
                (
                    matches!(r, Regime::TrendingUp),
                    matches!(r, Regime::TrendingDown),
                )
            })
            .unwrap_or((true, true));

        let gate_long = if strategy.filters.require_trend_filter && !uptrend {
            Some("Trend filter vetoed long (not uptrend)".into())
        } else if strategy.filters.require_price_confirmation && !price_above_both {
            Some("Price confirmation vetoed long (not above both MAs)".into())
        } else if !regime_up {
            Some("Regime filter vetoed long".into())
        } else {
            None
        };

        let gate_short = if strategy.filters.require_trend_filter && !downtrend {
            Some("Trend filter vetoed short (not downtrend)".into())
        } else if strategy.filters.require_price_confirmation && !price_below_both {
            Some("Price confirmation vetoed short (not below both MAs)".into())
        } else if !regime_down {
            Some("Regime filter vetoed short".into())
        } else {
            None
        };

        Self {
            smas,
            gate_long,
            gate_short,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Buy,
    Sell,
    Hold,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = format!("{:?}", self).to_uppercase();
        write!(f, "{}", out)
    }
}

#[derive(Debug, Clone)]
struct Decision {
    action: Action,
    reason: String,
    rule: String,
}

#[derive(Debug, Clone)]
enum RuleOutcome {
    NoMatch,
    Blocked { reason: String },
    Fired(Decision),
}

fn rule_crossovers(ctx: &AnalysisCtx) -> RuleOutcome {
    let golden =
        ctx.smas.prev_sma_short <= ctx.smas.prev_sma_long && ctx.smas.sma_short > ctx.smas.sma_long;
    let death =
        ctx.smas.prev_sma_short >= ctx.smas.prev_sma_long && ctx.smas.sma_short < ctx.smas.sma_long;

    if golden {
        if let Some(r) = &ctx.gate_long {
            return RuleOutcome::Blocked {
                reason: format!("Golden Cross, but {r}"),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Buy,
            rule: "Crossovers".into(),
            reason: "Golden Cross".into(),
        });
    }

    if death {
        if let Some(r) = &ctx.gate_short {
            return RuleOutcome::Blocked {
                reason: format!("Death Cross, but {r}"),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Sell,
            rule: "Crossovers".into(),
            reason: "Death Cross".into(),
        });
    }

    RuleOutcome::NoMatch
}

fn rule_breakouts(ctx: &AnalysisCtx, prices: &[f64], config: BreakoutConfig) -> RuleOutcome {
    let rule = "Breakouts";
    if is_breakout_above_recent_high(prices, config.breakout_lookback) {
        let reason = "Breakout above recent high";
        if let Some(r) = &ctx.gate_long {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Buy,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    if is_breakdown_below_recent_low(prices, config.breakout_lookback) {
        let reason = "Breakdown below recent low";
        if let Some(r) = &ctx.gate_short {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Sell,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    RuleOutcome::NoMatch
}

fn rule_pullbacks(
    ctx: &AnalysisCtx,
    prices: &[f64],
    pullback_config: PullbackConfig,
) -> RuleOutcome {
    let rule = "Pullbacks";
    if is_pullback_to_sma_short_and_bounce(
        prices,
        ctx.smas.sma_short,
        pullback_config.bounce_tolerance_pct,
    ) {
        let reason = "Pullback to SMA short and bounce";
        if let Some(r) = &ctx.gate_long {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Buy,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    if is_pullback_to_sma_short_and_reject_down(
        prices,
        ctx.smas.sma_short,
        pullback_config.reject_tolerance_pct,
    ) {
        let reason = "Pullback up to SMA short and rejection";
        if let Some(r) = &ctx.gate_short {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Sell,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    RuleOutcome::NoMatch
}

fn rule_bias_only(ctx: &AnalysisCtx) -> RuleOutcome {
    let rule = "Bias only";
    if ctx.smas.sma_short > ctx.smas.sma_long {
        let reason = "Uptrend (SMA short > SMA long)";
        if let Some(r) = &ctx.gate_long {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Buy,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    if ctx.smas.sma_short < ctx.smas.sma_long {
        let reason = "Downtrend (SMA short < SMA long)";
        if let Some(r) = &ctx.gate_short {
            return RuleOutcome::Blocked {
                reason: format!("{}, but {r}", reason),
            };
        }
        return RuleOutcome::Fired(Decision {
            action: Action::Sell,
            rule: rule.into(),
            reason: reason.into(),
        });
    }

    RuleOutcome::NoMatch
}

fn suggest_action(prices: &[f64], smas: Smas, strategy: StrategyConfig) -> (String, String) {
    // TODO: Consider mocking breakout, atr and regime indicators. Their functionality is already tested by other UTs

    // ~~~~ Volatility filter (ATR) ~~~~
    if let Some(atr_filter) = strategy.filters.atr {
        let atr_p = match atr_filter.atr_percent(prices) {
            Some(v) => v,
            None => {
                return (
                    "HOLD".into(),
                    format!(
                        "Insufficient data for ATR({}) volatility filter",
                        atr_filter.period()
                    ),
                );
            }
        };

        if atr_p < atr_filter.floor() {
            let atr_pct = atr_p * 100.0;
            let floor_pct = atr_filter.floor() * 100.0;
            return (
                "HOLD".into(),
                format!(
                    "Volatility too low: ATR({}) = {:.2}% < floor {:.2}%",
                    atr_filter.period(),
                    atr_pct,
                    floor_pct
                ),
            );
        }
    }

    let analysis_ctx = AnalysisCtx::new(prices, smas, &strategy);

    let mut fired_but_blocked = Vec::new();

    // TODO: Move the strategies to different files and create necessary Strategy Trait
    if let Some(breakouts) = strategy.breakouts {
        match rule_breakouts(&analysis_ctx, prices, breakouts) {
            RuleOutcome::Fired(d) => return (d.action.to_string(), d.reason),
            RuleOutcome::Blocked { reason } => fired_but_blocked.push(reason),
            _ => {}
        }
    }

    if let Some(lookback) = strategy.pullbacks {
        match rule_pullbacks(&analysis_ctx, prices, lookback) {
            RuleOutcome::Fired(d) => return (d.action.to_string(), d.reason),
            RuleOutcome::Blocked { reason } => fired_but_blocked.push(reason),
            _ => {}
        }
    }

    if strategy.enable_crossovers {
        match rule_crossovers(&analysis_ctx) {
            RuleOutcome::Fired(d) => return (d.action.to_string(), d.reason),
            RuleOutcome::Blocked { reason } => fired_but_blocked.push(reason),
            _ => {}
        }
    }

    if strategy.enable_bias_only {
        match rule_bias_only(&analysis_ctx) {
            RuleOutcome::Fired(d) => return (d.action.to_string(), d.reason),
            RuleOutcome::Blocked { reason } => fired_but_blocked.push(reason),
            _ => {}
        }
    }

    if !fired_but_blocked.is_empty() {
        return ("HOLD".into(), fired_but_blocked.join(" & "));
    }

    ("HOLD".into(), "No strategy matched".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl StrategyConfig {
        fn test_config() -> Self {
            Self {
                breakouts: Some(BreakoutConfig {
                    breakout_lookback: 5,
                }),
                enable_bias_only: true,
                enable_crossovers: true,
                pullbacks: Some(PullbackConfig {
                    bounce_tolerance_pct: 0.003,
                    reject_tolerance_pct: 0.003,
                }),
                sma_config: SmaConfig::sma_20_50(),
                filters: FilterConfig {
                    require_trend_filter: true,
                    require_price_confirmation: true,
                    atr: None,
                    regime: None,
                },
            }
        }
    }

    impl Smas {
        fn downtrend_for_breakdown() -> Self {
            Self {
                sma_short: 95.0,
                sma_long: 100.0,
                prev_sma_short: 96.0,
                prev_sma_long: 101.0, // sma_long <= prev_sma_long => 100 <= 101
            }
        }

        fn downtrend_for_pullback() -> Self {
            Self {
                sma_short: 100.0,
                sma_long: 110.0,
                prev_sma_short: 101.0,
                prev_sma_long: 111.0,
            }
        }

        fn uptrend_for_breakout() -> Self {
            Self {
                sma_short: 105.0,
                sma_long: 100.0,
                prev_sma_short: 104.0,
                prev_sma_long: 99.0, // sma_long >= prev_sma_long => 100 >= 99
            }
        }

        fn uptrend_for_bounce() -> Self {
            Self {
                sma_short: 100.0,
                sma_long: 95.0,
                prev_sma_short: 99.0,
                prev_sma_long: 94.0,
            }
        }

        fn golden_cross() -> Self {
            Self {
                sma_short: 105.0,
                sma_long: 100.0,
                prev_sma_short: 95.0,
                prev_sma_long: 100.0, // prev_sma_short <= prev_sma_long && sma_short > sma_long
            }
        }

        fn death_cross() -> Self {
            Self {
                sma_short: 95.0,
                sma_long: 100.0,
                prev_sma_short: 105.0,
                prev_sma_long: 100.0, // prev_sma_short >= prev_sma_long && sma_short < sma_long
            }
        }

        fn long_bias_only() -> Self {
            Self {
                sma_short: 105.0,
                sma_long: 100.0,
                prev_sma_short: 105.0,
                prev_sma_long: 100.0, // no golden cross (prev_sma_short <= prev_sma_long is false)
            }
        }

        fn short_bias_only() -> Self {
            Self {
                sma_short: 95.0,
                sma_long: 100.0,
                prev_sma_short: 95.0,
                prev_sma_long: 100.0, // no death cross (prev_sma_short >= prev_sma_long is false)
            }
        }
    }

    #[test]
    fn test_suggest_action_sell_on_breakdown_below_recent_low_in_downtrend() {
        // window = [100, 99, 98, 97, 96], recent_low = 96
        // last_price = 90 < 96 * (1 - eps) -> breakdown
        let prices = vec![100.0, 99.0, 98.0, 97.0, 96.0, 90.0];
        let smas = Smas::downtrend_for_breakdown();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "SELL");
        assert_eq!(reason, "Breakdown below recent low");
    }

    #[test]
    fn test_suggest_action_sell_on_pullback_to_sma_short_and_rejection_in_downtrend() {
        // Last 3 candles:
        // p2 = 95 (below sma_short)
        // p1 = 100 (pullback to
        // p0 =  98 (reject down)
        //
        // len = 3 => breakdown / breakout can't trigger (need >= 6)
        let prices = vec![95.0, 100.0, 98.0];
        let smas = Smas::downtrend_for_pullback();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "SELL");
        assert_eq!(reason, "Pullback up to SMA short and rejection");
    }

    #[test]
    fn test_suggest_action_buy_on_breakout_above_recent_high_in_uptrend() {
        // prices: [100, 101, 102, 103, 104, 110]
        // window (lookback=5) = [100..104], recent_high = 104
        // last_price = 110 > 104 * (1 + eps) => breakout
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0, 110.0];
        let smas = Smas::uptrend_for_breakout();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "BUY");
        assert_eq!(reason, "Breakout above recent high");
    }

    #[test]
    fn test_suggest_action_buy_on_pullback_to_sma_short_and_bounce_in_uptrend() {
        // Last 3 candles:
        // p2 = 105 (> sma_short=100)
        // p1 = 100 (pullback to SMA(short))
        // p0 = 103 (bounce above)
        //
        // len = 3 => no breakout/breakdown. Uptrend is true.
        let prices = vec![105.0, 100.0, 103.0];
        let smas = Smas::uptrend_for_bounce();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "BUY");
        assert_eq!(reason, "Pullback to SMA short and bounce");
    }

    #[test]
    fn test_suggest_action_buy_on_golden_cross_with_confirmation() {
        // Uptrend + golden cross + price_above_both.
        // prices: [100, 102, 106]; last_price = 106
        let prices = vec![100.0, 102.0, 106.0];
        let smas = Smas::golden_cross();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "BUY");
        assert_eq!(reason, "Golden Cross");
    }

    #[test]
    fn test_suggest_action_sell_on_death_cross_with_confirmation() {
        // Downtrend + death cross + price_below_both.
        // prices: [100, 99, 94]; last_price = 94
        let prices = vec![100.0, 99.0, 94.0];
        let smas = Smas::death_cross();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "SELL");
        assert_eq!(reason, "Death Cross");
    }

    #[test]
    fn test_suggest_action_hold_long_bias_when_uptrend_but_no_strong_signal() {
        // Uptrend, price above both MAs, but no cross / breakout / pullback pattern.
        // prices: [101, 103, 106]; p2 = 101 (not > sma_short=105) => no bounce pattern.
        let prices = vec![101.0, 103.0, 106.0];
        let smas = Smas::long_bias_only();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "BUY");
        assert_eq!(reason, "Uptrend (SMA short > SMA long)");
    }

    #[test]
    fn test_suggest_action_hold_short_bias_when_downtrend_but_no_strong_signal() {
        // Downtrend, price below both MAs, but no cross / breakdown / pullback pattern.
        // prices: [100, 95, 90]; len=3 -> no breakdown
        let prices = vec![100.0, 95.0, 90.0];
        let smas = Smas::short_bias_only();

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "SELL");
        assert_eq!(reason, "Downtrend (SMA short < SMA long)");
    }

    #[test]
    fn test_suggest_action_generic_hold_when_no_trend_or_signal() {
        // Flat SMAs, price neither above nor below both.
        let prices = vec![100.0, 100.0, 100.0];
        let smas = Smas {
            sma_short: 100.0,
            sma_long: 100.0,
            prev_sma_short: 100.0,
            prev_sma_long: 100.0,
        };

        let (suggestion, reason) =
            super::suggest_action(&prices, smas, StrategyConfig::test_config());

        assert_eq!(suggestion, "HOLD");
        assert_eq!(reason, "No strategy matched");
    }

    #[test]
    fn test_suggest_action_hold_when_volatility_below_floor() {
        // Flat / almost-flat prices -> ATR% ≈ 0, definitely below a 1% floor.
        // This should trigger the ATR gate *before* any trend / pattern logic.
        let prices = vec![100.0; 40]; // enough points for ATR(14) to be computed

        let smas = Smas {
            sma_short: 100.0,
            sma_long: 100.0,
            prev_sma_short: 100.0,
            prev_sma_long: 100.0,
        };

        // High-ish floor: 1% ATR required.
        // Since prices are constant, ATR% ≈ 0 -> won't pass the gate
        let atr_filter = AtrFilter::new_fixed(14, 0.01);
        let mut strategy = StrategyConfig::test_config();
        strategy.filters.atr = Some(atr_filter);
        let (suggestion, reason) = super::suggest_action(&prices, smas, strategy);

        assert_eq!(suggestion, "HOLD");
        assert!(
            reason.contains("Volatility too low"),
            "Expected 'Volatility too low' in reason, got: {reason}"
        );
    }

    impl RegimeFilter {
        fn trending_up_filter() -> Self {
            Self {
                long_window: 3,
                slope_window: 3,
                min_trend_strength: 0.01, // 1%
                min_range: 0.0,
            }
        }

        fn trending_down_filter() -> Self {
            Self {
                long_window: 3,
                slope_window: 3,
                min_trend_strength: 0.01,
                min_range: 0.0,
            }
        }

        fn sideways_filter() -> Self {
            // Parameters that make it hard to classify as trending
            Self {
                long_window: 3,
                slope_window: 3,
                min_trend_strength: 0.20, // 20% required move -> most of our tiny moves are "sideways"
                min_range: 0.20,          // and 20% range too
            }
        }
    }

    #[test]
    fn test_suggest_action_buy_allowed_in_trending_up_regime() {
        // prices chosen to:
        // - form an uptrend
        // - trigger breakout above recent high (lookback=5)
        // window = [100, 101, 102, 103, 104], last = 110 > 104
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0, 110.0];
        let smas = Smas::uptrend_for_breakout();

        let regime_filter = RegimeFilter::trending_up_filter();
        let mut strategy = StrategyConfig::test_config();
        strategy.filters.regime = Some(regime_filter);
        let (suggestion, reason) = super::suggest_action(&prices, smas, strategy);

        assert_eq!(suggestion, "BUY");
        assert!(
            reason.contains("Breakout above recent high"),
            "unexpected reason: {}",
            reason
        );
    }

    #[test]
    fn test_suggest_action_sell_allowed_in_trending_down_regime() {
        // Breakdown case:
        // window = [100, 99, 98, 97, 96], recent_low = 96
        // last = 90 < 96 => breakdown
        let prices = vec![100.0, 99.0, 98.0, 97.0, 96.0, 90.0];
        let smas = Smas::downtrend_for_breakdown();

        let regime_filter = RegimeFilter::trending_down_filter();
        let mut strategy = StrategyConfig::test_config();
        strategy.filters.regime = Some(regime_filter);
        let (suggestion, reason) = super::suggest_action(&prices, smas, strategy);

        assert_eq!(suggestion, "SELL");
        assert!(
            reason.contains("Breakdown below recent low"),
            "unexpected reason: {}",
            reason
        );
    }

    #[test]
    fn test_suggest_action_sell_blocked_in_sideways_regime() {
        // Same breakdown pattern + downtrend SMAs, but regime thinks "Sideways".
        // In that case we don't want strong SELL signals.
        let prices = vec![100.0, 99.0, 98.0, 97.0, 96.0, 90.0];
        let smas = Smas::downtrend_for_breakdown();

        let regime_filter = RegimeFilter::sideways_filter();
        let mut strategy = StrategyConfig::test_config();
        strategy.filters.regime = Some(regime_filter);
        let (suggestion, reason) = super::suggest_action(&prices, smas, strategy);

        assert_eq!(suggestion, "HOLD");
        assert_eq!(
            reason,
            "Breakdown below recent low, but Regime filter vetoed short & Downtrend (SMA short < SMA long), but Regime filter vetoed short"
        );
    }

    #[test]
    fn test_suggest_action_buy_blocked_in_sideways_regime() {
        // Uptrend breakout, but regime says Sideways -> block BUY
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0, 110.0];
        let smas = Smas::uptrend_for_breakout();

        let regime_filter = RegimeFilter::sideways_filter();

        let mut strategy = StrategyConfig::test_config();
        strategy.filters.regime = Some(regime_filter);
        let (suggestion, reason) = super::suggest_action(&prices, smas, strategy);

        assert_ne!(suggestion, "BUY");
        assert!(
            suggestion == "HOLD" || suggestion.starts_with("HOLD /"),
            "expected HOLD-like suggestion, got {} ({})",
            suggestion,
            reason
        );
    }
}
