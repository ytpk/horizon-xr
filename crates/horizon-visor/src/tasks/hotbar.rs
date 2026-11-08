// SPDX-License-Identifier: MPL-2.0
//! Visor's hotbar wheel (`VROverlayHotBar`, `HotBarSlice`, `TaskHotBar`): hold a grip, move the
//! hand, and the slot the hand points toward is selected; let go and the wheel closes.
//! Copyright (C) Phoenix-Ra and Visor contributors; MPL-2.0 (see LICENSE-MPL-2.0).
//!
//! The wheel has nine slots: 0 in the middle and 1 to 8 around it, clockwise from the top
//! (`HotBarSlice`). Everything here is in the wheel's own pixels, 98 by 98 for the picture, with
//! the origin in its middle and y growing downwards (`getSliceFromPos`).

use glam::{Quat, Vec3};
use std::f64::consts::PI;

/// How far in front of the hand the wheel hangs (`orientPosOffset`'s -0.6 m).
pub const WHEEL_DISTANCE: f32 = 0.6;
/// The wheel's pixels per metre: Visor's overlays are 1 m wide at a GUI 427 pixels wide.
pub const PIXELS_PER_METRE: f32 = 427.0;

/// Where the hand's ray meets the wheel, in the wheel's pixels from its middle (y down), for a wheel
/// that opened with the hand at `origin` turned by `rot` (-Z along the ray): a plane across the
/// ray [`WHEEL_DISTANCE`] ahead, upright. `None` while the ray runs away from the plane.
pub fn wheel_point(origin: Vec3, rot: Quat, hand: Vec3, direction: Vec3) -> Option<(f64, f64)> {
    let forward = rot * Vec3::NEG_Z;
    let right = forward
        .cross(Vec3::Y)
        .try_normalize()
        .unwrap_or(rot * Vec3::X);
    let up = right.cross(forward);
    let centre = origin + forward * WHEEL_DISTANCE;
    let facing = direction.dot(forward);
    if facing < 0.05 {
        return None;
    }
    let t = (centre - hand).dot(forward) / facing;
    if t <= 0.0 {
        return None;
    }
    let hit = hand + direction * t - centre;
    Some((
        f64::from(hit.dot(right) * PIXELS_PER_METRE),
        f64::from(-hit.dot(up) * PIXELS_PER_METRE),
    ))
}

/// `HOTBAR_IMAGE_SIZE`: the wheel's picture is this many pixels square.
pub const WHEEL_SIZE: i32 = 98;
/// `centerRadius`'s default (`VRClientSettings.hotBarCenterRadius`).
pub const DEFAULT_CENTER_RADIUS: f32 = 50.0;
/// `hysteresisMargin`'s default, in degrees.
pub const DEFAULT_HYSTERESIS: f32 = 7.0;
/// `SelectionBoxHotBar.STICKY_RADIUS_MARGIN`.
const STICKY_RADIUS_MARGIN: f64 = 8.0;
/// `SelectionBoxHotBar.SECTOR_HALF_ANGLE`: a side slot owns 45 degrees.
const SECTOR_HALF_ANGLE: f64 = PI / 8.0;

/// Where each slot's item sits in the wheel's picture, top left corner (`new SelectionBoxHotBar(slot,
/// itemX, itemY, angle)`): the order of the slots, 0 in the middle then clockwise from the top.
pub const ITEM_AT: [[i32; 2]; 9] = [
    [41, 41],
    [41, 5],
    [77, 5],
    [77, 41],
    [77, 77],
    [41, 77],
    [5, 77],
    [5, 41],
    [5, 5],
];

/// The direction each side slot is thrown at, radians in `atan2(y, x)` terms with y downwards:
/// the top is -pi/2, the right 0, the bottom pi/2, the left pi (`SelectionBoxHotBar.sectorAngle`).
const SECTOR_ANGLE: [f64; 9] = [
    0.0,
    -PI / 2.0,
    -PI / 4.0,
    0.0,
    PI / 4.0,
    PI / 2.0,
    3.0 * PI / 4.0,
    PI,
    -3.0 * PI / 4.0,
];

/// The wheel's tuning (`center_radius`, `hysteresis_margin` of the Hotbar settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub center_radius: f32,
    /// Degrees.
    pub hysteresis: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            center_radius: DEFAULT_CENTER_RADIUS,
            hysteresis: DEFAULT_HYSTERESIS,
        }
    }
}

