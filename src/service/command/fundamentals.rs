use chrono::{Datelike, NaiveDate, Utc};
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, CreateCommand,
    CreateCommandOption,
};

use ab_glyph::{FontArc, PxScale};
use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;
use image::{ImageFormat, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use std::io::Cursor;

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

fn normalize_metric_value(
    statement_type: StatementType,
    raw: &str,
) -> Result<(MetricSpec, bool), String> {
    let metrics = get_metrics_for_statement(statement_type);
    let norm = raw.trim().to_ascii_lowercase().replace(' ', "_");

    // Exact / common-case matches first
    if let Some(m) = metrics.iter().find(|m| m.slash_value == norm) {
        return Ok((m.clone(), false));
    }
    if let Some(m) = metrics
        .iter()
        .find(|m| m.field_key.to_ascii_lowercase() == norm)
    {
        return Ok((m.clone(), true));
    }
    if let Some(m) = metrics
        .iter()
        .find(|m| m.label.to_ascii_lowercase() == norm.replace('_', " "))
    {
        return Ok((m.clone(), true));
    }

    // Prefix/substring heuristic
    if let Some(m) = metrics.iter().find(|m| {
        m.slash_value.starts_with(&norm)
            || m.field_key.to_ascii_lowercase().starts_with(&norm)
            || m.label.to_ascii_lowercase().starts_with(&norm.replace('_', " "))
    }) {
        return Ok((m.clone(), true));
    }
    if let Some(m) = metrics.iter().find(|m| {
        m.slash_value.contains(&norm)
            || m.field_key.to_ascii_lowercase().contains(&norm)
            || m.label.to_ascii_lowercase().contains(&norm.replace('_', " "))
    }) {
        return Ok((m.clone(), true));
    }

    Err("unknown metric".into())
}

fn normalize_freq(raw: &str) -> (Frequency, bool) {
    let norm = raw.trim().to_ascii_lowercase();
    match norm.as_str() {
        "annual" => (Frequency::Annual, false),
        "quarterly" => (Frequency::Quarterly, false),
        _ => (Frequency::Annual, true), // default to annual if unrecognized
    }
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

    handle_text(
        finance,
        statement_type,
        ticker,
        metric_val,
        freq_val,
        year,
        quarter,
    )
    .await
}

pub async fn handle_text(
    finance: &FinanceService,
    statement_type: StatementType,
    ticker: &str,
    metric_val: &str,
    freq_val: &str,
    year: Option<i32>,
    quarter: Option<&str>,
) -> Result<String, String> {
    let mut corrections = Vec::new();

    let (metric, metric_corrected) =
        normalize_metric_value(statement_type, metric_val).map_err(|_| {
            format!(
                "unknown metric '{}'; try one of the slash choices for this statement",
                metric_val
            )
        })?;
    if metric_corrected {
        corrections.push(format!("metric→{}", metric.slash_value));
    }

    let (freq, freq_corrected) = normalize_freq(freq_val);
    if freq_corrected {
        corrections.push(format!("freq→{}", match freq { Frequency::Annual => "annual", Frequency::Quarterly => "quarterly", }));
    }

    let quarter_num = quarter.and_then(|q| match q {
        "Q1" => Some(1),
        "Q2" => Some(2),
        "Q3" => Some(3),
        "Q4" => Some(4),
        _ => None,
    });
    let quarter_num = match freq {
        Frequency::Annual => {
            if quarter.is_some() {
                corrections.push("ignored quarter for annual freq".to_string());
            }
            None
        }
        Frequency::Quarterly => quarter_num,
    };

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

    let (date, display) = selected;
    let quarter_text = quarter.map(|q| format!("{q} ")).unwrap_or_default();

    let freq_label = match freq {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let response = format!(
        "{} ({}) for {} {}on {}: {}",
        metric.label,
        freq_label,
        ticker.to_uppercase(),
        quarter_text,
        date,
        display
    );

    if corrections.is_empty() {
        Ok(response)
    } else {
        Ok(format!("{} (adjusted: {})", response, corrections.join(", ")))
    }
}

pub async fn render_statement_image(
    finance: &FinanceService,
    statement_type: StatementType,
    ticker: &str,
    freq_val: &str,
    year: Option<i32>,
    quarter: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    let (freq, _) = normalize_freq(freq_val);

    let quarter_num = quarter.and_then(|q| match q {
        "Q1" => Some(1),
        "Q2" => Some(2),
        "Q3" => Some(3),
        "Q4" => Some(4),
        _ => None,
    });
    let quarter_num = match freq {
        Frequency::Annual => None,
        Frequency::Quarterly => quarter_num,
    };

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
    let (date, rows) = select_statement_rows(
        &statements,
        statement_type,
        freq,
        year,
        quarter_num,
    )
    .ok_or_else(|| "no matching data for the requested filters".to_string())?;

    let freq_label = match freq {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let title = format!(
        "{} ({}) for {} on {}",
        match statement_type {
            StatementType::IncomeStatement => "Income Statement",
            StatementType::BalanceSheet => "Balance Sheet",
            StatementType::CashFlow => "Cash Flow",
        },
        freq_label,
        ticker.to_uppercase(),
        date
    );

    let image = render_rows_image(&title, &rows)?;
    Ok((title, image))
}

fn select_statement_rows(
    statements: &[crate::models::FinancialStatement],
    statement_type: StatementType,
    frequency: Frequency,
    year: Option<i32>,
    quarter: Option<u32>,
) -> Option<(String, Vec<(String, String)>)> {
    let freq_str = match frequency {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let stmt = statements
        .iter()
        .find(|s| s.statement_type == statement_type.as_str() && s.frequency == freq_str)?;

    // Choose the best date entry based on year/quarter filters
    let mut best: Option<(NaiveDate, String)> = None;
    for (date_str, _val) in stmt.statement.values().next()?.iter() {
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
            match &best {
                Some((best_date, _)) if nd <= *best_date => {}
                _ => best = Some((nd, date_str.clone())),
            }
        }
    }

    let (_, best_date) = best?;

    // Build rows for the selected date
    let mut rows = Vec::new();
    for (metric, series) in stmt.statement.iter() {
        if let Some(val) = series.get(&best_date) {
            let display = extract_display(val);
            rows.push((metric.clone(), display));
        }
    }

    // Sort rows alphabetically for consistency and cap to avoid huge images
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.truncate(40);

    Some((best_date, rows))
}

fn render_rows_image(title: &str, rows: &[(String, String)]) -> Result<Vec<u8>, String> {
    let font = load_font()?;
    let header_scale = PxScale::from(28.0);
    let row_scale = PxScale::from(20.0);

    let margin = 24;
    let line_h = 30;
    let width = 1200;
    let height = margin * 2 + 50 + rows.len() as u32 * line_h;

    let mut img = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));

    draw_text_mut(
        &mut img,
        Rgba([40, 40, 40, 255]),
        margin as i32,
        margin as i32,
        header_scale,
        &font,
        title,
    );

    let mut y = margin + 50;
    for (metric, value) in rows {
        let line = format!("{}: {}", to_title(metric), value);
        draw_text_mut(
            &mut img,
            Rgba([60, 60, 60, 255]),
            margin as i32,
            y as i32,
            row_scale,
            &font,
            &line,
        );
        y += line_h;
    }

    let mut buffer = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .map_err(|e| format!("failed to encode png: {e}"))?;

    Ok(buffer)
}

fn load_font() -> Result<FontArc, String> {
    let source = SystemSource::new();

    let handle = source
        .select_best_match(
            &[FamilyName::SansSerif],
            Properties::new().weight(Weight::BOLD),
        )
        .map_err(|e| format!("Failed to find system font: {}", e))?;

    let font = handle
        .load()
        .map_err(|e| format!("Failed to load font: {}", e))?;

    let font_data = font
        .copy_font_data()
        .ok_or_else(|| "Failed to copy font data".to_string())?
        .to_vec();

    FontArc::try_from_vec(font_data).map_err(|_| "Failed to create FontArc from system font".into())
}

fn select_metric(
    statements: &[crate::models::FinancialStatement],
    statement_type: StatementType,
    frequency: Frequency,
    metric: &str,
    year: Option<i32>,
    quarter: Option<u32>,
) -> Option<(String, String)> {
    let freq_str = match frequency {
        Frequency::Annual => "annual",
        Frequency::Quarterly => "quarterly",
    };

    let stmt = statements
        .iter()
        .find(|s| s.statement_type == statement_type.as_str() && s.frequency == freq_str)?;

    let metric_map = stmt.statement.get(metric)?;

    let mut best: Option<(NaiveDate, String, String)> = None;

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

            let display = extract_display(val);

            match &best {
                Some((best_date, _, _)) if nd <= *best_date => {}
                _ => best = Some((nd, display, date_str.clone())),
            }
        }
    }

    best.map(|(_, display, date)| (date, display))
}

fn extract_display(val: &serde_json::Value) -> String {
    if let Some(raw) = val
        .get("reportedValue")
        .and_then(|rv| rv.get("raw"))
        .and_then(|r| r.as_f64())
        .or_else(|| val.get("raw").and_then(|r| r.as_f64()))
    {
        return format!("{:.2}B", raw / 1_000_000_000.0);
    }

    val.get("reportedValue")
        .and_then(|rv| rv.get("fmt"))
        .and_then(|f| f.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "n/a".to_string())
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
