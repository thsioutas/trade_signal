# SMA Analyzer

A small command-line tool that reads a CSV file containing:

```csv
timestamp,price
```

and produces a trading signal based on:

* Breakout above recent high in an uptrend
* Breakout below recent low in a downtrend
* Pullback to SMA(short) + bounce (uptrend)
* Pullback to SMA(short) + rejection (downtrend)
* Golden Cross (bullish crossover)
* Death Cross (bearish crossover)
* Trend bias (long/short)

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

At least 51 rows (hour samples) are required to compute:

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

## TODOs

Implement:

* Improve volatility filter

* Higher timeframe trend confirmation (eliminate fake breakouts)
  * Only BUY when dainly SMA50 rising
  * Only SELL when dainly SMA50 failing
* Stop-loss and take-profit targets

  ```bash
  stop = last_price - 2 × ATR
  take_profit = last_price + 4 × ATR
  ```

* Volatility-based position sizing

  ``` bash
  buy_fraction = risk_per_trade / ATR
  ```

* "No-trade zone" filter

  ```bash
  if abs(SMA20 - SMA50) / price < 1%:
      do not trade (no trend)
  ```

* Optimize the lookbacks
