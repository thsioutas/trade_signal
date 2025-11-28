use anyhow::Result;
use clap::Parser;

use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Args {
    /// Path to the CSV file (timestamp,price)
    #[arg(long)]
    input: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Load raw samples from CSV
    let samples = sma_analyzer::data::get_samples_from_input_file(&args.input)?;
    if samples.is_empty() {
        println!("No data found in CSV.");
        return Ok(());
    }

    // Resample to hourly closes
    let hourly = sma_analyzer::data::resample_to_hourly(&samples);
    println!(
        "Loaded {} raw points, {} hourly candles after resampling.",
        samples.len(),
        hourly.len()
    );
    if hourly.is_empty() {
        println!("No hourly data after resampling.");
        return Ok(());
    }

    // Extract prices and compute SMAs
    let prices: Vec<f64> = hourly.iter().map(|s| s.price).collect();
    let Some(smas) = sma_analyzer::indicators::compute_smas(&prices) else {
        println!(
            "Not enough data: need at least 51 hourly candles for SMA20/50 logic, got {}.",
            prices.len()
        );
        return Ok(());
    };

    // Perform final analysis
    let result = sma_analyzer::signal::analyze(&hourly, &prices, smas);

    // Print result.clone()
    sma_analyzer::output::print_analysis(&result);

    Ok(())
}
