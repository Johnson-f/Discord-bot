use chrono::{Datelike, NaiveDate, Utc};
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, CreateCommand,
    CreateCommandOption,
};

use crate::models::{Frequency, StatementType};
use crate::service::finance::{
    fundamentals::{reshape_timeseries_to_financial_statements, FETCH_YEARS_DEFAULT},
    FinanceService,
};

#[derive(Debug, Clone)]
struct MetricSpec {
    slash_value: String,
    field_key: String,
    label: String,
}

fn get_metrics_for_statement(statement_type: StatementType) -> Vec<MetricSpec> {
    use finance_query_core::utils::financials_constants::{
        BALANCE_SHEET_FIELDS, CASH_FLOW_FIELDS, INCOME_STATEMENT_FIELDS,
    };

    let fields = match statement_type {
        StatementType::IncomeStatement => INCOME_STATEMENT_FIELDS,
        StatementType::BalanceSheet => BALANCE_SHEET_FIELDS,
        StatementType::CashFlow => CASH_FLOW_FIELDS,
    };

    fields
        .iter()
        .map(|field| MetricSpec {
            slash_value: to_snake(field),
            field_key: field.to_string(),
            label: to_title(field),
        })
        .collect()
}

fn to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 5);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn to_title(name: &str) -> String {
    to_snake(name)
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), c.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn register_command(statement_type: StatementType) -> CreateCommand {
    let (cmd_name, description) = match statement_type {
        StatementType::IncomeStatement => (
            "income",
            "Get income statement metrics (annual or quarterly)",
        ),
        StatementType::BalanceSheet => {
            ("balance", "Get balance sheet metrics (annual or quarterly)")
        }
        StatementType::CashFlow => ("cashflow", "Get cash flow metrics (annual or quarterly)"),
    };

    let metrics = get_metrics_for_statement(statement_type);

    CreateCommand::new(cmd_name)
        .description(description)
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "ticker",
                "Ticker symbol, e.g., AAPL",
            )
            .required(true),
        )
        .add_option({
            let mut opt = CreateCommandOption::new(
                CommandOptionType::String,
                "metric",
                "Which metric to fetch",
            )
            .required(true);

            // Limit to 25 choices (Discord's max)
            for m in metrics.iter().take(25) {
                opt = opt.add_string_choice(m.label.clone(), m.slash_value.clone());
            }
            opt
        })
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "freq", "annual or quarterly")
                .required(true)
                .add_string_choice("Annual", "annual")
                .add_string_choice("Quarterly", "quarterly"),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "year",
                "Filter by year (optional)",
            )
            .min_int_value(1990),
        )
        .add_option({
            CreateCommandOption::new(
                CommandOptionType::String,
                "quarter",
                "Quarter (Q1-Q4, only with quarterly)",
            )
            .add_string_choice("Q1", "Q1")
            .add_string_choice("Q2", "Q2")
            .add_string_choice("Q3", "Q3")
            .add_string_choice("Q4", "Q4")
        })
}

