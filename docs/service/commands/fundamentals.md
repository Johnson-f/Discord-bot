# /income, /balance, /cashflow

Fetch fundamentals as text (slash) or as an image (mention).

Usage
- Slash: `/income|/balance|/cashflow ticker:<symbol> metric:<choice> freq:<annual|quarterly> [year] [quarter]`
- Mention (image): `@Bot income|balance|cashflow TICKER FREQ [YEAR] [QUARTER]`

Behavior
- Slash: pick a single metric (first 25 exposed as choices), auto-normalized if slightly off.
- Mention: renders an image of up to 40 metrics for the selected period (latest matching date), no metric argument needed.
- `freq` must match `annual` or `quarterly`; invalid values default to `annual`.
- `quarter` only applies to `quarterly`; ignored for `annual`.
- Metric names are normalized (case-insensitive, partials) when provided (slash).
- Values are formatted to billions in outputs.

Output
- Slash: `Label (freq) for TICKER [Qx ]on YYYY-MM-DD: VALUE`
- Mention: PNG attachment listing metrics and values for the period.

