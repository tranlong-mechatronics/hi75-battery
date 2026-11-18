use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{fontdb, Options, Tree};
use std::sync::{Arc, OnceLock};

pub const WIDGET_LOGICAL_HEIGHT: f32 = 32.0;

static FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

fn get_font_db() -> Arc<fontdb::Database> {
    FONT_DB
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

pub fn is_light_theme() -> bool {
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let val_name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut hkey: winapi::shared::minwindef::HKEY = std::ptr::null_mut();

    unsafe {
        if winapi::um::winreg::RegOpenKeyExW(
            winapi::um::winreg::HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            winapi::um::winnt::KEY_READ,
            &mut hkey,
        ) == 0
        {
            let mut data: u32 = 0;
            let mut data_size = std::mem::size_of::<u32>() as u32;
            let res = winapi::um::winreg::RegQueryValueExW(
                hkey,
                val_name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut data as *mut u32 as _,
                &mut data_size,
            );
            winapi::um::winreg::RegCloseKey(hkey);
            if res == 0 {
                return data == 1;
            }
        }
    }
    true
}

fn battery_fill_color(battery_opt: Option<u8>, is_charging: bool) -> &'static str {
    if is_charging {
        return "#16A34A";
    }
    match battery_opt {
        Some(b) => {
            if b > 40 {
                "#16A34A" // Emerald/Green matching Win11 battery
            } else if b > 20 {
                "#F59E0B" // Amber
            } else {
                "#EF4444" // Red
            }
        }
        None => "#94A3B8",
    }
}

/// Renders the square system tray icon at `size x size` physical pixels (RGBA8 unpremultiplied).
pub fn create_battery_icon(battery_opt: Option<u8>, is_charging: bool, size: u32) -> Vec<u8> {
    let size = size.max(16);
    let light_mode = is_light_theme();
    let stroke_color = if light_mode { "#1E293B" } else { "#F8FAFC" };
    let fill_color = battery_fill_color(battery_opt, is_charging);

    let inner_bar = match battery_opt {
        Some(pct_val) => {
            let w = (20.8 * (pct_val as f32 / 100.0)).clamp(3.0, 20.8);
            format!(
                r##"<rect x="3.6" y="8.4" width="{:.1}" height="15.2" rx="2.4" fill="{}" />"##,
                w, fill_color
            )
        }
        None if is_charging => format!(
            r##"<rect x="3.6" y="8.4" width="20.8" height="15.2" rx="2.4" fill="{}" />"##,
            fill_color
        ),
        None => String::new(),
    };

    let content_overlay = if is_charging {
        r##"
        <path d="M15.8 7.5 L9.8 16.5 H14.8 L13.2 24.5 L20.8 14.5 H15.8 L17.2 7.5 Z"
              fill="#FBBF24" stroke="#0F172A" stroke-width="1.2" stroke-linejoin="round" />
        "##
        .to_string()
    } else if let Some(b) = battery_opt {
        let (font_size, letter_spacing) = if b == 100 {
            ("9.5", "-0.8px")
        } else {
            ("11.2", "-0.4px")
        };
        format!(
            r##"
            <text x="14.0" y="20.0"
                  font-family="Segoe UI Variable Display, Segoe UI, sans-serif"
                  font-size="{}" font-weight="800"
                  letter-spacing="{}"
                  fill="#FFFFFF"
                  stroke="#0F172A" stroke-width="2.4" paint-order="stroke fill"
                  text-anchor="middle">{}</text>
            "##,
            font_size, letter_spacing, b
        )
    } else {
        format!(
            r##"
            <text x="14.0" y="20.0"
                  font-family="Segoe UI Variable Display, Segoe UI, sans-serif"
                  font-size="11.5" font-weight="700"
                  fill="{}"
                  text-anchor="middle">?</text>
            "##,
            stroke_color
        )
    };

    let svg = format!(
        r##"
        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
            <rect x="1.5" y="6.2" width="25.0" height="19.6" rx="4.2"
                  fill="none" stroke="{}" stroke-width="2.2" stroke-linejoin="round" />
            <path d="M29.2 12.6 V19.4"
                  stroke="{}" stroke-width="2.6" stroke-linecap="round" />
            {}
            {}
        </svg>
        "##,
        stroke_color, stroke_color, inner_bar, content_overlay
    );

    let fontdb = get_font_db();
    let mut opt = Options::default();
    opt.fontdb = fontdb;
    opt.font_family = "Segoe UI Variable Display".to_string();

    let tree = Tree::from_str(&svg, &opt).unwrap();
    let mut pixmap = Pixmap::new(size, size).unwrap();
    let scale = size as f32 / 32.0;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    let mut raw = Vec::with_capacity((size * size * 4) as usize);
    for pixel in pixmap.pixels() {
        let c = pixel.demultiply();
        raw.push(c.red());
        raw.push(c.green());
        raw.push(c.blue());
        raw.push(c.alpha());
    }
    raw
}