pub async fn handle(
    command: &CommandInteraction,
    finance: &FinanceService,
) -> Result<String, String> {
    let ticker = get_str_opt(command, "ticker").ok_or("ticker is required")?;
    let metric_val = get_str_opt(command, "metric").ok_or("metric is required")?;
    let freq_val = get_str_opt(command, "freq").ok_or("freq is required")?;
    let year = get_i64_opt(command, "year").map(|v| v as i32);
    let quarter = get_str_opt(command, "quarter");

    // Determine statement type from command name
    let statement_type = match command.data.name.as_str() {
        "income" => StatementType::IncomeStatement,
        "balance" => StatementType::BalanceSheet,
        "cashflow" => StatementType::CashFlow,
        _ => return Err("unknown command".into()),
    };

    let metrics = get_metrics_for_statement(statement_type);
    let metric = metrics
        .iter()
        .find(|m| m.slash_value == metric_val)
        .ok_or("unknown metric")?;

    let freq = match freq_val {
        "annual" => Frequency::Annual,
        "quarterly" => Frequency::Quarterly,
        _ => return Err("freq must be annual or quarterly".into()),
    };

    if freq == Frequency::Annual && quarter.is_some() {
        return Err("quarter can only be used with quarterly frequency".into());
    }

    let quarter_num = quarter.and_then(|q| match q {
        "Q1" => Some(1),
        "Q2" => Some(2),
        "Q3" => Some(3),
        "Q4" => Some(4),
        _ => None,
    });

    let years_back = year
        .map(|y| {
            let current_year = Utc::now().year();
            (current_year - y + 1).max(FETCH_YEARS_DEFAULT as i32) as i64
        })
        .unwrap_or(FETCH_YEARS_DEFAULT);

    let raw = finance
        .get_fundamentals_raw(ticker, statement_type, freq, years_back)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    let statements = reshape_timeseries_to_financial_statements(&raw);
    let selected = select_metric(
        &statements,
        statement_type,
        freq,
        &metric.field_key,
        year,
        quarter_num,
    )
    .ok_or_else(|| "no matching data for the requested filters".to_string())?;

    let (date, display, raw_num) = selected;
    let quarter_text = quarter.map(|q| format!("{q} ")).unwrap_or_default();

    let freq_label = match freq {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let mut response = format!(
        "{} ({}) for {} {}on {}: {}",
        metric.label,
        freq_label,
        ticker.to_uppercase(),
        quarter_text,
        date,
        display
    );

    if let Some(raw) = raw_num {
        response.push_str(&format!(" (raw: {:.2})", raw));
    }

    Ok(response)
}

fn select_metric(
    statements: &[crate::models::FinancialStatement],
    statement_type: StatementType,
    frequency: Frequency,
    metric: &str,
    year: Option<i32>,
    quarter: Option<u32>,
) -> Option<(String, String, Option<f64>)> {
    let freq_str = match frequency {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let stmt = statements
        .iter()
        .find(|s| s.statement_type == statement_type.as_str() && s.frequency == freq_str)?;

    let metric_map = stmt.statement.get(metric)?;

    let mut best: Option<(NaiveDate, String, Option<f64>, String)> = None;

    for (date_str, val) in metric_map {
        if let Ok(nd) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            if let Some(y) = year {
                if nd.year() != y {
                    continue;
                }
            }
            if let Some(q) = quarter {
                let month = nd.month();
                let q_calc = ((month - 1) / 3) + 1;
                if q_calc != q {
                    continue;
                }
            }

            let (display, raw_num) = extract_display(val);

            match &best {
                Some((best_date, _, _, _)) if nd <= *best_date => {}
                _ => best = Some((nd, display, raw_num, date_str.clone())),
            }
        }
    }

    best.map(|(_, display, raw, date)| (date, display, raw))
}

fn extract_display(val: &serde_json::Value) -> (String, Option<f64>) {
    let raw_num = val
        .get("reportedValue")
        .and_then(|rv| rv.get("raw"))
        .and_then(|r| r.as_f64())
        .or_else(|| val.get("raw").and_then(|r| r.as_f64()));

    let fmt = val
        .get("reportedValue")
        .and_then(|rv| rv.get("fmt"))
        .and_then(|f| f.as_str())
        .map(|s| s.to_string());

    let display = fmt.unwrap_or_else(|| {
        raw_num
            .map(|n| format!("{n:.2}"))
            .unwrap_or_else(|| "n/a".to_string())
    });

    (display, raw_num)
}

fn get_str_opt<'a>(command: &'a CommandInteraction, name: &str) -> Option<&'a str> {
    command
        .data
        .options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            CommandDataOptionValue::String(ref s) => Some(s.as_str()),
            _ => None,
        })
}

fn get_i64_opt(command: &CommandInteraction, name: &str) -> Option<i64> {
    command
        .data
        .options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            CommandDataOptionValue::Integer(v) => Some(v),
            _ => None,
        })
}
