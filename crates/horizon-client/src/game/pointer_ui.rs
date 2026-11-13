//! Where a pointer is in the units the Starling cards (the Starling folder's offer, game/files_offer.rs)
//! draw and hit-test in: `ui_theme::Ui`'s, which scale the GUI's pixels by a density of their own.
//! A headset's pointer is a position on the menu panel, which the game renders at the renderer's
//! size, not the window's: a card must be hit-tested with the same size it was drawn at.
// The card helpers below are used on Android only.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]
use horizon_client::hud::HudBatch;

/// `ui_theme::Ui`'s density for a surface of `size` pixels (the GUI batch's own, as `Ui::new`).
fn density(size: [u32; 2]) -> f32 {
    let b = HudBatch::new(size);
    let (w, h) = (b.width * b.scale, b.height * b.scale);
    (w / 1040.0).min(h / 700.0).clamp(1.0, 4.0)
}

/// A position in pixels of a surface of `size`, in UI units.
pub(super) fn pixel_to_ui(px: [f32; 2], size: [u32; 2]) -> [f32; 2] {
    let d = density(size);
    [px[0] / d, px[1] / d]
}

/// The game's cursor (GUI pixels, `Game::cursor`), in UI units, for a surface of `size` pixels.
pub(super) fn gui_to_ui(cursor: (f32, f32), size: [u32; 2]) -> [f32; 2] {
    let scale = HudBatch::new(size).scale;
    pixel_to_ui([cursor.0 * scale, cursor.1 * scale], size)
}

/// Where a laser meets the panel (`at`, 0..1 from its top left) in GUI pixels, which is what
/// `Game::cursor` is in, for a renderer of `size` pixels: the point the pointer is drawn at.
pub(super) fn laser_gui_point(at: [f32; 2], size: [u32; 2]) -> (f32, f32) {
    let scale = HudBatch::new(size).scale;
    (
        at[0] * size[0] as f32 / scale,
        at[1] * size[1] as f32 / scale,
    )
}

/// The cursor is off the laser's point (`at`, in window pixels) by more than half a GUI pixel:
/// something moved it since the laser did.
pub(super) fn cursor_off_laser(cursor: (f32, f32), at: (f32, f32), scale: f32) -> bool {
    (cursor.0 - at.0 / scale).abs() > 0.5 || (cursor.1 - at.1 / scale).abs() > 0.5
}

/// The rectangle (`[x, y, w, h]`) `p` is inside, the first of them.
pub(super) fn rect_at(rects: &[[f32; 4]], p: [f32; 2]) -> Option<usize> {
    rects
        .iter()
        .position(|r| p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Quest's eye image: the renderer's size on the headset, nearly square.
    const EYE: [u32; 2] = [1832, 1920];

    #[test]
    fn the_middle_of_the_panel_is_the_middle_of_the_ui() {
        // The card is centred in the UI's width: a pointer in the panel's middle must be there.
        let b = HudBatch::new(EYE);
        let d = density(EYE);
        let ui_w = b.width * b.scale / d;
        let ui_h = b.height * b.scale / d;
        let p = pixel_to_ui([EYE[0] as f32 / 2.0, EYE[1] as f32 / 2.0], EYE);
        assert!((p[0] - ui_w / 2.0).abs() < 1.5, "{p:?} in {ui_w}");
        assert!((p[1] - ui_h / 2.0).abs() < 1.5, "{p:?} in {ui_h}");
    }

    #[test]
    fn the_cursor_and_its_pixel_are_the_same_point() {
        let scale = HudBatch::new(EYE).scale;
        let cursor = (123.4, 210.9);
        let a = gui_to_ui(cursor, EYE);
        let b = pixel_to_ui([cursor.0 * scale, cursor.1 * scale], EYE);
        assert_eq!(a, b);
    }

    #[test]
    fn a_card_is_hit_at_the_size_it_was_drawn_for_not_the_windows() {
        // A button 100 UI units wide at x = 200, hit by the panel pixel that shows it. At the
        // window's size (1920 x 1080 here) the same pixel lands on another point.
        let d = density(EYE);
        let button = [[200.0, 100.0, 100.0, 40.0]];
        let pixel = [290.0 * d, 120.0 * d];
        assert_eq!(rect_at(&button, pixel_to_ui(pixel, EYE)), Some(0));
        assert_eq!(rect_at(&button, pixel_to_ui(pixel, [1920, 1080])), None);
    }

    #[test]
    fn the_drawn_pointer_is_where_the_laser_meets_the_panel_every_frame() {
        let scale = HudBatch::new(EYE).scale;
        // Corners and a middle: the GUI point times the scale is the pixel of the panel.
        for at in [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0], [0.31, 0.83]] {
            let (x, y) = laser_gui_point(at, EYE);
            assert!((x * scale - at[0] * EYE[0] as f32).abs() < 1e-2, "{at:?}");
            assert!((y * scale - at[1] * EYE[1] as f32).abs() < 1e-2, "{at:?}");
        }
        // As the laser moves the point moves with it: it is not tied to the middle.
        assert_ne!(
            laser_gui_point([0.2, 0.3], EYE),
            laser_gui_point([0.5, 0.5], EYE)
        );
    }

    #[test]
    fn a_cursor_taken_to_the_middle_is_off_the_laser_and_one_on_it_is_not() {
        let scale = HudBatch::new(EYE).scale;
        let at = (0.2 * EYE[0] as f32, 0.7 * EYE[1] as f32);
        let on = laser_gui_point([0.2, 0.7], EYE);
        assert!(!cursor_off_laser(on, at, scale));
        let middle = laser_gui_point([0.5, 0.5], EYE);
        assert!(cursor_off_laser(middle, at, scale));
    }

    #[test]
    fn the_first_rectangle_under_the_point_wins_and_none_misses() {
        let rects = [[0.0, 0.0, 10.0, 10.0], [5.0, 5.0, 10.0, 10.0]];
        assert_eq!(rect_at(&rects, [7.0, 7.0]), Some(0));
        assert_eq!(rect_at(&rects, [12.0, 12.0]), Some(1));
        assert_eq!(rect_at(&rects, [30.0, 1.0]), None);
    }
}
