# SMA Analyzer

A small command-line tool that reads a CSV file containing:

```csv
timestamp,price
```

and analyzes it using SMA20, SMA50, and crossover logic.

The tool detects:

* Golden Cross (bullish crossover)
* Death Cross (bearish crossover)
* Trend bias (long/short)
* A suggested action: BUY, SELL, HOLD / LONG BIAS, HOLD / SHORT BIAS, or HOLD.

**Important note:** The tool is meant to be used for analysis only, not real trading.

## How it works

### Input format

CSV must contain at least:

```csv
timestamp,price
2025-11-22T10:00:00Z,70234.12
2025-11-22T10:01:00Z,70236.55
...
```

At least 51 rows are required to compute:

* previous SMA50
* current SMA50
* valid crossover detection

#### Running the application

```bash
cargo run -- --input path/to/bitcoin_usd.csv
```

#### Output example

```bash
Last timestamp: 2025-11-22T10:30:00Z
Last price:     70234.1200
SMA(20):        70180.5500
SMA(50):        69990.3200
Prev SMA(20):   70170.4400
Prev SMA(50):   69980.1100
Suggestion:     BUY
Reason:         Golden Cross + SMA50 rising + price above SMA20 & SMA50
```