fn wrap(angle: f64) -> f64 {
    let a = angle % (2.0 * PI);
    if a > PI {
        a - 2.0 * PI
    } else if a < -PI {
        a + 2.0 * PI
    } else {
        a
    }
}

/// `SelectionBoxHotBar.isInBox` for `slot`; `sticky` is `isInBoxSticky` (a wider margin around the
/// slot that is already selected, so the choice does not flicker between two neighbours).
fn in_box(slot: usize, x: f64, y: f64, tuning: Tuning, sticky: bool) -> bool {
    let (angle_margin, radius_margin) = if sticky {
        (
            f64::from(tuning.hysteresis).to_radians(),
            STICKY_RADIUS_MARGIN,
        )
    } else {
        (0.0, 0.0)
    };
    let center = f64::from(tuning.center_radius);
    let distance = x.hypot(y);
    if slot == 0 {
        return distance <= center + radius_margin;
    }
    if distance < center - radius_margin {
        return false;
    }
    let delta = wrap(y.atan2(x) - SECTOR_ANGLE[slot]);
    delta.abs() <= SECTOR_HALF_ANGLE + angle_margin
}

/// `VROverlayRadialSelector.getSliceFromPos`: the slot at `(x, y)` pixels from the wheel's middle,
/// the `selected` one keeping it while the point stays inside its sticky box, `disabled` slots
/// (the other hand's) refusing. `None` when nothing is there.
pub fn slice_from_pos(
    x: f64,
    y: f64,
    selected: Option<usize>,
    disabled: &[usize],
    tuning: Tuning,
) -> Option<usize> {
    if let Some(s) = selected
        && s < 9
        && !disabled.contains(&s)
        && in_box(s, x, y, tuning, true)
    {
        return Some(s);
    }
    (0..9)
        .find(|&slot| in_box(slot, x, y, tuning, false))
        .filter(|slot| !disabled.contains(slot))
}

/// `TaskHotBar.NOT_SELECTED`.
pub const NOT_SELECTED: i32 = -1;
/// `TaskHotBar.NULL`: no slot to switch back to.
const NULL: i32 = -2;

/// What `TaskHotBar.onRun` reads each tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct HotbarInput {
    /// `VRServerSettings.isTwoHandedVR()`.
    pub two_handed: bool,
    /// The inventory's selected slot (`McInventory.selectedSlot`).
    pub selected: usize,
    /// `decorationRenderer.getHandState(..).isWorldHand()`: the hand is a hand of the world (no
    /// screen has taken it for its pointer).
    pub main_in_world: bool,
    pub off_in_world: bool,
    /// The grip actions (`GameActionHotBar`).
    pub main_pressed: bool,
    pub off_pressed: bool,
    /// `VROverlayHotBar.getSelectedSlice` of each wheel: the slice the hand points at.
    pub main_slice: Option<usize>,
    pub off_slice: Option<usize>,
}

/// What the task does in a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotbarOutput {
    /// The last `McInventory.setSelectedSlot` of the tick.
    pub select: Option<usize>,
    /// `setEnabled` on the wheels: `Some(true)` opens, `Some(false)` closes.
    pub main_wheel: Option<bool>,
    pub off_wheel: Option<bool>,
    /// `HapticFeedback.HOTBAR_SHOW` / `HOTBAR_HIDE` for each hand: `Some(true)` show.
    pub main_haptic: Option<bool>,
    pub off_haptic: Option<bool>,
}

/// `TaskHotBar`: the main hand's wheel picks the selected hotbar slot, the off hand's (two-handed
/// play only) the slot its item comes from, and the two never share one. Choosing the other
/// hand's slot moves that hand away and, while the grip is held, back again if the choice moves on
/// (`slotMainBack`, `slotOffhandBack`).
#[derive(Clone, Copy, Debug)]
pub struct HotbarTask {
    slot_main: i32,
    /// `TaskHotBar.slotOffhand`: -1 until chosen. This is what `OFFHAND_SLOT` carries.
    pub slot_offhand: i32,
    slot_main_back: i32,
    slot_offhand_back: i32,
    reset_data: bool,
    pressed_main: bool,
    pressed_off: bool,
}

impl Default for HotbarTask {
    fn default() -> Self {
        Self {
            slot_main: 0,
            slot_offhand: NOT_SELECTED,
            slot_main_back: NULL,
            slot_offhand_back: NULL,
            reset_data: true,
            pressed_main: false,
            pressed_off: false,
        }
    }
}

