use std::collections::BTreeMap;
use std::env;
use std::io::Cursor;
use std::sync::Arc;

use ab_glyph::{FontArc, PxScale};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use once_cell::sync::Lazy;
use serenity::all::{CreateAttachment, CreateMessage, Http};
use serenity::model::prelude::ChannelId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::models::EarningsEvent;
use crate::service::command::earnings::format_output;
use crate::service::finance::FinanceService;

static LAST_POST_DATE: Lazy<Mutex<Option<chrono::NaiveDate>>> = Lazy::new(|| Mutex::new(None));

/// Spawn a weekly earnings poster (Sunday at 5pm ET).
pub fn spawn_earnings_poster(
    http: Arc<Http>,
    finance: Arc<FinanceService>,
) -> Option<JoinHandle<()>> {
    if env::var("ENABLE_EARNINGS_PINGER")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        info!("Earnings poster disabled via ENABLE_EARNINGS_PINGER=0");
        return None;
    }

    let channel_id = match env::var("EARNINGS_CHANNEL_ID")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(id) => ChannelId::new(id),
        None => {
            info!("EARNINGS_CHANNEL_ID not set; earnings poster not started");
            return None;
        }
    };

    info!("Starting weekly earnings poster to channel {}", channel_id);

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if should_post_now().await {
                if let Err(e) = post_once(&http, &finance, channel_id).await {
                    error!("earnings poster iteration failed: {e}");
                }
            }
        }
    }))
}

async fn post_once(
    http: &Http,
    finance: &FinanceService,
    channel_id: ChannelId,
) -> Result<(), String> {
    let now_et = Utc::now().with_timezone(&New_York);
    let start = now_et.date_naive();
    let end = start + chrono::Duration::days(5); // Sunday through Friday
    let week_monday = start + chrono::Duration::days(1);
    let heading = format!("@everyone 📊 Earnings Calendar — Week of {}", week_monday);

    let events = finance
        .get_earnings_range(start, end)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    if events.is_empty() {
        info!("No earnings in next 7 days; skipping post");
        return Ok(());
    }

    match render_calendar_image(&events).await {
        Ok(bytes) => {
            let attachment = CreateAttachment::bytes(bytes, "earnings-calendar.png");
            channel_id
                .send_files(
                    http,
                    vec![attachment],
                    CreateMessage::new().content(heading.clone()),
                )
                .await
                .map_err(|e| format!("failed to post earnings calendar image: {e}"))?;
        }
        Err(render_err) => {
            warn!("Falling back to text earnings calendar: {}", render_err);
            let content = format!("{}\n\n{}", heading, format_output(&events));
            channel_id
                .say(http, content)
                .await
                .map_err(|e| format!("failed to post fallback earnings calendar: {e}"))?;
        }
    }

    Ok(())
}

async fn should_post_now() -> bool {
    let now_utc = Utc::now();
    let now_et = now_utc.with_timezone(&New_York);

    // Only on Sundays at 5:00 PM ET (allow a small window to avoid missing the minute)
    if now_et.weekday() != Weekday::Sun {
        return false;
    }
    if !(now_et.hour() == 17 && now_et.minute() < 5) {
        return false;
    }

    let today = now_et.date_naive();
    let mut last = LAST_POST_DATE.lock().await;
    if let Some(prev) = *last {
        if prev == today {
            return false;
        }
    }
    *last = Some(today);
    true
}

#[derive(Clone)]
struct DayColumn {
    date: NaiveDate,
    before: Vec<EarningsEvent>,
    after: Vec<EarningsEvent>,
}

enum Session {
    Before,
    After,
    Tba,
}

const HALF_WIDTH: u32 = 180;
const DAY_WIDTH: u32 = HALF_WIDTH * 2;
const ENTRY_HEIGHT: u32 = 85;
const HEADER_HEIGHT: u32 = 60;
const MARGIN: u32 = 20;
const DIVIDER_WIDTH: u32 = 2;

const TITLE_COLOR: Rgba<u8> = Rgba([40, 35, 30, 255]);
const HEADER_COLOR: Rgba<u8> = Rgba([100, 100, 100, 255]);
const CANVAS_BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const COLUMN_BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const ENTRY_BG: Rgba<u8> = Rgba([248, 248, 248, 255]);
const DIVIDER_COLOR: Rgba<u8> = Rgba([220, 220, 220, 255]);

const LOGO_W: u32 = 120;
const LOGO_H: u32 = 50;
const MAX_PER_COLUMN: usize = 12;

