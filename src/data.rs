use anyhow::{Context, Result};
use chrono::{DateTime, Timelike, Utc};
use csv::ReaderBuilder;
use serde::Deserialize;

use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct PriceRow {
    pub timestamp: String,
    pub price: f64,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub ts: DateTime<Utc>,
    pub price: f64,
}

pub fn get_samples_from_input_file(input: &PathBuf) -> Result<Vec<Sample>> {
    let file =
        File::open(input).with_context(|| format!("failed to open input file: {:?}", input))?;

    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut samples: Vec<Sample> = Vec::new();

    for result in rdr.deserialize::<PriceRow>() {
        let row: PriceRow = result.with_context(|| "failed to deserialize CSV row")?;
        let ts = DateTime::parse_from_rfc3339(&row.timestamp)
            .with_context(|| format!("failed to parse timestamp: {}", row.timestamp))?
            .with_timezone(&Utc);
        samples.push(Sample {
            ts,
            price: row.price,
        });
    }
    Ok(samples)
}

/// Resample raw samples (possibly 5m + 1h mixed) into 1-hour "closes".
/// For each hour bucket, we keep the *last* price observed in that hour.
pub fn resample_to_hourly(samples: &[Sample]) -> Vec<Sample> {
    let mut buckets: BTreeMap<DateTime<Utc>, Sample> = BTreeMap::new();

    for s in samples {
        // Truncate to the start of the hour for the bucket key
        let hour_start =
            s.ts.with_minute(0)
                .and_then(|dt| dt.with_second(0))
                .and_then(|dt| dt.with_nanosecond(0))
                .expect("valid hour truncation");

        // Because we iterate in chronological order,
        // inserting again for the same hour will overwrite with the *latest* sample.
        buckets.insert(
            hour_start,
            Sample {
                ts: s.ts,
                price: s.price,
            },
        );
    }

    buckets.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32, price: f64) -> Sample {
        let ts = Utc
            .with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("valid datetime");
        Sample { ts, price }
    }

    #[test]
    fn test_resample_to_hourly_empty_input_returns_empty_vec() {
        let out = resample_to_hourly(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn test_resample_to_hourly_single_sample_is_preserved() {
        let s = sample(2025, 11, 28, 10, 15, 0, 100.0);
        let out = resample_to_hourly(&[s.clone()]);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ts, s.ts);
        assert_eq!(out[0].price, s.price);
    }

    #[test]
    fn test_resample_to_hourly_multiple_samples_in_same_hour_keep_last_price_and_timestamp() {
        // All in the 10:00–10:59 hour
        let s1 = sample(2025, 11, 28, 10, 05, 00, 100.0);
        let s2 = sample(2025, 11, 28, 10, 30, 00, 101.0);
        let s3 = sample(2025, 11, 28, 10, 59, 59, 102.0);

        let samples = vec![s1, s2, s3.clone()];
        let out = resample_to_hourly(&samples);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ts, s3.ts); // original timestamp of last tick in that hour
        assert_eq!(out[0].price, 102.0); // close price
    }

    #[test]
    fn test_resample_to_hourly_multiple_hours_keep_last_sample_per_hour_and_order_by_hour() {
        // Hour 10
        let h10_early = sample(2025, 11, 28, 10, 05, 00, 100.0);
        let h10_last = sample(2025, 11, 28, 10, 55, 00, 101.0);

        // Hour 11
        let h11_early = sample(2025, 11, 28, 11, 00, 00, 200.0);
        let h11_last = sample(2025, 11, 28, 11, 45, 00, 201.0);

        let samples = vec![h10_early, h10_last.clone(), h11_early, h11_last.clone()];
        let out = resample_to_hourly(&samples);

        assert_eq!(out.len(), 2);

        // First bucket: 10:00–10:59, last tick in that hour
        assert_eq!(out[0].ts, h10_last.ts);
        assert_eq!(out[0].price, 101.0);

        // Second bucket: 11:00–11:59, last tick in that hour
        assert_eq!(out[1].ts, h11_last.ts);
        assert_eq!(out[1].price, 201.0);
    }

    #[test]
    fn test_resample_to_hourly_amples_exactly_on_hour_boundary_form_separate_buckets() {
        // 10:00:00 and 10:30 in same hour
        let h10_start = sample(2025, 11, 28, 10, 00, 00, 100.0);
        let h10_mid = sample(2025, 11, 28, 10, 30, 00, 101.0);

        // 11:00:00 – new (exact) hour, its own bucket
        let h11_start = sample(2025, 11, 28, 11, 00, 00, 200.0);

        let samples = vec![h10_start, h10_mid.clone(), h11_start.clone()];
        let out = resample_to_hourly(&samples);

        assert_eq!(out.len(), 2);

        // Hour 10: we should keep the last sample in that hour (10:30)
        assert_eq!(out[0].ts, h10_mid.ts);
        assert_eq!(out[0].price, 101.0);

        // Hour 11: only one sample, kept as-is
        assert_eq!(out[1].ts, h11_start.ts);
        assert_eq!(out[1].price, 200.0);
    }
}
