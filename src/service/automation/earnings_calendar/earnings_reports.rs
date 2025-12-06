// This file will contain the logic to return companies that have already ready earnings & their 
// financials are currently available on the crate 
use::std::{collections::HashMap, sync::Arc};
use crate::service::finance::FinanceService;
use crate::models::EarningsEvent;

struct EarningsReport {
    symbol: String,
    earnings_date: NaiveDate,
    earnings_time: String,
    earnings_report: String,
    earnings_beat: String,
    earnings_surprise: String,
    earnings_surprise_percentage: String,
    earnings_surprise_percentage_percentage: String,
    earnings_surprise_percentage_percentage_percentage: String,
    earnings_surprise_percentage_percentage_percentage_percentage: String,
}