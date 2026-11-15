// SPDX-License-Identifier: LGPL-3.0-only
// Copyright (c) 2025-2026 Phoenix-Ra and VisorEssentials contributors.
//! VRInvScreen slot layout and VROverlayInventory's default wrist pose.
use glam::{Quat, Vec3};
pub const HEIGHT: f32 = 156.0;
pub const CANVAS_HEIGHT: f32 = 492.0;
pub fn width(offhand: bool) -> f32 {
    if offhand { 278.0 } else { 258.0 }
}
/// Starling slot IDs -> VRInvScreen positions; container mode hides crafting/equipment.
pub fn slot(id: usize, full: bool, offhand: bool) -> Option<(f32, f32)> {
    let dx = if offhand { 20.0 } else { 0.0 };
    let (x, y) = match id {
        0..9 => [
            (121.0, 38.0),
            (121.0, 11.0),
            (148.0, 11.0),
            (148.0, 38.0),
            (148.0, 65.0),
            (121.0, 65.0),
            (94.0, 65.0),
            (94.0, 38.0),
            (94.0, 11.0),
        ][id],
        9..36 => (
            49.0 + ((id - 9) % 9) as f32 * 18.0,
            96.0 + ((id - 9) / 9) as f32 * 18.0,
        ),
        100..104 if full => (
            181.0 + ((id - 100) % 2) as f32 * 18.0,
            26.0 + ((id - 100) / 2) as f32 * 18.0,
        ),
        200 if full => (237.0, 36.0),
        500..504 if full => return Some((8.0, 8.0 + (id - 500) as f32 * 18.0)),
        504 if full && offhand => return Some((79.0, 62.0)),
        _ => return None,
    };
    Some((x + dx, y))
}
pub fn wrist(position: Vec3, rotation: Quat, scale: f32) -> (Vec3, Quat) {
    (
        position + rotation * Vec3::new(-0.07, -0.081, 0.2) * scale,
        rotation
            * Quat::from_rotation_z(std::f32::consts::PI)
            * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    )
}
pub fn container_height(name: &str, rows: Option<usize>) -> f32 {
    if let Some(rows) = rows {
        return 34.0 + rows as f32 * 18.0;
    }
    if name.contains("merchant") {
        166.0
    } else if name.contains("beacon") {
        130.0
    } else if name.contains("hopper") {
        55.0
    } else if name.contains("anvil") || name.contains("brewing") || name.contains("cartography") {
        89.0
    } else if name.contains("smithing") {
        82.0
    } else if name.contains("enchant") {
        85.0
    } else if name.contains("grindstone") {
        87.0
    } else if name.contains("horse") || name.contains("mount") || name.contains("shulker") {
        88.0
    } else {
        86.0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrist_layout_has_no_overlap_and_matches_original_hotbar_order() {
        assert_eq!(slot(0, true, true), Some((141.0, 38.0)));
        assert_eq!(slot(8, true, true), Some((114.0, 11.0)));
        assert_eq!(slot(35, true, true), Some((213.0, 132.0)));
        assert_eq!(slot(200, true, true), Some((257.0, 36.0)));
        for a in (0..36).chain(100..104).chain([200]).chain(500..505) {
            if let Some((x, y)) = slot(a, true, true) {
                assert!(x >= 0.0 && x + 16.0 <= width(true) && y >= 0.0 && y + 16.0 <= HEIGHT);
                for b in 0..36 {
                    if a != b {
                        let (bx, by) = slot(b, true, true).unwrap();
                        assert!(
                            x + 16.0 <= bx || bx + 16.0 <= x || y + 16.0 <= by || by + 16.0 <= y
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn a_container_keeps_inventory_but_hides_armor_crafting_and_offhand() {
        assert_eq!(slot(9, false, true), Some((69.0, 96.0)));
        for id in [100, 103, 200, 500, 504, 300] {
            assert_eq!(slot(id, false, true), None);
        }
        assert_eq!(slot(504, true, false), None);
    }
}

/// Hit testing uses the same one-pixel expansion as AbstractContainerScreen.isHovering.
pub fn hit(x: f32, y: f32, full: bool, offhand: bool) -> Option<usize> {
    (0..36)
        .chain(100..104)
        .chain([200])
        .chain(500..505)
        .find(|&id| {
            slot(id, full, offhand).is_some_and(|(sx, sy)| {
                x >= sx - 1.0 && x < sx + 17.0 && y >= sy - 1.0 && y < sy + 17.0
            })
        })
}
