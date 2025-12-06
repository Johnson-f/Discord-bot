use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use ab_glyph::{FontArc, PxScale};
use chrono::{Datelike, NaiveDate, Timelike, Utc};
use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Weight};
use font_kit::source::SystemSource;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, GenericImageView, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use once_cell::sync::Lazy;
use reqwest::Client;
use serenity::all::{CreateAttachment, CreateMessage, Http};
use serenity::model::prelude::ChannelId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::models::EarningsEvent;
use crate::service::command::earnings::format_output;
use crate::service::finance::FinanceService;

static LAST_POST_DATE: Lazy<Mutex<Option<chrono::NaiveDate>>> = Lazy::new(|| Mutex::new(None));

/// Spawn a daily earnings poster (once per day).
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

    info!("Starting earnings poster to channel {}", channel_id);

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1800)); // every 30 minutes
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
    let start = chrono::Utc::now().date_naive();
    let end = start + chrono::Duration::days(7);

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
                    CreateMessage::new().content("📊 Earnings Calendar (next 7 days)"),
                )
                .await
                .map_err(|e| format!("failed to post earnings calendar image: {e}"))?;
        }
        Err(render_err) => {
            warn!("Falling back to text earnings calendar: {}", render_err);
            let content = format_output(&events);
            channel_id
                .say(http, content)
                .await
                .map_err(|e| format!("failed to post fallback earnings calendar: {e}"))?;
        }
    }

    Ok(())
}

async fn should_post_now() -> bool {
    let now = Utc::now();
    // Target hour: 13:00-13:29 UTC (~9:00 AM ET)
    let in_window = now.hour() == 13;
    if !in_window {
        return false;
    }

    let today = now.date_naive();
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
    tba: Vec<EarningsEvent>,
}

enum Session {
    Before,
    After,
    Tba,
}

const COLUMN_WIDTH: u32 = 320;
const ENTRY_HEIGHT: u32 = 115;
const HEADER_HEIGHT: u32 = 70;
const MARGIN: u32 = 18;
const TITLE_COLOR: Rgba<u8> = Rgba([60, 45, 24, 255]);
const ACCENT_COLOR: Rgba<u8> = Rgba([196, 140, 79, 255]);
const SECTION_BG: Rgba<u8> = Rgba([255, 255, 255, 255]);
const CANVAS_BG: Rgba<u8> = Rgba([245, 238, 228, 255]);
const COLUMN_BG: Rgba<u8> = Rgba([252, 244, 232, 255]);
const BORDER_COLOR: Rgba<u8> = Rgba([220, 205, 180, 255]);
const LOGO_W: u32 = 240;
const LOGO_H: u32 = 90;
const MAX_PER_SECTION: usize = 8;

/// Render a 5-day earnings calendar image for Discord posts/commands.
pub async fn render_calendar_image(events: &[EarningsEvent]) -> Result<Vec<u8>, String> {
    use tokio::time::timeout;
    
    let columns = build_columns(events);
    if columns.is_empty() {
        return Err("no events to render".into());
    }

    let font = load_font()?;
    let client = Client::builder()
        .timeout(StdDuration::from_secs(5))
        .user_agent("stacks-bot/earnings-calendar")
        .build()
        .map_err(|e| format!("logo client build failed: {e}"))?;

    let unique_symbols: HashSet<String> = columns
        .iter()
        .flat_map(|c| {
            c.before
                .iter()
                .chain(c.after.iter())
                .chain(c.tba.iter())
                .map(|e| e.symbol.clone())
        })
        .collect();

    // Fetch logos with 20 second timeout for entire operation
    let logos = match timeout(
        StdDuration::from_secs(20),
        fetch_logos(&unique_symbols, &client)
    ).await {
        Ok(logos) => logos,
        Err(_) => {
            warn!("Logo fetching timed out, rendering without logos");
            HashMap::new()
        }
    };

    let mut image = DynamicImage::ImageRgba8(draw_canvas(&columns, &font, &logos));
    let mut buffer = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .map_err(|e| format!("failed to encode png: {e}"))?;

    Ok(buffer)
}