pub async fn render_calendar_image(events: &[EarningsEvent]) -> Result<Vec<u8>, String> {
    let columns = build_columns(events);
    if columns.is_empty() {
        return Err("no events to render".into());
    }

    let font = load_font()?;

    // Extract and decode logos from events
    let logos = extract_logos_from_events(events)?;

    let image = DynamicImage::ImageRgba8(draw_canvas(&columns, &font, &logos));
    let mut buffer = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .map_err(|e| format!("failed to encode png: {e}"))?;

    Ok(buffer)
}

fn extract_logos_from_events(
    events: &[EarningsEvent],
) -> Result<std::collections::HashMap<String, RgbaImage>, String> {
    use std::collections::HashMap;

    let mut logos = HashMap::new();

    for event in events {
        if let Some(ref logo_data) = event.logo {
            if let Some(base64_data) = logo_data.strip_prefix("data:image/png;base64,") {
                match STANDARD.decode(base64_data) {
                    Ok(decoded) => match image::load_from_memory(&decoded) {
                        Ok(img) => {
                            logos.insert(event.symbol.clone(), fit_logo(&img));
                        }
                        Err(e) => {
                            warn!("Failed to load logo for {}: {}", event.symbol, e);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to decode base64 logo for {}: {}", event.symbol, e);
                    }
                }
            }
        }
    }

    info!("Loaded {} logos from API", logos.len());
    Ok(logos)
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

    FontArc::try_from_vec(font_data)
        .map_err(|_| "Failed to create FontArc from system font".to_string())
}

fn fit_logo(img: &DynamicImage) -> RgbaImage {
    let (w, h) = img.dimensions();

    let scale = (LOGO_W as f32 / w as f32)
        .min(LOGO_H as f32 / h as f32)
        .min(1.5);

    let new_w = (w as f32 * scale).max(1.0).round() as u32;
    let new_h = (h as f32 * scale).max(1.0).round() as u32;
    let resized: RgbaImage = imageops::resize(img, new_w, new_h, FilterType::Lanczos3);

    let mut canvas = RgbaImage::from_pixel(LOGO_W, LOGO_H, Rgba([255, 255, 255, 0]));
    let x = ((LOGO_W - new_w) / 2) as i64;
    let y = ((LOGO_H - new_h) / 2) as i64;
    imageops::overlay(&mut canvas, &resized, x, y);
    canvas
}

fn build_columns(events: &[EarningsEvent]) -> Vec<DayColumn> {
    let mut grouped: BTreeMap<NaiveDate, DayColumn> = BTreeMap::new();

    for ev in events {
        let date = ev.date.date_naive();
        let entry = grouped.entry(date).or_insert_with(|| DayColumn {
            date,
            before: Vec::new(),
            after: Vec::new(),
        });

        match classify_session(ev.time_of_day.as_deref()) {
            Session::Before | Session::Tba => entry.before.push(ev.clone()),
            Session::After => entry.after.push(ev.clone()),
        }
    }

    grouped.into_values().take(5).collect()
}

fn draw_canvas(
    columns: &[DayColumn],
    font: &FontArc,
    logos: &std::collections::HashMap<String, RgbaImage>,
) -> RgbaImage {
    let count = columns.len() as u32;
    let width = count * (DAY_WIDTH + DIVIDER_WIDTH) + 2 * MARGIN - DIVIDER_WIDTH;

    let max_entries = columns
        .iter()
        .map(|c| c.before.len().max(c.after.len()))
        .max()
        .unwrap_or(0)
        .min(MAX_PER_COLUMN);

    let height = MARGIN + HEADER_HEIGHT + (max_entries as u32 * ENTRY_HEIGHT) + MARGIN;

    let mut img = RgbaImage::from_pixel(width, height, CANVAS_BG);

    for (idx, column) in columns.iter().enumerate() {
        let x = MARGIN + idx as u32 * (DAY_WIDTH + DIVIDER_WIDTH);
        draw_day_column(&mut img, x, MARGIN, column, font, logos);

        if idx < columns.len() - 1 {
            let divider_x = x + DAY_WIDTH;
            let divider_rect = Rect::at(divider_x as i32, MARGIN as i32)
                .of_size(DIVIDER_WIDTH, height - 2 * MARGIN);
            draw_filled_rect_mut(&mut img, divider_rect, DIVIDER_COLOR);
        }
    }

    img
}

fn draw_day_column(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    column: &DayColumn,
    font: &FontArc,
    logos: &std::collections::HashMap<String, RgbaImage>,
) {
    let max_entries = column
        .before
        .len()
        .max(column.after.len())
        .min(MAX_PER_COLUMN);
    let col_height = HEADER_HEIGHT + (max_entries as u32 * ENTRY_HEIGHT);
    let bg_rect = Rect::at(x as i32, y as i32).of_size(DAY_WIDTH, col_height);
    draw_filled_rect_mut(img, bg_rect, COLUMN_BG);

    let day_label = column.date.weekday().to_string();
    let date_label = column.date.format("%b %e").to_string();

    draw_centered_text(
        img,
        font,
        &day_label,
        PxScale::from(24.0),
        x,
        DAY_WIDTH,
        y + 8,
        TITLE_COLOR,
    );
    draw_centered_text(
        img,
        font,
        &date_label,
        PxScale::from(18.0),
        x,
        DAY_WIDTH,
        y + 35,
        HEADER_COLOR,
    );

    let header_y = y + HEADER_HEIGHT - 20;
    draw_centered_text(
        img,
        font,
        "Before Open",
        PxScale::from(14.0),
        x,
        HALF_WIDTH,
        header_y,
        HEADER_COLOR,
    );
    draw_centered_text(
        img,
        font,
        "After Close",
        PxScale::from(14.0),
        x + HALF_WIDTH,
        HALF_WIDTH,
        header_y,
        HEADER_COLOR,
    );

    let entry_start_y = y + HEADER_HEIGHT;
    draw_half_column(img, font, x, entry_start_y, &column.before, logos);
    draw_half_column(
        img,
        font,
        x + HALF_WIDTH,
        entry_start_y,
        &column.after,
        logos,
    );
}

fn draw_half_column(
    img: &mut RgbaImage,
    font: &FontArc,
    x: u32,
    y: u32,
    events: &[EarningsEvent],
    logos: &std::collections::HashMap<String, RgbaImage>,
) {
    for (idx, ev) in events.iter().take(MAX_PER_COLUMN).enumerate() {
        let entry_y = y + (idx as u32 * ENTRY_HEIGHT);

        let entry_rect = Rect::at((x + 4) as i32, (entry_y + 2) as i32)
            .of_size(HALF_WIDTH - 8, ENTRY_HEIGHT - 4);
        draw_filled_rect_mut(img, entry_rect, ENTRY_BG);

        // Place ticker on the left and logo on the right so they do not overlap
        let text_y = entry_y + 10;
        let text_x = x + 12;
        draw_text_mut(
            img,
            HEADER_COLOR,
            text_x as i32,
            text_y as i32,
            PxScale::from(14.0),
            font,
            &ev.symbol,
        );

        let logo_x = x + HALF_WIDTH - LOGO_W - 8;
        let logo_y = entry_y + (ENTRY_HEIGHT.saturating_sub(LOGO_H)) / 2;
        if let Some(logo) = logos.get(&ev.symbol) {
            imageops::overlay(img, logo, logo_x as i64, logo_y as i64);
        } else {
            let placeholder = placeholder_logo(&ev.symbol, font);
            imageops::overlay(img, &placeholder, logo_x as i64, logo_y as i64);
        }
    }

    let overflow = events.len().saturating_sub(MAX_PER_COLUMN);
    if overflow > 0 {
        let overflow_y = y + (MAX_PER_COLUMN as u32 * ENTRY_HEIGHT) - 15;
        let notice = format!("+{}", overflow);
        draw_centered_text(
            img,
            font,
            &notice,
            PxScale::from(14.0),
            x,
            HALF_WIDTH,
            overflow_y,
            HEADER_COLOR,
        );
    }
}

fn placeholder_logo(symbol: &str, font: &FontArc) -> RgbaImage {
    // Transparent background so logos are not placed on a card
    let mut img = RgbaImage::from_pixel(LOGO_W, LOGO_H, Rgba([0, 0, 0, 0]));
    draw_centered_text(
        &mut img,
        font,
        symbol,
        PxScale::from(18.0),
        0,
        LOGO_W,
        (LOGO_H / 2) - 12,
        HEADER_COLOR,
    );
    img
}

#[allow(clippy::too_many_arguments)]
fn draw_centered_text(
    img: &mut RgbaImage,
    font: &FontArc,
    text: &str,
    scale: PxScale,
    x: u32,
    width: u32,
    y: u32,
    color: Rgba<u8>,
) {
    let (tw, th) = text_size(scale, font, text);
    let offset_x = x as i32 + ((width as i32 - tw as i32) / 2);
    let offset_y = y as i32;
    draw_text_mut(
        img,
        color,
        offset_x,
        offset_y + th as i32,
        scale,
        font,
        text,
    );
}

fn classify_session(time: Option<&str>) -> Session {
    let Some(raw) = time else {
        return Session::Tba;
    };
    let t = raw.to_ascii_lowercase();
    if t.contains("amc") || t.contains("after") || t.starts_with("16") || t.ends_with("pm") {
        Session::After
    } else if t.contains("bmo") || t.contains("pre") || t.starts_with("09") || t.ends_with("am") {
        Session::Before
    } else {
        Session::Tba
    }
}
