# Quotes

Lightweight price snapshot used in Discord quote commands.

Model: `PriceQuote`
- `symbol` (String): Ticker symbol.
- `name` (String): Company or asset name.
- `price` (Option<f64>): Last traded price.
- `currency` (Option<String>): Quoting currency (e.g., `USD`).
- `change` (Option<f64>): Absolute change vs. previous close.
- `percent_change` (Option<f64>): Percent change vs. previous close.
- `pre_market_price` (Option<f64>): Latest pre-market trade, if offered.
- `after_hours_price` (Option<f64>): Latest after-hours trade, if offered.

Example payload:
```json
{
  "symbol": "MSFT",
  "name": "Microsoft Corporation",
  "price": 409.12,
  "currency": "USD",
  "change": 3.21,
  "percent_change": 0.79,
  "after_hours_price": 408.50
}
```