fn load_font() -> Result<FontArc, String> {
    let source = SystemSource::new();
    
    // Try to find a bold sans-serif font
    let handle = source
        .select_best_match(
            &[FamilyName::SansSerif],
            &Properties::new().weight(Weight::BOLD)
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

async fn fetch_logos(symbols: &HashSet<String>, client: &Client) -> HashMap<String, RgbaImage> {
    let mut handles = Vec::new();
    
    for sym in symbols {
        let sym = sym.clone();
        let client = client.clone();
        let handle = tokio::spawn(async move {
            download_logo(&sym, &client).await.map(|img| (sym, img))
        });
        handles.push(handle);
    }
    
    let mut logos = HashMap::new();
    for handle in handles {
        if let Ok(Some((sym, img))) = handle.await {
            logos.insert(sym, img);
        }
    }
    logos
}

async fn download_logo(symbol: &str, client: &Client) -> Option<RgbaImage> {
    use tokio::time::timeout;
    
    let urls = [
        format!("https://financialmodelingprep.com/image-stock/{symbol}.png"),
        format!("https://storage.googleapis.com/iex/api/logos/{symbol}.png"),
    ];

    for url in urls {
        // Add per-request timeout of 3 seconds
        let fetch = async {
            let resp = client.get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let bytes = resp.bytes().await.ok()?;
            let img = image::load_from_memory(&bytes).ok()?;
            Some(fit_logo(&img))
        };
        
        if let Ok(Some(img)) = timeout(StdDuration::from_secs(3), fetch).await {
            return Some(img);
        }
    }
    None
}

fn fit_logo(img: &DynamicImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let scale = (LOGO_W as f32 / w as f32)
        .min(LOGO_H as f32 / h as f32)
        .min(1.2);
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
            tba: Vec::new(),
        });

        match classify_session(ev.time_of_day.as_deref()) {
            Session::Before => entry.before.push(ev.clone()),
            Session::After => entry.after.push(ev.clone()),
            Session::Tba => entry.tba.push(ev.clone()),
        }
    }

    grouped
        .into_values()
        .take(5) // keep the view compact like the sample
        .collect()
}

fn draw_canvas(
    columns: &[DayColumn],
    font: &FontArc,
    logos: &HashMap<String, RgbaImage>,
) -> RgbaImage {
    let count = columns.len() as u32;
    let width = count * COLUMN_WIDTH + (count + 1) * MARGIN;

    let column_heights: Vec<u32> = columns
        .iter()
        .map(|c| {
            let mut height = HEADER_HEIGHT;
            
            // BMO section
            if !c.before.is_empty() {
                let before_visible = c.before.len().min(MAX_PER_SECTION) as u32;
                let before_overflow = if c.before.len() > MAX_PER_SECTION { 24 } else { 0 };
                height += 26 + before_visible * ENTRY_HEIGHT + before_overflow + 12;
            }
            
            // TBA section
            if !c.tba.is_empty() {
                let tba_visible = c.tba.len().min(MAX_PER_SECTION) as u32;
                let tba_overflow = if c.tba.len() > MAX_PER_SECTION { 24 } else { 0 };
                height += 26 + tba_visible * ENTRY_HEIGHT + tba_overflow + 12;
            }
            
            // AMC section
            if !c.after.is_empty() {
                let after_visible = c.after.len().min(MAX_PER_SECTION) as u32;
                let after_overflow = if c.after.len() > MAX_PER_SECTION { 24 } else { 0 };
                height += 26 + after_visible * ENTRY_HEIGHT + after_overflow + 12;
            }
            
            height + 12 // bottom breathing room
        })
        .collect();
    let height = column_heights.iter().max().cloned().unwrap_or(0) + 2 * MARGIN;

    let mut img = RgbaImage::from_pixel(width, height, CANVAS_BG);

    for (idx, column) in columns.iter().enumerate() {
        let x = MARGIN + idx as u32 * (COLUMN_WIDTH + MARGIN);
        let y = MARGIN;
        draw_column(&mut img, x, y, column, font, logos, column_heights[idx]);
    }

    img
}