/// Renders the horizontal taskbar widget `[Kuromi] [Battery] 49%` as premultiplied BGRA8 bytes
/// suitable for Win32 `UpdateLayeredWindow`. Returns `(width_px, height_px, bgra_bytes)`.
pub fn create_taskbar_widget_bgra(
    battery_opt: Option<u8>,
    is_charging: bool,
    hovered: bool,
    scale: f32,
    show_kuromi: bool,
    kuromi_mood: crate::kuromi::KuromiMood,
    kuromi_frame: usize,
) -> (u32, u32, Vec<u8>) {
    let scale = scale.clamp(1.0, 3.0);
    let height_dip = WIDGET_LOGICAL_HEIGHT;
    let pad_x = 7.0f32;
    let kuromi_dip = 28.0f32;
    let kuromi_left_dip = 3.0f32;
    let kuromi_gap_dip = 3.5f32;

    let light_mode = is_light_theme();
    let is_active = battery_opt.is_some() || is_charging;
    let text_color = match (light_mode, is_active) {
        (true, true) => "#111827",
        (true, false) => "#64748B",
        (false, true) => "#F8FAFC",
        (false, false) => "#94A3B8",
    };

    // Outer battery shell color (rich dark charcoal in light mode, crisp white in dark mode,
    // or soft red when disconnected like the bottom-right icon in the reference image)
    let is_disconnected = battery_opt.is_none() && !is_charging;
    let shell_color = if is_disconnected {
        "#E11D48"
    } else if light_mode {
        "#0F172A"
    } else {
        "#F8FAFC"
    };

    // Tiny alpha (0.004) when unhovered ensures Windows hit-testing catches mouse events
    // across the whole pill while staying visually transparent over the taskbar.
    let (pill_fill, pill_stroke) = if hovered {
        if light_mode {
            ("rgba(255, 255, 255, 0.55)", "rgba(0, 0, 0, 0.08)")
        } else {
            ("rgba(255, 255, 255, 0.10)", "rgba(255, 255, 255, 0.12)")
        }
    } else {
        ("rgba(0, 0, 0, 0.004)", "none")
    };

    // Gradient stops matching the reference image (smooth green -> lime, or warm red-orange -> amber)
    let (grad_start, grad_end) = match battery_opt {
        Some(b) if b > 45 => ("#22C55E", "#84CC16"),
        Some(b) if b > 20 => ("#EA580C", "#FB923C"),
        Some(_) => ("#DC2626", "#F97316"),
        None => ("#94A3B8", "#CBD5E1"),
    };

    // Battery body: x=0.0..20.5, y=9.6..22.4 (height 12.8, rx=3.8 for super smooth squircle curve)
    // Inner bar: x=2.3, y=11.9, max_width=15.9, height=8.2, rx=2.2 (hidden when charging so the slanted bolt stands out cleanly like Image_2)
    let inner_bar = if is_charging {
        String::new()
    } else if let Some(pct_val) = battery_opt {
        let w = (15.9 * (pct_val as f32 / 100.0)).clamp(3.2, 15.9);
        format!(
            r##"<rect x="2.3" y="11.9" width="{:.2}" height="8.2" rx="2.2" fill="url(#batGrad)" />"##,
            w
        )
    } else {
        String::new()
    };

    let overlay_graphic = if is_charging {
        // Slanted, smooth-cornered yellow lightning bolt inside the battery matching Image_2
        r##"
        <path d="M12.4 11.6 L6.4 15.7 L9.9 16.4 L8.0 20.4 L14.1 16.3 L10.6 15.6 Z"
              fill="url(#boltGrad)" stroke="url(#boltGrad)" stroke-width="0.95"
              stroke-linejoin="round" stroke-linecap="round" />
        "##
        .to_string()
    } else if is_disconnected {
        // Diagonal red slash across the battery matching the bottom-right reference icon
        format!(
            r##"
            <line x1="3.8" y1="23.6" x2="17.2" y2="8.4"
                  stroke="{}" stroke-width="2.1" stroke-linecap="round" />
            "##,
            shell_color
        )
    } else {
        String::new()
    };

    let label = match (battery_opt, is_charging) {
        (Some(pct), _) => format!("{}%", pct),
        (None, true) => "Sạc".to_string(),
        (None, false) => "--%".to_string(),
    };

    let content_group = format!(
        r##"
        <defs>
            <linearGradient id="batGrad" x1="0%" y1="0%" x2="100%" y2="0%">
                <stop offset="0%" stop-color="{}" />
                <stop offset="100%" stop-color="{}" />
            </linearGradient>
            <linearGradient id="boltGrad" x1="0%" y1="0%" x2="100%" y2="100%">
                <stop offset="0%" stop-color="#FDE047" />
                <stop offset="100%" stop-color="#F59E0B" />
            </linearGradient>
        </defs>
        <g id="content">
            <!-- Smooth rounded battery body with transparent interior -->
            <rect x="0.0" y="9.6" width="20.5" height="12.8" rx="3.8"
                  fill="none" stroke="{}" stroke-width="1.75" stroke-linejoin="round" />
            <!-- Solid rounded positive terminal bump attached to right wall -->
            <path d="M20.5 12.7 H21.7 A1.3 1.3 0 0 1 23.0 14.0 V18.0 A1.3 1.3 0 0 1 21.7 19.3 H20.5 Z"
                  fill="{}" />
            {}
            {}
            <text x="26.2" y="19.8"
                  font-family="Segoe UI Variable Text, Segoe UI, sans-serif"
                  font-size="11.2" font-weight="600"
                  fill="{}">{}</text>
        </g>
        "##,
        grad_start,
        grad_end,
        shell_color,
        shell_color,
        inner_bar,
        overlay_graphic,
        text_color,
        label
    );

    let fontdb = get_font_db();
    let mut opt = Options::default();
    opt.fontdb = fontdb;
    opt.font_family = "Segoe UI Variable Text".to_string();

    // First pass: measure exact rendered bounding box of the battery + label group
    let measure_svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="140" height="32" viewBox="0 0 140 32">{}</svg>"##,
        content_group
    );
    let measure_tree = Tree::from_str(&measure_svg, &opt).unwrap();
    let (content_left, content_width) = measure_tree
        .node_by_id("content")
        .map(|n| {
            let b = n.abs_bounding_box();
            (b.left(), (b.right() - b.left()).max(24.0))
        })
        .unwrap_or((0.0, 48.0));

    let left_offset_dip = if show_kuromi {
        kuromi_left_dip + kuromi_dip + kuromi_gap_dip
    } else {
        pad_x
    };
    let width_dip = left_offset_dip + content_width + pad_x;
    let width_px = (width_dip * scale).round() as u32;
    let height_px = (height_dip * scale).round() as u32;
    let x_shift = left_offset_dip - content_left;

    let final_svg = format!(
        r##"
        <svg xmlns="http://www.w3.org/2000/svg" width="{:.2}" height="{:.2}" viewBox="0 0 {:.2} {:.2}">
            <rect x="0.5" y="0.5" width="{:.2}" height="{:.2}" rx="4.5"
                  fill="{}" stroke="{}" stroke-width="1" />
            <g transform="translate({:.2}, 0)">
                {}
            </g>
        </svg>
        "##,
        width_dip,
        height_dip,
        width_dip,
        height_dip,
        width_dip - 1.0,
        height_dip - 1.0,
        pill_fill,
        pill_stroke,
        x_shift,
        content_group
    );

    let tree = Tree::from_str(&final_svg, &opt).unwrap();
    let mut pixmap = Pixmap::new(width_px, height_px).unwrap();
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    // resvg/tiny-skia already outputs premultiplied RGBA; Windows UpdateLayeredWindow wants premultiplied BGRA.
    let mut bgra = Vec::with_capacity((width_px * height_px * 4) as usize);
    for pixel in pixmap.pixels() {
        bgra.push(pixel.blue());
        bgra.push(pixel.green());
        bgra.push(pixel.red());
        bgra.push(pixel.alpha());
    }

    if show_kuromi {
        let sprite_px = (kuromi_dip * scale).round() as u32;
        let dst_x = (kuromi_left_dip * scale).round() as i32;
        let dst_y = ((height_px as i32 - sprite_px as i32) / 2).max(0);
        crate::kuromi::blit_kuromi_bgra(
            &mut bgra,
            width_px,
            height_px,
            dst_x,
            dst_y,
            sprite_px,
            kuromi_mood,
            kuromi_frame,
        );
    }

    (width_px, height_px, bgra)
}
