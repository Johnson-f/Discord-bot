use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, CreateCommand,
    CreateCommandOption,
};

use crate::models::{
    HolderType, InsiderPurchase, InsiderRosterMember, InsiderTransaction, InstitutionalHolder,
    MutualFundHolder,
};
use crate::service::finance::FinanceService;

pub fn register_command() -> CreateCommand {
    CreateCommand::new("holders")
        .description("Show holders information for a ticker")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "ticker",
                "Ticker symbol, e.g., AAPL",
            )
            .required(true),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "type", "Holder category")
                .add_string_choice("Major", "major")
                .add_string_choice("Institutional", "institutional")
                .add_string_choice("Mutual Fund", "mutualfund")
                .add_string_choice("Insider Transactions", "insider_transactions")
                .add_string_choice("Insider Purchases (summary)", "insider_purchases")
                .add_string_choice("Insider Roster", "insider_roster")
                .required(true),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "limit",
                "Rows to show (default 5)",
            )
            .min_int_value(1)
            .max_int_value(10),
        )
}

pub async fn handle(
    command: &CommandInteraction,
    finance: &FinanceService,
) -> Result<String, String> {
    let ticker = get_str_opt(command, "ticker").ok_or("ticker is required")?;
    let holder_type_raw = get_str_opt(command, "type").ok_or("type is required")?;
    let limit = get_int_opt(command, "limit").map(|v| v as usize);
    handle_text(finance, ticker, holder_type_raw, limit).await
}

pub async fn handle_text(
    finance: &FinanceService,
    ticker: &str,
    holder_type_raw: &str,
    limit: Option<usize>,
) -> Result<String, String> {
    let holder_type = match holder_type_raw {
        "major" => HolderType::Major,
        "institutional" => HolderType::Institutional,
        "mutualfund" => HolderType::MutualFund,
        "insider_transactions" => HolderType::InsiderTransactions,
        "insider_purchases" => HolderType::InsiderPurchases,
        "insider_roster" => HolderType::InsiderRoster,
        _ => {
            return Err(
                "type must be major | institutional | mutualfund | insider_transactions | insider_purchases | insider_roster"
                    .into(),
            )
        }
    };
    let limit = limit.unwrap_or(5).clamp(1, 10) as usize;

    let data = finance
        .get_holders(ticker, holder_type)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    match holder_type {
        HolderType::Major => format_major(&data).ok_or_else(|| "no major holders found".into()),
        HolderType::Institutional => format_table(
            &data.institutional_holders.unwrap_or_default(),
            limit,
            "Institutional holders",
            &data.symbol,
        ),
        HolderType::MutualFund => format_table(
            &data.mutualfund_holders.unwrap_or_default(),
            limit,
            "Mutual fund holders",
            &data.symbol,
        ),
        HolderType::InsiderTransactions => format_transactions(
            &data.insider_transactions.unwrap_or_default(),
            limit,
            &data.symbol,
        ),
        HolderType::InsiderPurchases => {
            format_purchases(data.insider_purchases.as_ref(), &data.symbol)
        }
        HolderType::InsiderRoster => format_roster(
            &data.insider_roster.unwrap_or_default(),
            limit,
            &data.symbol,
        ),
    }
}

fn format_major(data: &crate::models::HoldersOverview) -> Option<String> {
    let breakdown = data.major_breakdown.as_ref()?;
    let mut parts = Vec::new();
    for (k, v) in breakdown.breakdown_data.iter() {
        let numeric = v
            .get("raw")
            .and_then(|r| r.as_f64())
            .or_else(|| v.get("raw").and_then(|r| r.as_i64()).map(|i| i as f64))
            .or_else(|| v.as_f64())
            .or_else(|| v.as_i64().map(|i| i as f64));

        if let Some(num) = numeric {
            parts.push(format!("{}: {}", k, format_percent(num)));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!(
            "Major holders for {} | {}",
            data.symbol,
            parts.join(" | ")
        ))
    }
}

fn format_table<T>(rows: &[T], limit: usize, heading: &str, symbol: &str) -> Result<String, String>
where
    T: HolderRow,
{
    if rows.is_empty() {
        return Err(format!("no {} found", heading.to_lowercase()));
    }

    let mut rows_sorted: Vec<&T> = rows.iter().collect();
    rows_sorted.sort_by_key(|r| -r.shares());

    let mut lines = Vec::new();
    lines.push(format!("{} for {}", heading, symbol));
    for row in rows_sorted.iter().take(limit) {
        lines.push(format!(
            "{} — shares: {}, %out: {}, reported: {}",
            row.name(),
            format_shares(row.shares()),
            row.percent_out()
                .map(format_percent)
                .unwrap_or_else(|| "n/a".to_string()),
            row.date_reported()
        ));
    }

    Ok(lines.join("\n"))
}

