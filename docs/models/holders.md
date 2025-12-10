# Holders

Aggregated ownership and insider data exposed to the bot.

Model: `HoldersOverview`
- `symbol` (String): Ticker symbol.
- `major_breakdown` (Option<MajorHoldersBreakdown>): High-level percentages (insiders, institutions, float, etc.).
- `institutional_holders` (Option<Vec<InstitutionalHolder>>): Institution positions and sizes.
- `mutualfund_holders` (Option<Vec<MutualFundHolder>>): Mutual fund positions.
- `insider_transactions` (Option<Vec<InsiderTransaction>>): Recent insider buys/sells.
- `insider_purchases` (Option<InsiderPurchase>): Aggregated insider purchase summary.
- `insider_roster` (Option<Vec<InsiderRosterMember>>): Current insider roster details.

Related exports (from `finance_query_core`):
- `HolderType`, `InstitutionalHolder`, `MutualFundHolder`, `InsiderTransaction`, `InsiderPurchase`, `InsiderRosterMember`, `MajorHoldersBreakdown`.

Helper utilities:
- `parse_timestamp(serde_json::Value) -> Option<DateTime<Utc>>`: Converts `{"raw": <unix_seconds>}` to a UTC timestamp.
- `value_to_i64` / `value_to_f64`: Extracts numeric `raw` fields or direct numbers.
- `object_to_map`: Clones a JSON object into a `HashMap<String, Value>` for ergonomic access.

Example payload (trimmed):
```json
{
  "symbol": "AMZN",
  "major_breakdown": {
    "held_by_insiders": 0.09,
    "held_by_institutions": 0.61
  },
  "institutional_holders": [
    { "organization": "Vanguard", "position": 78500000, "pct_held": 7.0 }
  ],
  "insider_transactions": [
    { "insider": "Jassy Andrew", "transaction_text": "Sale", "shares": 5000 }
  ]
}
```
