use crate::{indicators::sma::SmaConfig, signal::AnalysisResult};

pub fn print_analysis(result: &AnalysisResult, sma_config: SmaConfig) {
    println!("Last (hourly) timestamp: {}", result.last.ts);
    println!("Last (hourly) price:     {:.4}", result.last.price);
    println!(
        "SMA({}):                 {:.4}",
        sma_config.short_window, result.smas.sma_short
    );
    println!(
        "SMA({}):                 {:.4}",
        sma_config.long_window, result.smas.sma_long
    );
    println!(
        "Prev SMA({}):            {:.4}",
        sma_config.short_window, result.smas.prev_sma_short
    );
    println!(
        "Prev SMA({}):            {:.4}",
        sma_config.long_window, result.smas.prev_sma_long
    );

    println!("Suggestion:              {}", result.suggestion);
    println!("Reason:                  {}", result.reason);
}