trait HolderRow {
    fn name(&self) -> String;
    fn shares(&self) -> i64;
    fn percent_out(&self) -> Option<f64>;
    fn date_reported(&self) -> String;
}

impl HolderRow for InstitutionalHolder {
    fn name(&self) -> String {
        self.holder.clone()
    }
    fn shares(&self) -> i64 {
        self.shares
    }
    fn percent_out(&self) -> Option<f64> {
        self.percent_out
    }
    fn date_reported(&self) -> String {
        self.date_reported.date_naive().to_string()
    }
}

impl HolderRow for MutualFundHolder {
    fn name(&self) -> String {
        self.holder.clone()
    }
    fn shares(&self) -> i64 {
        self.shares
    }
    fn percent_out(&self) -> Option<f64> {
        self.percent_out
    }
    fn date_reported(&self) -> String {
        self.date_reported.date_naive().to_string()
    }
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

fn get_int_opt(command: &CommandInteraction, name: &str) -> Option<i64> {
    command
        .data
        .options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            CommandDataOptionValue::Integer(i) => Some(i),
            _ => None,
        })
}

fn format_percent(value: f64) -> String {
    let pct = if value.abs() <= 1.0 {
        value * 100.0
    } else {
        value
    };
    format!("{:.2}%", pct)
}

fn format_shares(shares: i64) -> String {
    if shares.abs() >= 1_000_000_000 {
        format!("{:.2}B", shares as f64 / 1_000_000_000.0)
    } else if shares.abs() >= 1_000_000 {
        format!("{:.2}M", shares as f64 / 1_000_000.0)
    } else {
        shares.to_string()
    }
}

fn format_currency(value: i64) -> String {
    let abs = value.abs() as f64;
    if abs >= 1_000_000_000.0 {
        format!("${:.2}B", value as f64 / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("${:.2}M", value as f64 / 1_000_000.0)
    } else {
        format!("${}", value)
    }
}

fn format_transactions(
    txs: &[InsiderTransaction],
    limit: usize,
    symbol: &str,
) -> Result<String, String> {
    if txs.is_empty() {
        return Err("no insider transactions found".into());
    }
    let mut lines = Vec::new();
    lines.push(format!("Insider transactions for {}", symbol));
    for tx in txs.iter().take(limit) {
        lines.push(format!(
            "{} ({}) — {} | shares: {} | value: {} | date: {}",
            tx.insider,
            tx.position,
            tx.transaction,
            tx.shares
                .map(format_shares)
                .unwrap_or_else(|| "n/a".into()),
            tx.value
                .map(format_currency)
                .unwrap_or_else(|| "n/a".into()),
            tx.start_date.date_naive()
        ));
    }
    Ok(lines.join("\n"))
}

fn format_purchases(p: Option<&InsiderPurchase>, symbol: &str) -> Result<String, String> {
    let p = p.ok_or("no insider purchase summary found")?;
    Ok(format!(
        "Insider purchases (recent) for {}\nBuys: {} shares in {} transactions\nSells: {} shares in {} transactions\nNet shares: {}",
        symbol,
        p.purchases_shares.map(format_shares).unwrap_or_else(|| "n/a".into()),
        p.purchases_transactions.unwrap_or(0),
        p.sales_shares.map(format_shares).unwrap_or_else(|| "n/a".into()),
        p.sales_transactions.unwrap_or(0),
        p.net_shares.map(format_shares).unwrap_or_else(|| "n/a".into()),
    ))
}

fn format_roster(
    rows: &[InsiderRosterMember],
    limit: usize,
    symbol: &str,
) -> Result<String, String> {
    if rows.is_empty() {
        return Err("no insider roster found".into());
    }
    let mut lines = Vec::new();
    lines.push(format!("Insider roster for {}", symbol));
    for row in rows.iter().take(limit) {
        lines.push(format!(
            "{} — {} | last txn: {} ({}) | direct: {} | indirect: {}",
            row.name,
            row.position,
            row.most_recent_transaction
                .clone()
                .unwrap_or_else(|| "n/a".into()),
            row.latest_transaction_date
                .map(|d| d.date_naive().to_string())
                .unwrap_or_else(|| "n/a".into()),
            row.shares_owned_directly
                .map(format_shares)
                .unwrap_or_else(|| "n/a".into()),
            row.shares_owned_indirectly
                .map(format_shares)
                .unwrap_or_else(|| "n/a".into()),
        ));
    }
    Ok(lines.join("\n"))
}
