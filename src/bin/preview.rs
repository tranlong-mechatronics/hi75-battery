use std::path::Path;

#[path = "../icon.rs"]
mod icon;
#[path = "../kuromi.rs"]
mod kuromi;

fn save_bgra_as_png(path: &str, w: u32, h: u32, bgra: &[u8]) {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        let b = px[0];
        let g = px[1];
        let r = px[2];
        let a = px[3];
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            rgba.extend_from_slice(&[r, g, b, 255]);
        } else {
            let af = a as f32 / 255.0;
            rgba.push(((r as f32 / af).min(255.0)) as u8);
            rgba.push(((g as f32 / af).min(255.0)) as u8);
            rgba.push(((b as f32 / af).min(255.0)) as u8);
            rgba.push(a);
        }
    }
    image::save_buffer(Path::new(path), &rgba, w, h, image::ExtendedColorType::Rgba8).unwrap();
}

fn main() {
    let widget_cases = [
        (
            Some(85),
            false,
            false,
            kuromi::KuromiMood::Sit,
            "D:/hi75-battery/preview_kuromi_sit.png",
        ),
        (
            Some(85),
            false,
            true,
            kuromi::KuromiMood::Love,
            "D:/hi75-battery/preview_kuromi_love.png",
        ),
        (
            Some(80),
            true,
            false,
            kuromi::KuromiMood::Happy,
            "D:/hi75-battery/preview_kuromi_charging.png",
        ),
        (
            Some(15),
            false,
            false,
            kuromi::KuromiMood::LowBattery,
            "D:/hi75-battery/preview_kuromi_low.png",
        ),
        (
            None,
            false,
            false,
            kuromi::KuromiMood::Sleep,
            "D:/hi75-battery/preview_kuromi_sleep.png",
        ),
    ];
    for (pct, charging, hovered, mood, path) in widget_cases {
        let (w, h, bgra) =
            icon::create_taskbar_widget_bgra(pct, charging, hovered, 1.5, true, mood, 1);
        save_bgra_as_png(path, w, h, &bgra);
        println!("Saved {} ({}x{})", path, w, h);
    }
}
