// SPDX-License-Identifier: MPL-2.0
//! Visor's own textures, loaded as shipped (`assets/visor/`, copied unchanged from the mod; see
//! its `NOTICE`): the hotbar wheel's pictures (`VROverlayHotBar`, `HotBarSlice`), the movement
//! state icons (`VROverlayMovementState`), the fullscreen warning's sign
//! (`VROverlayFullscreenWarning`) and the headset icon (`VRMenuSky`, `VRInactiveScreen`).
//! Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! Each is an atlas sprite `horizon/visor/<name>` of the size of its file.

macro_rules! art {
    ($($name:literal => $file:literal),* $(,)?) => {
        /// Sprite name and PNG bytes of each texture.
        pub const TEXTURES: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../assets/visor/", $file))),)*
        ];
    };
}

art! {
    "horizon/visor/hotbar_default" => "hotbar/default.png",
    "horizon/visor/hotbar_top" => "hotbar/top.png",
    "horizon/visor/hotbar_top_right" => "hotbar/top_right.png",
    "horizon/visor/hotbar_right" => "hotbar/right.png",
    "horizon/visor/hotbar_bottom_right" => "hotbar/bottom_right.png",
    "horizon/visor/hotbar_bottom" => "hotbar/bottom.png",
    "horizon/visor/hotbar_bottom_left" => "hotbar/bottom_left.png",
    "horizon/visor/hotbar_left" => "hotbar/left.png",
    "horizon/visor/hotbar_top_left" => "hotbar/top_left.png",
    "horizon/visor/hotbar_slot_numbers" => "hotbar/slot_numbers.png",
    "horizon/visor/hotbar_main_selected0" => "hotbar/hotbar_main_selected0.png",
    "horizon/visor/hotbar_main_selected1" => "hotbar/hotbar_main_selected1.png",
    "horizon/visor/hotbar_offhand_selected0" => "hotbar/hotbar_offhand_selected0.png",
    "horizon/visor/hotbar_offhand_selected1" => "hotbar/hotbar_offhand_selected1.png",
    "horizon/visor/movement_state" => "movement_state.png",
    "horizon/visor/warning" => "warning.png",
    "horizon/visor/icon_headset" => "icon_headset.png",
}

/// The wheel's picture for the slot the hand points at (`HotBarSlice.getBackground`: 0 is the
/// middle, 1 to 8 run clockwise from the top; none is the middle's picture).
pub fn hotbar_background(slot: Option<usize>) -> &'static str {
    match slot {
        Some(1) => "horizon/visor/hotbar_top",
        Some(2) => "horizon/visor/hotbar_top_right",
        Some(3) => "horizon/visor/hotbar_right",
        Some(4) => "horizon/visor/hotbar_bottom_right",
        Some(5) => "horizon/visor/hotbar_bottom",
        Some(6) => "horizon/visor/hotbar_bottom_left",
        Some(7) => "horizon/visor/hotbar_left",
        Some(8) => "horizon/visor/hotbar_top_left",
        _ => "horizon/visor/hotbar_default",
    }
}

/// A texture as `(width, height, RGBA8)`.
pub fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = (reader.info().width, reader.info().height);
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let channels = frame.color_type.samples();
    let pixels = buf
        .chunks(channels)
        .take((width * height) as usize)
        .flat_map(|p| match channels {
            4 => [p[0], p[1], p[2], p[3]],
            3 => [p[0], p[1], p[2], 255],
            2 => [p[0], p[0], p[0], p[1]],
            _ => [p[0], p[0], p[0], 255],
        })
        .collect();
    Some((width, height, pixels))
}

/// Every texture, decoded, for the HUD atlas.
pub fn images() -> impl Iterator<Item = (String, u32, u32, Vec<u8>)> {
    TEXTURES.iter().filter_map(|(name, bytes)| {
        let (w, h, pixels) = decode(bytes)?;
        Some(((*name).to_string(), w, h, pixels))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_decodes_at_the_size_visor_ships_it() {
        let size = |name: &str| {
            let bytes = TEXTURES.iter().find(|(n, _)| *n == name).unwrap().1;
            let (w, h, px) = decode(bytes).expect(name);
            assert_eq!(px.len(), (w * h * 4) as usize);
            (w, h)
        };
        // The sizes `VROverlayHotBar` hard-codes: HOTBAR_IMAGE_SIZE 98, a 112 slot-number sheet,
        // 26 pixel selection marks.
        assert_eq!(size("horizon/visor/hotbar_default"), (98, 98));
        assert_eq!(size("horizon/visor/hotbar_slot_numbers"), (112, 112));
        assert_eq!(size("horizon/visor/hotbar_main_selected1"), (26, 26));
        assert_eq!(size("horizon/visor/hotbar_offhand_selected0"), (26, 26));
        assert_eq!(size("horizon/visor/movement_state"), (162, 212));
        assert_eq!(size("horizon/visor/warning"), (16, 16));
        assert_eq!(size("horizon/visor/icon_headset"), (256, 144));
        assert_eq!(images().count(), TEXTURES.len());
    }

    #[test]
    fn a_slot_picks_its_own_picture() {
        let all: std::collections::HashSet<_> =
            (0..9).map(|s| hotbar_background(Some(s))).collect();
        assert_eq!(all.len(), 9);
        assert_eq!(hotbar_background(None), hotbar_background(Some(0)));
    }
}
