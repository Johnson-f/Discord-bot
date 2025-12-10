# Fundamentals

Financial statement structures mirrored from `finance-query-core` for bot output. The bot formats numeric values to billions (suffix `B`) and does not return raw/unformatted numbers in responses.

Enums
- `StatementType`: `IncomeStatement` | `BalanceSheet` | `CashFlow` (snake_case serialization).
- `Frequency`: `Annual` | `Quarterly` (snake_case serialization).

Statement metrics (as exposed by `finance-query-core`)

- Income: `TotalRevenue`, `OperatingRevenue`, `CostOfRevenue`, `GrossProfit`, `OperatingExpense`, `SellingGeneralAndAdministration`, `ResearchAndDevelopment`, `OperatingIncome`, `NetInterestIncome`, `InterestExpense`, `InterestIncome`, `NetNonOperatingInterestIncomeExpense`, `OtherIncomeExpense`, `PretaxIncome`, `TaxProvision`, `NetIncomeCommonStockholders`, `NetIncome`, `DilutedEPS`, `BasicEPS`, `DilutedAverageShares`, `BasicAverageShares`, `EBIT`, `EBITDA`, `ReconciledCostOfRevenue`, `ReconciledDepreciation`, `NetIncomeFromContinuingOperationNetMinorityInterest`, `NormalizedEBITDA`, `TotalExpenses`, `TotalOperatingIncomeAsReported`

- Balance: `TotalAssets`, `CurrentAssets`, `CashCashEquivalentsAndShortTermInvestments`, `CashAndCashEquivalents`, `CashFinancial`, `Receivables`, `AccountsReceivable`, `Inventory`, `PrepaidAssets`, `OtherCurrentAssets`, `TotalNonCurrentAssets`, `NetPPE`, `GrossPPE`, `AccumulatedDepreciation`, `Goodwill`, `GoodwillAndOtherIntangibleAssets`, `OtherIntangibleAssets`, `InvestmentsAndAdvances`, `LongTermEquityInvestment`, `OtherNonCurrentAssets`, `TotalLiabilitiesNetMinorityInterest`, `CurrentLiabilities`, `PayablesAndAccruedExpenses`, `AccountsPayable`, `CurrentDebt`, `CurrentDeferredRevenue`, `OtherCurrentLiabilities`, `TotalNonCurrentLiabilitiesNetMinorityInterest`, `LongTermDebt`, `LongTermDebtAndCapitalLeaseObligation`, `NonCurrentDeferredRevenue`, `NonCurrentDeferredTaxesLiabilities`, `OtherNonCurrentLiabilities`, `StockholdersEquity`, `CommonStockEquity`, `CommonStock`, `RetainedEarnings`, `AdditionalPaidInCapital`, `TreasuryStock`, `TotalEquityGrossMinorityInterest`, `WorkingCapital`, `InvestedCapital`, `TangibleBookValue`, `TotalDebt`, `NetDebt`, `ShareIssued`, `OrdinarySharesNumber`

- Cashflow: `OperatingCashFlow`, `CashFlowFromContinuingOperatingActivities`, `NetIncomeFromContinuingOperations`, `DepreciationAndAmortization`, `DeferredIncomeTax`, `ChangeInWorkingCapital`, `ChangeInReceivables`, `ChangesInAccountReceivables`, `ChangeInInventory`, `ChangeInAccountPayable`, `ChangeInOtherWorkingCapital`, `StockBasedCompensation`, `OtherNonCashItems`, `InvestingCashFlow`, `CashFlowFromContinuingInvestingActivities`, `NetPPEPurchaseAndSale`, `PurchaseOfPPE`, `SaleOfPPE`, `CapitalExpenditure`, `NetBusinessPurchaseAndSale`, `PurchaseOfBusiness`, `SaleOfBusiness`, `NetInvestmentPurchaseAndSale`, `PurchaseOfInvestment`, `SaleOfInvestment`, `NetOtherInvestingChanges`, `FinancingCashFlow`, `CashFlowFromContinuingFinancingActivities`, `NetIssuancePaymentsOfDebt`, `NetLongTermDebtIssuance`, `LongTermDebtIssuance`, `LongTermDebtPayments`, `NetShortTermDebtIssuance`, `NetCommonStockIssuance`, `CommonStockIssuance`, `CommonStockPayments`, `RepurchaseOfCapitalStock`, `CashDividendsPaid`, `CommonStockDividendPaid`, `NetOtherFinancingCharges`, `EndCashPosition`, `BeginningCashPosition`, `ChangesinCash`, `EffectOfExchangeRateChanges`, `FreeCashFlow`, `CapitalExpenditureReported`

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
