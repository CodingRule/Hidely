//! The tray icon is drawn in code so its colour can follow the app's state.

use tauri::image::Image;

const SIZE: u32 = 32;

/// What the tray icon should say at a glance. The tooltip always says it in words too.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayLook {
    /// No privacy session.
    Idle,
    /// A session is running and nothing has gone wrong.
    Active,
    /// A session is running, but some windows could not be protected or restored.
    Attention,
}

/// A little ghost: grey when idle, red during a session, amber when something needs a look.
pub fn tray_icon(look: TrayLook) -> Image<'static> {
    let colour: [u8; 3] = match look {
        TrayLook::Idle => [0x9A, 0xA0, 0xA6],
        TrayLook::Active => [0xE5, 0x48, 0x4D],
        TrayLook::Attention => [0xF5, 0xA5, 0x24],
    };
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let alpha = coverage(x as f32 + 0.5, y as f32 + 0.5);
            let i = ((y * SIZE + x) * 4) as usize;
            rgba[i..i + 3].copy_from_slice(&colour);
            rgba[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new_owned(rgba, SIZE, SIZE)
}

/// How much of the pixel centred on (x, y) the ghost covers, 0.0..=1.0.
fn coverage(x: f32, y: f32) -> f32 {
    const CX: f32 = 16.0;
    const HEAD_Y: f32 = 14.0;
    const RADIUS: f32 = 11.0;
    const BOTTOM: f32 = 29.0;

    let dx = x - CX;
    let body = if y < HEAD_Y {
        // Round head.
        RADIUS - (dx * dx + (y - HEAD_Y) * (y - HEAD_Y)).sqrt()
    } else {
        // Straight sides ending in a wavy hem.
        let hem = BOTTOM - 2.0 * (1.0 + (dx * std::f32::consts::PI / 3.67).cos());
        (RADIUS - dx.abs()).min(hem - y)
    };
    let eyes = [-4.0f32, 4.0]
        .iter()
        .map(|ex| 2.4 - ((dx - ex) * (dx - ex) + (y - 13.0) * (y - 13.0)).sqrt())
        .fold(f32::MIN, f32::max);
    // Inside the body and outside the eyes, with a one-pixel soft edge.
    (body.min(-eyes) + 0.5).clamp(0.0, 1.0)
}
