use image::imageops::FilterType;
use image::RgbaImage;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KuromiMood {
    Sit,
    Love,
    Happy,
    LowBattery,
    Sleep,
}

struct KuromiSprites {
    sit: [RgbaImage; 4],
    love: [RgbaImage; 4],
    happy: [RgbaImage; 4],
    low: [RgbaImage; 4],
    sleep: [RgbaImage; 4],
}

static SPRITES: OnceLock<KuromiSprites> = OnceLock::new();

fn decode_png(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes)
        .expect("Failed to decode embedded Kuromi PNG")
        .into_rgba8()
}

fn get_sprites() -> &'static KuromiSprites {
    SPRITES.get_or_init(|| KuromiSprites {
        sit: [
            decode_png(include_bytes!("../assets/kuromi_pack/sit_0.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sit_1.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sit_2.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sit_3.png")),
        ],
        love: [
            decode_png(include_bytes!("../assets/kuromi_pack/love_0.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/love_1.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/love_2.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/love_3.png")),
        ],
        happy: [
            decode_png(include_bytes!("../assets/kuromi_pack/happy_0.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/happy_1.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/happy_2.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/happy_3.png")),
        ],
        low: [
            decode_png(include_bytes!("../assets/kuromi_pack/low_0.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/low_1.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/low_2.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/low_3.png")),
        ],
        sleep: [
            decode_png(include_bytes!("../assets/kuromi_pack/sleep_0.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sleep_1.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sleep_2.png")),
            decode_png(include_bytes!("../assets/kuromi_pack/sleep_3.png")),
        ],
    })
}

pub fn pick_mood(
    battery: Option<u8>,
    charging: bool,
    hovered: bool,
    love_ticks_left: u8,
) -> KuromiMood {
    if love_ticks_left > 0 || hovered {
        return KuromiMood::Love;
    }
    if charging {
        return KuromiMood::Happy;
    }
    match battery {
        Some(pct) if pct <= 20 => KuromiMood::LowBattery,
        Some(_) => KuromiMood::Sit,
        None => KuromiMood::Sleep,
    }
}

/// Blits the selected Kuromi frame (premultiplied BGRA) onto a widget canvas of size `canvas_w x canvas_h`.
pub fn blit_kuromi_bgra(
    canvas: &mut [u8],
    canvas_w: u32,
    canvas_h: u32,
    dst_x: i32,
    dst_y: i32,
    sprite_px: u32,
    mood: KuromiMood,
    frame_idx: usize,
) {
    let sprites = get_sprites();
    let frames = match mood {
        KuromiMood::Sit => &sprites.sit,
        KuromiMood::Love => &sprites.love,
        KuromiMood::Happy => &sprites.happy,
        KuromiMood::LowBattery => &sprites.low,
        KuromiMood::Sleep => &sprites.sleep,
    };
    let src = &frames[frame_idx % frames.len()];
    let resized = if src.width() == sprite_px && src.height() == sprite_px {
        src.clone()
    } else {
        image::imageops::resize(src, sprite_px, sprite_px, FilterType::Lanczos3)
    };

    for sy in 0..sprite_px as i32 {
        let cy = dst_y + sy;
        if cy < 0 || cy >= canvas_h as i32 {
            continue;
        }
        for sx in 0..sprite_px as i32 {
            let cx = dst_x + sx;
            if cx < 0 || cx >= canvas_w as i32 {
                continue;
            }
            let p = resized.get_pixel(sx as u32, sy as u32);
            let sa = p[3] as u32;
            if sa == 0 {
                continue;
            }
            // Premultiply source RGBA
            let sr = (p[0] as u32 * sa + 127) / 255;
            let sg = (p[1] as u32 * sa + 127) / 255;
            let sb = (p[2] as u32 * sa + 127) / 255;

            let idx = ((cy as u32 * canvas_w + cx as u32) * 4) as usize;
            let db = canvas[idx] as u32;
            let dg = canvas[idx + 1] as u32;
            let dr = canvas[idx + 2] as u32;
            let da = canvas[idx + 3] as u32;

            let inv_sa = 255 - sa;
            canvas[idx] = (sb + (db * inv_sa + 127) / 255).min(255) as u8;
            canvas[idx + 1] = (sg + (dg * inv_sa + 127) / 255).min(255) as u8;
            canvas[idx + 2] = (sr + (dr * inv_sa + 127) / 255).min(255) as u8;
            canvas[idx + 3] = (sa + (da * inv_sa + 127) / 255).min(255) as u8;
        }
    }
}
