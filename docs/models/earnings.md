# Earnings

Discord-facing representation of an earnings event from external finance APIs.

Model: `EarningsEvent`
- `symbol` (String): Ticker the event belongs to.
- `date` (DateTime<Utc>): Start date for the event window.
- `date_end` (Option<DateTime<Utc>>): End date for multi-day events.
- `time_of_day` (Option<String>): Session hint such as `BMO` (before market open) or `AMC`.
- `eps_estimate` / `eps_actual` (Option<f64>): EPS numbers when available.
- `revenue_estimate` / `revenue_actual` (Option<f64>): Revenue in the provider’s units.
- `importance` (Option<i64>): Provider-defined importance score.
- `title` (Option<String>): Human-friendly headline used in embeds.
- `emoji` (Option<String>): Short emoji marker for quick scanning.
- `logo` (Option<String>): Base64-encoded logo from the API for richer cards.

Example payload:
```json
{
  "symbol": "AAPL",
  "date": "2024-10-26T00:00:00Z",
  "time_of_day": "AMC",
  "eps_estimate": 1.39,
  "eps_actual": 1.46,
  "revenue_estimate": 89500000000,
  "revenue_actual": 90700000000,
  "importance": 90,
  "title": "Apple Q4 Earnings",
  "emoji": "🍏"
}
```
