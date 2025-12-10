# Fundamentals

Financial statement structures mirrored from `finance-query-core` for bot output. The bot formats numeric values to billions (suffix `B`) and does not return raw/unformatted numbers in responses.

Enums
- `StatementType`: `IncomeStatement` | `BalanceSheet` | `CashFlow` (snake_case serialization).
- `Frequency`: `Annual` | `Quarterly` (snake_case serialization).

Models
- `FinancialStatement`: Raw timeseries response
  - `symbol` (String)
  - `statement_type` (String): Matches `StatementType::as_str()`.
  - `frequency` (String): Matches `Frequency::as_str()`.
  - `statement` (HashMap<String, HashMap<String, serde_json::Value>>): Metric name → period → value map.
- `FinancialSummary`: Bot-facing snapshot
  - `symbol` (String)
  - `revenue` (Option<f64>)
  - `eps` (Option<f64>)
  - `pe_ratio` (Option<f64>)
  - `market_cap` (Option<f64>)
  - `currency` (Option<String>)

Example `FinancialSummary` (values shown in billions by the bot):
```json
{
  "symbol": "GOOGL",
  "revenue": "84.00B",
  "eps": "0.00B",
  "pe_ratio": "0.00B",
  "market_cap": "1920.00B",
  "currency": "USD"
}
```

Example `FinancialStatement.statement` shape (trimmed):
```json
{
  "total_revenue": {
    "2023-12-31": { "raw": 84000000000 },
    "2022-12-31": { "raw": 76000000000 }
  }
}
```