fn draw_column(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    column: &DayColumn,
    font: &FontArc,
    logos: &HashMap<String, RgbaImage>,
    column_height: u32,
) {
    let rect = Rect::at(x as i32, y as i32).of_size(COLUMN_WIDTH, column_height);
    draw_filled_rect_mut(img, rect, COLUMN_BG);

    let scale_header = PxScale::from(30.0);
    let label = format!(
        "{} {}",
        column.date.weekday().to_string(),
        column.date.format("%b %e")
    );
    draw_centered_text(
        img,
        font,
        &label,
        scale_header,
        x,
        COLUMN_WIDTH,
        y + 12,
        TITLE_COLOR,
    );

    let mut current_y = y + HEADER_HEIGHT;
    
    // Draw BMO section if there are any
    if !column.before.is_empty() {
        draw_section_title(img, font, "Before Open", x, current_y, ACCENT_COLOR);
        current_y += 26;
        current_y = draw_entries(img, font, x, current_y, &column.before, logos, "BMO");
        current_y += 12;
    }
    
    // Draw TBA section if there are any
    if !column.tba.is_empty() {
        draw_section_title(img, font, "Time TBA", x, current_y, ACCENT_COLOR);
        current_y += 26;
        current_y = draw_entries(img, font, x, current_y, &column.tba, logos, "TBA");
        current_y += 12;
    }
    
    // Draw AMC section if there are any
    if !column.after.is_empty() {
        draw_section_title(img, font, "After Close", x, current_y, ACCENT_COLOR);
        current_y += 26;
        let _ = draw_entries(img, font, x, current_y, &column.after, logos, "AMC");
    }
}

fn draw_entries(
    img: &mut RgbaImage,
    font: &FontArc,
    x: u32,
    mut y: u32,
    events: &[EarningsEvent],
    logos: &HashMap<String, RgbaImage>,
    fallback_label: &str,
) -> u32 {
    let scale_symbol = PxScale::from(28.0);
    let scale_sub = PxScale::from(20.0);

    let display_events: Vec<&EarningsEvent> = events.iter().take(MAX_PER_SECTION).collect();
    for ev in display_events {
        let entry_rect =
            Rect::at(x as i32 + 8, y as i32).of_size(COLUMN_WIDTH - 16, ENTRY_HEIGHT - 8);
        draw_filled_rect_mut(img, entry_rect, SECTION_BG);

        if let Some(logo) = logos.get(&ev.symbol) {
            let lx = x + (COLUMN_WIDTH - LOGO_W) / 2;
            let ly = y + 10;
            imageops::overlay(img, logo, lx as i64, ly as i64);
        } else {
            let placeholder = placeholder_logo(&ev.symbol, font);
            let lx = x + (COLUMN_WIDTH - LOGO_W) / 2;
            let ly = y + 10;
            imageops::overlay(img, &placeholder, lx as i64, ly as i64);
        }

        let label = session_label(ev, fallback_label);
        draw_centered_text(
            img,
            font,
            &ev.symbol,
            scale_symbol,
            x,
            COLUMN_WIDTH,
            y + LOGO_H + 12,
            TITLE_COLOR,
        );
        draw_centered_text(
            img,
            font,
            label,
            scale_sub,
            x,
            COLUMN_WIDTH,
            y + LOGO_H + 38,
            ACCENT_COLOR,
        );

        // border around entry
        imageproc::drawing::draw_hollow_rect_mut(img, entry_rect, BORDER_COLOR);

        y += ENTRY_HEIGHT;
    }

    let overflow = events.len().saturating_sub(MAX_PER_SECTION);
    if overflow > 0 {
        let notice = format!("+{} more", overflow);
        draw_centered_text(
            img,
            font,
            &notice,
            PxScale::from(18.0),
            x,
            COLUMN_WIDTH,
            y + 4,
            TITLE_COLOR,
        );
        y += 24;
    }

    y
}

fn placeholder_logo(symbol: &str, font: &FontArc) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(LOGO_W, LOGO_H, Rgba([235, 227, 210, 255]));
    draw_centered_text(
        &mut img,
        font,
        symbol,
        PxScale::from(26.0),
        0,
        LOGO_W,
        (LOGO_H / 2) - 18,
        TITLE_COLOR,
    );
    img
}

fn draw_section_title(
    img: &mut RgbaImage,
    font: &FontArc,
    text: &str,
    x: u32,
    y: u32,
    color: Rgba<u8>,
) {
    draw_centered_text(
        img,
        font,
        text,
        PxScale::from(22.0),
        x,
        COLUMN_WIDTH,
        y,
        color,
    );
}

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
    draw_text_mut(img, color, offset_x, offset_y + th as i32, scale, font, text);
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

fn session_label<'a>(event: &EarningsEvent, fallback: &'a str) -> &'a str {
    match classify_session(event.time_of_day.as_deref()) {
        Session::Before => "BMO",
        Session::After => "AMC",
        Session::Tba => fallback,
    }
}