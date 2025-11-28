use crate::signal::AnalysisResult;

pub fn print_analysis(result: &AnalysisResult) {
    println!("Last (hourly) timestamp: {}", result.last.ts);
    println!("Last (hourly) price:     {:.4}", result.last.price);
    println!("SMA(20):                 {:.4}", result.smas.sma20);
    println!("SMA(50):                 {:.4}", result.smas.sma50);
    println!("Prev SMA(20):            {:.4}", result.smas.prev_sma20);
    println!("Prev SMA(50):            {:.4}", result.smas.prev_sma50);

    println!("Suggestion:              {}", result.suggestion);
    println!("Reason:                  {}", result.reason);
}