impl HotbarTask {
    /// `setOffhandSlot`: the server's remembered slot.
    pub fn set_offhand_slot(&mut self, slot: i32, selected: &mut usize) {
        if slot == self.slot_offhand {
            return;
        }
        self.slot_offhand = slot;
        self.slot_main_back = NULL;
        self.slot_offhand_back = NULL;
        if self.slot_main == self.slot_offhand {
            self.collide(true, false, selected);
        }
    }

    /// `onClear`: the task stops (a screen opened, the player is gone).
    pub fn clear(&mut self) {
        self.pressed_main = false;
        self.pressed_off = false;
        self.slot_main_back = NULL;
        self.slot_offhand_back = NULL;
        self.reset_data = true;
    }

    /// `handleSlotCollision`: the main hand steps to the slot after the off hand's, or the off
    /// hand lets go.
    fn collide(&mut self, main: bool, switchable_back: bool, selected: &mut usize) {
        if main {
            if switchable_back {
                self.slot_main_back = self.slot_main;
            }
            *selected = if self.slot_offhand == 8 {
                0
            } else {
                (self.slot_offhand + 1).clamp(0, 8) as usize
            };
            self.slot_main = *selected as i32;
        } else {
            if switchable_back {
                self.slot_offhand_back = self.slot_offhand;
            }
            self.slot_offhand = NOT_SELECTED;
        }
    }

    /// `onRun`.
    pub fn tick(&mut self, input: HotbarInput) -> HotbarOutput {
        let mut out = HotbarOutput::default();
        let mut selected = input.selected;
        let new_main = selected as i32;
        if self.reset_data || new_main != self.slot_main {
            self.slot_main = new_main;
            self.reset_data = false;
        }
        let slice = |s: Option<usize>| s.map_or(NOT_SELECTED, |s| s as i32);
        if input.two_handed {
            if !input.off_in_world {
                out.off_wheel = Some(false);
            } else {
                if input.off_pressed && !self.pressed_off {
                    out.off_haptic = Some(true);
                    out.off_wheel = Some(true);
                    self.pressed_off = true;
                }
                if self.pressed_off {
                    self.slot_offhand = slice(input.off_slice);
                    if self.slot_offhand != NOT_SELECTED {
                        if self.slot_main_back != NULL && self.slot_offhand != self.slot_main_back {
                            // Switching back.
                            self.slot_main = self.slot_main_back;
                            selected = self.slot_main as usize;
                            self.slot_main_back = NULL;
                        } else if self.slot_offhand == self.slot_main {
                            self.collide(true, true, &mut selected);
                        }
                    }
                    if !input.off_pressed {
                        out.off_haptic = Some(false);
                        out.off_wheel = Some(false);
                        self.pressed_off = false;
                        self.slot_offhand_back = NULL;
                        self.slot_main_back = NULL;
                    }
                }
            }
        }
        if !input.main_in_world {
            out.main_wheel = Some(false);
        } else {
            if input.main_pressed && !self.pressed_main {
                out.main_haptic = Some(true);
                out.main_wheel = Some(true);
                self.pressed_main = true;
            }
            if self.pressed_main {
                self.slot_main = slice(input.main_slice);
                if self.slot_main != NOT_SELECTED {
                    selected = self.slot_main as usize;
                    if self.slot_offhand_back != NULL && self.slot_main != self.slot_offhand_back {
                        self.slot_offhand = self.slot_offhand_back;
                        self.slot_offhand_back = NULL;
                    } else if self.slot_main == self.slot_offhand {
                        self.collide(false, true, &mut selected);
                    }
                }
                if !input.main_pressed {
                    out.main_haptic = Some(false);
                    out.main_wheel = Some(false);
                    self.pressed_main = false;
                    self.slot_offhand_back = NULL;
                    self.slot_main_back = NULL;
                }
            }
            if self.slot_offhand == self.slot_main {
                self.slot_offhand = NOT_SELECTED;
            }
        }
        if selected != input.selected {
            out.select = Some(selected);
        }
        out
    }
}

