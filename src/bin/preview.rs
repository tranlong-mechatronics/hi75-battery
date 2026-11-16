use std::path::Path;

#[path = "../icon.rs"]
mod icon;

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
    let test_cases = [
        (Some(100), false, "D:/hi75-battery/preview_100.png"),
        (Some(85), false, "D:/hi75-battery/preview_85.png"),
        (Some(45), false, "D:/hi75-battery/preview_45.png"),
        (Some(15), false, "D:/hi75-battery/preview_15.png"),
        (Some(80), true, "D:/hi75-battery/preview_charging.png"),
    ];

    for (pct, charging, path) in test_cases {
        let raw = icon::create_battery_icon(pct, charging, 32);
        image::save_buffer(
            Path::new(path),
            &raw,
            32,
            32,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        println!("Saved {}", path);
    }

    let widget_cases = [
        (Some(46), false, true, "D:/hi75-battery/preview_widget_49.png"),
        (Some(100), false, true, "D:/hi75-battery/preview_widget_100_hover.png"),
        (Some(80), true, false, "D:/hi75-battery/preview_widget_charging.png"),
        (None, true, false, "D:/hi75-battery/preview_widget_wired_charging.png"),
        (None, false, false, "D:/hi75-battery/preview_widget_none.png"),
    ];
    for (pct, charging, hovered, path) in widget_cases {
        let (w, h, bgra) = icon::create_taskbar_widget_bgra(pct, charging, hovered, 1.25);
        save_bgra_as_png(path, w, h, &bgra);
        println!("Saved {} ({}x{})", path, w, h);
    }
}
