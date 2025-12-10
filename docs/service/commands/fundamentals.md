# /income, /balance, /cashflow

Fetch a single financial metric from fundamentals time series.

Usage
- Slash: `/income|/balance|/cashflow ticker:<symbol> metric:<choice> freq:<annual|quarterly> [year] [quarter]`
- Mention: `@Bot income|balance|cashflow TICKER METRIC FREQ [YEAR] [QUARTER]`

Behavior
- Metrics offered per statement type (first 25 exposed as slash choices).
- `freq` must match `annual` or `quarterly`; invalid values default to `annual`.
- `quarter` is only valid with `quarterly`; if provided with `annual`, it is ignored.
- Optional `year` filter; for quarterly, optional `quarter` filter (`Q1`–`Q4`).
- Metric names are auto-normalized (case-insensitive, allows partials) to the closest known metric.
- Picks the latest matching datapoint; formats numbers to billions (bot-side).

Output
- `Label (freq) for TICKER [Qx ]on YYYY-MM-DD: VALUE`
- Returns error strings for unknown metric/freq or missing data.