/// `VROffhandEquipment`'s test (`redirecting`): the off hand is a hotbar slot when the server
/// plays two-handed and the player is a local VR player. The slot is -1 while none is chosen
/// (the off hand is then empty and takes nothing).
pub fn offhand_redirect(two_handed: bool, vr_player: bool, slot: i32) -> Option<i32> {
    (two_handed && vr_player).then_some(if (0..9).contains(&slot) { slot } else { -1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Tuning = Tuning {
        center_radius: 50.0,
        hysteresis: 7.0,
    };

    #[test]
    fn the_middle_is_slot_zero_and_each_direction_beyond_the_radius_is_a_side_slot() {
        assert_eq!(slice_from_pos(0.0, 0.0, None, &[], T), Some(0));
        assert_eq!(slice_from_pos(49.0, 0.0, None, &[], T), Some(0));
        // Just past 50 px: right is 3, down is 5, left is 7, up is 1 (y grows downwards).
        assert_eq!(slice_from_pos(51.0, 0.0, None, &[], T), Some(3));
        assert_eq!(slice_from_pos(0.0, 60.0, None, &[], T), Some(5));
        assert_eq!(slice_from_pos(-60.0, 0.0, None, &[], T), Some(7));
        assert_eq!(slice_from_pos(0.0, -60.0, None, &[], T), Some(1));
        // The corners are 45 degrees: up-right 2, down-right 4, down-left 6, up-left 8.
        assert_eq!(slice_from_pos(50.0, -50.0, None, &[], T), Some(2));
        assert_eq!(slice_from_pos(50.0, 50.0, None, &[], T), Some(4));
        assert_eq!(slice_from_pos(-50.0, 50.0, None, &[], T), Some(6));
        assert_eq!(slice_from_pos(-50.0, -50.0, None, &[], T), Some(8));
    }

    #[test]
    fn the_selected_slot_holds_a_few_degrees_past_its_own_sector() {
        // Straight right is 3; 25 degrees up from it is outside 3's 22.5 degree half sector but
        // inside the 7 degree hysteresis, so 3 is kept; without it the point belongs to 2.
        let (x, y) = (
            60.0 * 25f64.to_radians().cos(),
            -60.0 * 25f64.to_radians().sin(),
        );
        assert_eq!(slice_from_pos(x, y, None, &[], T), Some(2));
        assert_eq!(slice_from_pos(x, y, Some(3), &[], T), Some(3));
        // 35 degrees is past the margin: it moves on.
        let (x, y) = (
            60.0 * 35f64.to_radians().cos(),
            -60.0 * 35f64.to_radians().sin(),
        );
        assert_eq!(slice_from_pos(x, y, Some(3), &[], T), Some(2));
        // The middle keeps up to 8 px past the radius for the selected slot.
        assert_eq!(slice_from_pos(56.0, 0.0, Some(0), &[], T), Some(0));
        assert_eq!(slice_from_pos(59.0, 0.0, Some(0), &[], T), Some(3));
    }

    #[test]
    fn a_slot_the_other_hand_holds_cannot_be_chosen() {
        assert_eq!(slice_from_pos(60.0, 0.0, None, &[3], T), None);
        assert_eq!(slice_from_pos(60.0, 0.0, Some(3), &[3], T), None);
        assert_eq!(slice_from_pos(0.0, 0.0, None, &[3], T), Some(0));
    }

    #[test]
    fn the_radius_is_the_players_to_tune() {
        let wide = Tuning {
            center_radius: 80.0,
            hysteresis: 7.0,
        };
        assert_eq!(slice_from_pos(60.0, 0.0, None, &[], wide), Some(0));
        assert_eq!(slice_from_pos(81.0, 0.0, None, &[], wide), Some(3));
    }

    #[test]
    fn the_hand_ray_is_the_cursor_on_a_wheel_hung_0_6_m_ahead() {
        // The hand at the head's height looking straight ahead (-Z): the ray through the middle.
        let origin = Vec3::new(0.0, 1.2, 0.0);
        let (x, y) = wheel_point(origin, Quat::IDENTITY, origin, Vec3::NEG_Z).unwrap();
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6);
        // Turn the hand 10 degrees to the right: the ray meets the plane 0.6 * tan(10 deg) = 0.1058 m
        // to the right of the middle, 45.2 px at 427 px per metre.
        let turned = Quat::from_rotation_y((-10f32).to_radians());
        let (x, y) = wheel_point(origin, Quat::IDENTITY, origin, turned * Vec3::NEG_Z).unwrap();
        assert!((x - 45.18).abs() < 0.05, "{x}");
        assert!(y.abs() < 1e-4);
        // Up is negative y.
        let up = Quat::from_rotation_x(10f32.to_radians());
        let (_, y) = wheel_point(origin, Quat::IDENTITY, origin, up * Vec3::NEG_Z).unwrap();
        assert!((y + 45.18).abs() < 0.05, "{y}");
        // A wheel opened with the hand turned to the left (90 degrees) is still upright and ahead of it.
        let left = Quat::from_rotation_y(90f32.to_radians());
        let (x, y) = wheel_point(origin, left, origin, left * Vec3::NEG_Z).unwrap();
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4);
        // Pointing away from the plane misses.
        assert_eq!(wheel_point(origin, Quat::IDENTITY, origin, Vec3::Z), None);
    }

    fn two() -> HotbarInput {
        HotbarInput {
            two_handed: true,
            main_in_world: true,
            off_in_world: true,
            ..HotbarInput::default()
        }
    }

    #[test]
    fn the_off_hand_choosing_the_main_hands_slot_moves_the_main_hand_on() {
        let mut t = HotbarTask::default();
        let open = HotbarInput {
            selected: 4,
            off_pressed: true,
            off_slice: Some(4),
            ..two()
        };
        let out = t.tick(open);
        assert_eq!(out.off_wheel, Some(true));
        assert_eq!(out.off_haptic, Some(true));
        assert_eq!(out.select, Some(5));
        assert_eq!(t.slot_offhand, 4);
        // On 8 the main hand wraps to 0.
        let mut t = HotbarTask::default();
        let out = t.tick(HotbarInput {
            selected: 8,
            off_pressed: true,
            off_slice: Some(8),
            ..two()
        });
        assert_eq!((out.select, t.slot_offhand), (Some(0), 8));
    }

    #[test]
    fn moving_the_choice_on_gives_the_main_hand_its_slot_back() {
        let mut t = HotbarTask::default();
        t.tick(HotbarInput {
            selected: 4,
            off_pressed: true,
            off_slice: Some(4),
            ..two()
        });
        // The inventory now selects 5. The off hand points at 7: the main hand returns to 4.
        let out = t.tick(HotbarInput {
            selected: 5,
            off_pressed: true,
            off_slice: Some(7),
            ..two()
        });
        assert_eq!(out.select, Some(4));
        assert_eq!(t.slot_offhand, 7);
        // Letting go closes the wheel and keeps the choice.
        let out = t.tick(HotbarInput {
            selected: 4,
            off_pressed: false,
            off_slice: Some(7),
            ..two()
        });
        assert_eq!(out.off_wheel, Some(false));
        assert_eq!(out.off_haptic, Some(false));
        assert_eq!(t.slot_offhand, 7);
    }

    #[test]
    fn the_main_hand_choosing_the_off_hands_slot_empties_the_off_hand_and_returns_it_later() {
        let mut t = HotbarTask::default();
        let mut sel = 1;
        t.set_offhand_slot(6, &mut sel);
        assert_eq!((t.slot_offhand, sel), (6, 1));
        let held = HotbarInput {
            selected: 1,
            main_pressed: true,
            main_slice: Some(6),
            ..two()
        };
        let out = t.tick(held);
        assert_eq!(out.select, Some(6));
        assert_eq!(t.slot_offhand, NOT_SELECTED);
        // Pointing at another slot: the off hand gets its slot back.
        t.tick(HotbarInput {
            selected: 6,
            main_pressed: true,
            main_slice: Some(2),
            ..two()
        });
        assert_eq!(t.slot_offhand, 6);
    }

    #[test]
    fn a_remembered_slot_the_main_hand_holds_moves_the_main_hand() {
        let mut t = HotbarTask::default();
        let mut sel = 3;
        t.tick(HotbarInput {
            selected: 3,
            ..two()
        });
        t.set_offhand_slot(3, &mut sel);
        assert_eq!(sel, 4);
    }

    #[test]
    fn without_two_handed_play_the_off_wheel_never_opens() {
        let mut t = HotbarTask::default();
        let out = t.tick(HotbarInput {
            two_handed: false,
            selected: 0,
            main_in_world: true,
            off_in_world: true,
            off_pressed: true,
            off_slice: Some(2),
            ..HotbarInput::default()
        });
        assert_eq!(out, HotbarOutput::default());
        assert_eq!(t.slot_offhand, NOT_SELECTED);
    }

    #[test]
    fn the_off_hand_is_a_hotbar_slot_only_for_a_vr_player_on_a_two_handed_server() {
        assert_eq!(offhand_redirect(true, true, 3), Some(3));
        assert_eq!(offhand_redirect(true, true, -1), Some(-1));
        assert_eq!(offhand_redirect(true, true, 40), Some(-1));
        assert_eq!(offhand_redirect(false, true, 3), None);
        assert_eq!(offhand_redirect(true, false, 3), None);
    }
}
